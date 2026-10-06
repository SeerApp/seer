use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use solana_program_runtime::{
    execution_budget::SVMTransactionExecutionBudget,
    invoke_context::InvokeContext,
    solana_sbpf::{
        disassembler::disassemble_instruction, ebpf, elf::Executable, program::BuiltinProgram,
        static_analysis::CfgNode, vm::ContextObject,
    },
};
use solana_svm_feature_set::SVMFeatureSet;
use solana_syscalls::create_program_runtime_environment;

use crate::lift::{is_any_jump, is_unconditional_jump, lift_rust_like_line, LiftRow};
use crate::symbols::{text_section_vma_from_elf, TextFnSymbols};
use crate::JsonChunk;

const CHUNK_INSTRUCTIONS: usize = 1000;

/// `solana-sbpf` ELF relocation uses `function_<insn_pc>` when no real name is registered.
fn is_sbpf_placeholder_callee(name: &str) -> bool {
    name.strip_prefix("function_")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn call_imm_operand_name(line: &str) -> Option<&str> {
    let s = line.trim();
    let rest = s.strip_prefix("call ")?;
    let name_part = rest.split(" @ 0x").next()?.split_whitespace().next()?;
    Some(name_part)
}

/// Byte offset matching llvm-objdump: `text_section_vma + insn.ptr * INSN_SIZE`.
fn instruction_objdump_byte_offset(text_section_vma: u64, insn: &ebpf::Insn) -> Result<u64> {
    let insn_offset = u64::try_from(
        insn.ptr
            .checked_mul(ebpf::INSN_SIZE)
            .context("instruction slot offset overflow")?,
    )
    .map_err(|_| anyhow::anyhow!("instruction slot offset overflow"))?;
    text_section_vma
        .checked_add(insn_offset)
        .context("instruction VMA byte offset overflow")
}

/// Byte address of the `CALL_IMM` target in the same space as llvm-objdump / our JSON keys
/// (`text_section_vma + target_slot * 8`). Uses `insn.ptr` as the current PC so static relative
/// calls match the interpreter (`reg[11]` at the call site).
fn call_imm_destination_byte_offset<C: ContextObject>(
    executable: &Executable<C>,
    insn: &ebpf::Insn,
    text_section_vma: u64,
) -> Result<Option<u64>> {
    if insn.opc != ebpf::CALL_IMM {
        return Ok(None);
    }
    let ver = executable.get_sbpf_version();
    // SBPF v3+: `CALL_IMM` with `src == 0` is an external syscall (`imm` is the loader key), not a
    // PC-relative destination (crates.io `solana-sbpf` 0.14+; no separate `ebpf::SYSCALL` opcode).
    if ver.static_syscalls() && insn.src == 0 {
        return Ok(None);
    }
    let key = ver.calculate_call_imm_target_pc(insn.ptr, insn.imm);

    let target_slot = if ver.static_syscalls() {
        usize::try_from(u64::from(key))
            .map_err(|_| anyhow::anyhow!("CALL_IMM target PC overflow"))?
    } else if let Some((_name, target_pc)) = executable.get_function_registry().lookup_by_key(key) {
        target_pc
    } else {
        return Ok(None);
    };

    let byte_off = u64::try_from(
        target_slot
            .checked_mul(ebpf::INSN_SIZE)
            .context("CALL_IMM target byte offset overflow")?,
    )
    .map_err(|_| anyhow::anyhow!("CALL_IMM target byte offset overflow"))?;
    Ok(Some(
        text_section_vma
            .checked_add(byte_off)
            .context("CALL_IMM destination VMA overflow")?,
    ))
}

fn loader_syscall_name<C: ContextObject>(loader: &BuiltinProgram<C>, key: u32) -> Option<String> {
    loader
        .get_function_registry()
        .lookup_by_key(key)
        .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
}

fn embellish_call_imm_line<C: ContextObject>(
    executable: &Executable<C>,
    insn: &ebpf::Insn,
    text_section_vma: u64,
    text_syms: &TextFnSymbols,
    line: &str,
) -> Result<String> {
    if insn.opc != ebpf::CALL_IMM {
        return Ok(line.to_string());
    }
    let ver = executable.get_sbpf_version();
    let trimmed = line.trim_end();

    if let Some(dest_byte) = call_imm_destination_byte_offset(executable, insn, text_section_vma)? {
        if trimmed.starts_with("syscall") {
            if trimmed.ends_with("[invalid]") || trimmed == "syscall" || trimmed == "syscall " {
                return Ok(format!("syscall 0x{dest_byte:x}"));
            }
            if trimmed.contains(" @ 0x") {
                return Ok(line.to_string());
            }
            return Ok(format!("{line} @ 0x{dest_byte:x}"));
        }

        // BPF-to-BPF / static PC-relative call: prefer ELF symbol at callee VMA.
        if let Some(elf_name) = text_syms.resolve_vaddr(dest_byte) {
            return Ok(format!("call {elf_name} @ 0x{dest_byte:x}"));
        }

        let operand = call_imm_operand_name(trimmed);
        let placeholder = operand.is_some_and(is_sbpf_placeholder_callee);
        let invalid = trimmed.ends_with("[invalid]");

        if placeholder || invalid {
            return Ok(format!("call 0x{dest_byte:x}"));
        }

        if trimmed.contains(" @ 0x") {
            return Ok(line.to_string());
        }
        return Ok(format!("{line} @ 0x{dest_byte:x}"));
    }

    if ver.static_syscalls() && insn.src == 0 {
        return Ok(embellish_syscall_line(executable, insn));
    }

    if !ver.static_syscalls() {
        let key = insn.imm as u32;
        if let Some(name) = loader_syscall_name(executable.get_loader(), key) {
            if trimmed.contains(name.as_str()) && !trimmed.contains("[invalid]") {
                return Ok(line.to_string());
            }
            return Ok(format!("syscall {name} (key {key:#010x})"));
        }
        if trimmed.contains("[invalid]") || trimmed.starts_with("syscall") {
            return Ok(format!("syscall key {key:#010x} [unregistered]"));
        }
    }

    Ok(line.to_string())
}

fn embellish_call_reg_line(line: &str) -> String {
    if !line.trim_start().starts_with("callx") {
        return line.to_string();
    }
    format!("{line}  [indirect callx; target in register at runtime]")
}

fn embellish_syscall_line<C: ContextObject>(
    executable: &Executable<C>,
    insn: &ebpf::Insn,
) -> String {
    let key = insn.imm as u32;
    match loader_syscall_name(executable.get_loader(), key) {
        Some(name) => format!("syscall {name} (key {key:#010x})"),
        None => format!("syscall key {key:#010x} [unregistered]"),
    }
}

/// Byte address of a PC-relative jump target (`ptr + off + 1` in instruction slots), in the same
/// space as llvm-objdump / our JSON keys (`text_section_vma + target_slot * 8`).
///
/// `solana-sbpf`'s disassembler prints the CFG label (`lbb_{pc}` for non–function-entry blocks, or
/// a demangled symbol at function starts). Those `lbb_*` names are VM instruction indices (`pc`),
/// not VMAs—we replace that operand with `0x{vma}` in `embellish_pc_relative_jump_line`.
pub(crate) fn jmp_destination_byte_offset(
    text_section_vma: u64,
    insn: &ebpf::Insn,
) -> Result<Option<u64>> {
    // Opcode names follow `solana-sbpf` (e.g. 0.14.x: `JEQ64_*` / `JEQ32_*`, not legacy `JEQ_*`).
    if !matches!(
        insn.opc,
        ebpf::JA
            | ebpf::JEQ32_IMM
            | ebpf::JEQ32_REG
            | ebpf::JGT32_IMM
            | ebpf::JGT32_REG
            | ebpf::JGE32_IMM
            | ebpf::JGE32_REG
            | ebpf::JLT32_IMM
            | ebpf::JLT32_REG
            | ebpf::JLE32_IMM
            | ebpf::JLE32_REG
            | ebpf::JSET32_IMM
            | ebpf::JSET32_REG
            | ebpf::JNE32_IMM
            | ebpf::JNE32_REG
            | ebpf::JSGT32_IMM
            | ebpf::JSGT32_REG
            | ebpf::JSGE32_IMM
            | ebpf::JSGE32_REG
            | ebpf::JSLT32_IMM
            | ebpf::JSLT32_REG
            | ebpf::JSLE32_IMM
            | ebpf::JSLE32_REG
            | ebpf::JEQ64_IMM
            | ebpf::JEQ64_REG
            | ebpf::JGT64_IMM
            | ebpf::JGT64_REG
            | ebpf::JGE64_IMM
            | ebpf::JGE64_REG
            | ebpf::JLT64_IMM
            | ebpf::JLT64_REG
            | ebpf::JLE64_IMM
            | ebpf::JLE64_REG
            | ebpf::JSET64_IMM
            | ebpf::JSET64_REG
            | ebpf::JNE64_IMM
            | ebpf::JNE64_REG
            | ebpf::JSGT64_IMM
            | ebpf::JSGT64_REG
            | ebpf::JSGE64_IMM
            | ebpf::JSGE64_REG
            | ebpf::JSLT64_IMM
            | ebpf::JSLT64_REG
            | ebpf::JSLE64_IMM
            | ebpf::JSLE64_REG
    ) {
        return Ok(None);
    }
    let target_pc = (insn.ptr as isize)
        .checked_add(insn.off as isize)
        .and_then(|x| x.checked_add(1))
        .context("jump target PC overflow")?;
    if target_pc < 0 {
        return Err(anyhow::anyhow!("jump target PC underflow: {target_pc}"));
    }
    let target_pc = usize::try_from(target_pc)
        .map_err(|_| anyhow::anyhow!("jump target PC does not fit usize"))?;
    let byte_off = u64::try_from(
        target_pc
            .checked_mul(ebpf::INSN_SIZE)
            .context("jump target byte offset overflow")?,
    )
    .map_err(|_| anyhow::anyhow!("jump target byte offset overflow"))?;
    Ok(Some(
        text_section_vma
            .checked_add(byte_off)
            .context("jump destination VMA overflow")?,
    ))
}

fn embellish_pc_relative_jump_line(
    text_section_vma: u64,
    insn: &ebpf::Insn,
    line: &str,
) -> Result<String> {
    let Some(dest_byte) = jmp_destination_byte_offset(text_section_vma, insn)? else {
        return Ok(line.to_string());
    };
    let replacement = format!("0x{dest_byte:x}");
    let trimmed = line.trim();
    let mut parts: Vec<String> = trimmed.split_whitespace().map(String::from).collect();
    let Some(last) = parts.last_mut() else {
        return Ok(line.to_string());
    };
    if *last == replacement {
        return Ok(line.to_string());
    }
    *last = replacement;
    Ok(parts.join(" "))
}

fn embellish_instruction_line<C: ContextObject>(
    executable: &Executable<C>,
    insn: &ebpf::Insn,
    text_section_vma: u64,
    text_syms: &TextFnSymbols,
    line: &str,
) -> Result<String> {
    match insn.opc {
        ebpf::CALL_IMM => {
            embellish_call_imm_line(executable, insn, text_section_vma, text_syms, line)
        }
        ebpf::CALL_REG => Ok(embellish_call_reg_line(line)),
        ebpf::EXIT => Ok(line.to_string()),
        _ => embellish_pc_relative_jump_line(text_section_vma, insn, line),
    }
}

/// Decode `.text` the same way `Analysis::from_executable` does, without building CFG/DFG.
fn decode_text_instructions(program: &[u8]) -> Result<Vec<ebpf::Insn>> {
    match program.len().checked_rem(ebpf::INSN_SIZE) {
        Some(0) => {}
        _ => anyhow::bail!(
            "eBPF program length {} is not a multiple of {}",
            program.len(),
            ebpf::INSN_SIZE
        ),
    }
    let insn_slots = program
        .len()
        .checked_div(ebpf::INSN_SIZE)
        .context("INSN_SIZE is zero")?;
    let mut instructions = Vec::with_capacity(insn_slots);
    let mut insn_ptr: usize = 0;
    while insn_ptr
        .checked_mul(ebpf::INSN_SIZE)
        .is_some_and(|off| off < program.len())
    {
        let mut insn = ebpf::get_insn_unchecked(program, insn_ptr);
        if insn.opc == ebpf::LD_DW_IMM {
            insn_ptr = insn_ptr
                .checked_add(1)
                .context("LD_DW_IMM insn_ptr overflow")?;
            let next_off = insn_ptr
                .checked_mul(ebpf::INSN_SIZE)
                .context("LD_DW_IMM offset overflow")?;
            if next_off >= program.len() {
                break;
            }
            ebpf::augment_lddw_unchecked(program, &mut insn);
        }
        instructions.push(insn);
        insn_ptr = insn_ptr.checked_add(1).context("insn_ptr overflow")?;
    }
    Ok(instructions)
}

pub(crate) fn decode_program(bytes: &[u8]) -> Result<(Vec<JsonChunk>, Vec<LiftRow>)> {
    let text_vma = text_section_vma_from_elf(bytes)?;
    let text_syms = TextFnSymbols::build(bytes)?;
    let feature_set = SVMFeatureSet::all_enabled();
    let compute_budget = SVMTransactionExecutionBudget::new_with_defaults(false);
    let env = create_program_runtime_environment(&feature_set, &compute_budget, false, true)
        .map_err(|e| anyhow::anyhow!("create_program_runtime_environment: {e:?}"))?;
    let loader = Arc::clone(&*env);
    let executable = Executable::<InvokeContext>::load(bytes, loader)
        .map_err(|e| anyhow::anyhow!("ELF load failed: {e:?}"))?;
    let (_program_vm_addr, program) = executable.get_text_bytes();
    let instructions = decode_text_instructions(program)?;
    let cfg_nodes = BTreeMap::<usize, CfgNode>::new();
    let mut chunks = Vec::new();
    let mut lift_rows = Vec::with_capacity(instructions.len());
    for slice in instructions.chunks(CHUNK_INSTRUCTIONS) {
        let (start_pc, end_pc) = chunk_range(text_vma, slice)?;
        let (disasm_json, slice_lifts) =
            build_chunk_outputs(slice, &executable, text_vma, &text_syms, &cfg_nodes)?;
        lift_rows.extend(slice_lifts);
        chunks.push(JsonChunk {
            start_pc,
            end_pc,
            json: Value::Object(disasm_json),
        });
    }
    let want: Vec<u64> = instructions
        .iter()
        .map(|insn| instruction_objdump_byte_offset(text_vma, insn))
        .collect::<Result<Vec<_>>>()?;
    check_disasm_coverage_pcs(&want, &chunks)?;
    Ok((chunks, lift_rows))
}

pub fn disasm_chunks(elf: &[u8]) -> Result<Vec<JsonChunk>> {
    let (chunks, _) = decode_program(elf)?;
    Ok(chunks)
}

pub fn store_disasm(storage: &storage::Storage, elf_hash: &[u8; 32]) -> Result<()> {
    if !storage.db.program_disasm_chunks(elf_hash)?.is_empty() {
        return Ok(());
    }
    let elf = storage.blob.read(elf_hash)?;
    storage.db.insert_program(elf_hash)?;
    write_chunks(storage, elf_hash, &disasm_chunks(&elf)?, true)
}

pub(crate) fn write_chunks(
    storage: &storage::Storage,
    elf_hash: &[u8; 32],
    chunks: &[JsonChunk],
    disasm: bool,
) -> Result<()> {
    let mut rows = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let bytes = serde_json::to_vec(&chunk.json)?;
        let blob_hash = storage.blob.store(&bytes)?;
        rows.push(storage::ProgramChunk {
            start_pc: i64::try_from(chunk.start_pc).context("start_pc fits i64")?,
            end_pc: i64::try_from(chunk.end_pc).context("end_pc fits i64")?,
            blob_hash,
        });
    }
    if disasm {
        storage.db.insert_program_disasm(elf_hash, &rows)?;
    } else {
        storage.db.insert_program_lifted(elf_hash, &rows)?;
    }
    Ok(())
}

fn check_disasm_coverage_pcs(want: &[u64], chunks: &[JsonChunk]) -> Result<()> {
    let mut got = Vec::with_capacity(want.len());
    let mut prev_end: Option<u64> = None;
    for chunk in chunks {
        let obj = chunk
            .json
            .as_object()
            .context("disasm chunk is an object")?;
        if obj.len() > CHUNK_INSTRUCTIONS {
            anyhow::bail!("disasm chunk exceeds {CHUNK_INSTRUCTIONS} insns");
        }
        if let Some(end) = prev_end {
            if chunk.start_pc <= end {
                anyhow::bail!(
                    "disasm chunks overlap or are out of order: {end} then {}",
                    chunk.start_pc
                );
            }
        }
        prev_end = Some(chunk.end_pc);
        let mut pcs: Vec<u64> = obj
            .keys()
            .map(|k| k.parse::<u64>().context("disasm pc key"))
            .collect::<Result<Vec<_>>>()?;
        pcs.sort_unstable();
        got.extend(pcs);
    }
    if got.as_slice() != want {
        anyhow::bail!("disasm chunks do not cover decoded insns");
    }
    Ok(())
}

fn chunk_range(text_vma: u64, chunk: &[ebpf::Insn]) -> Result<(u64, u64)> {
    let first = chunk
        .first()
        .context("instruction chunk unexpectedly empty")?;
    let last = chunk
        .last()
        .context("instruction chunk unexpectedly empty")?;
    Ok((
        instruction_objdump_byte_offset(text_vma, first)?,
        instruction_objdump_byte_offset(text_vma, last)?,
    ))
}

fn build_chunk_outputs(
    chunk: &[ebpf::Insn],
    executable: &Executable<InvokeContext>,
    text_vma: u64,
    text_syms: &TextFnSymbols,
    cfg_nodes: &BTreeMap<usize, CfgNode>,
) -> Result<(Map<String, Value>, Vec<LiftRow>)> {
    let mut disasm_json = Map::new();
    let mut lift_rows = Vec::with_capacity(chunk.len());

    for insn in chunk {
        let pc = instruction_objdump_byte_offset(text_vma, insn)?;
        let line = disassemble_instruction(
            insn,
            insn.ptr,
            cfg_nodes,
            executable.get_function_registry(),
            executable.get_loader(),
            executable.get_sbpf_version(),
        );
        let line = embellish_instruction_line(executable, insn, text_vma, text_syms, &line)?;
        disasm_json.insert(pc.to_string(), Value::String(line.clone()));

        let is_uncond_jump = is_unconditional_jump(insn.opc);
        lift_rows.push(LiftRow {
            pc,
            rust_line: lift_rust_like_line(&line),
            jump_target: jmp_destination_byte_offset(text_vma, insn)?,
            is_cond_jump: is_any_jump(insn.opc) && !is_uncond_jump,
            is_uncond_jump,
            is_exit: insn.opc == ebpf::EXIT,
        });
    }

    Ok((disasm_json, lift_rows))
}
