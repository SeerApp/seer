use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use object::{
    File as ObjectFile, Object, ObjectSection, ObjectSymbol, SymbolKind, SymbolSection,
};
use agave_syscalls::create_program_runtime_environment_v1;
use serde_json::{Map, Value};
use solana_program_runtime::{
    execution_budget::SVMTransactionExecutionBudget,
    invoke_context::InvokeContext,
    solana_sbpf::{
        ebpf,
        elf::Executable,
        program::BuiltinProgram,
        static_analysis::Analysis,
        vm::ContextObject,
    },
};
use solana_svm_feature_set::SVMFeatureSet;

/// `.text` section VMA (`sh_addr`) from the ELF file alone — same basis as llvm-objdump's address
/// column, without extending `solana-sbpf`.
fn text_section_vma_from_elf(elf_bytes: &[u8]) -> Result<u64> {
    let obj = ObjectFile::parse(elf_bytes).context("parse ELF for .text VMA")?;
    Ok(obj
        .section_by_name(".text")
        .map(|s| s.address())
        .unwrap_or(0))
}

/// Maps VMAs in `.text` to linker symbol names so `call` lines name real callees instead of
/// `solana-sbpf` placeholders like `function_<pc>`.
#[derive(Default)]
struct TextFnSymbols {
    /// Sorted, non-overlapping `[start, end)` spans with best name for that range.
    spans: Vec<(u64, u64, String)>,
}

fn prefer_symbol_name(a: &str, b: &str) -> String {
    let score = |s: &str| -> i32 {
        let mut sc = s.len() as i32;
        if s.contains("::") {
            sc += 100;
        }
        if s.starts_with('.') {
            sc -= 30;
        }
        sc
    };
    if score(b) > score(a) {
        b.to_string()
    } else {
        a.to_string()
    }
}

impl TextFnSymbols {
    fn build(elf_bytes: &[u8]) -> Result<Self> {
        let obj = ObjectFile::parse(elf_bytes).context("parse ELF for symbol index")?;
        let Some(text) = obj.section_by_name(".text") else {
            return Ok(Self::default());
        };
        let text_id = text.index();
        let text_vma = text.address();
        let text_size = text.size();
        let text_end = text_vma.saturating_add(text_size);
        let insn_sz = ebpf::INSN_SIZE as u64;

        let mut by_start: BTreeMap<u64, (u64, String)> = BTreeMap::new();

        let mut ingest = |sym: <ObjectFile as Object>::Symbol<'_>| {
            if sym.kind() != SymbolKind::Text && sym.kind() != SymbolKind::Unknown {
                return;
            }
            if sym.section() == SymbolSection::Undefined {
                return;
            }
            if sym.section_index() != Some(text_id) {
                return;
            }
            let Ok(name) = sym.name() else {
                return;
            };
            if name.is_empty() {
                return;
            }

            let mut vma = sym.address();
            if vma < text_vma && vma < text_size {
                vma = text_vma.saturating_add(vma);
            }
            if vma < text_vma || vma >= text_end {
                return;
            }

            let size = sym.size().max(insn_sz);

            match by_start.entry(vma) {
                Entry::Vacant(e) => {
                    e.insert((size, name.to_string()));
                }
                Entry::Occupied(mut e) => {
                    let (sz0, n0) = e.get_mut();
                    *n0 = prefer_symbol_name(n0, name);
                    *sz0 = (*sz0).max(size);
                }
            }
        };

        for sym in obj.symbols() {
            ingest(sym);
        }
        for sym in obj.dynamic_symbols() {
            ingest(sym);
        }

        let mut entries: Vec<(u64, u64, String)> = by_start
            .into_iter()
            .map(|(addr, (sz, name))| (addr, sz, name))
            .collect();
        entries.sort_by_key(|e| e.0);

        for i in 0..entries.len() {
            let next_start = entries.get(i + 1).map(|e| e.0).unwrap_or(text_end);
            let (start, sz, _) = &entries[i];
            let own_end = start.saturating_add(*sz).max(start.saturating_add(insn_sz));
            let end = next_start.min(own_end).max(start.saturating_add(insn_sz));
            entries[i].1 = end.saturating_sub(*start);
        }

        let spans: Vec<(u64, u64, String)> = entries
            .into_iter()
            .map(|(start, span, name)| {
                let end = start.saturating_add(span).min(text_end);
                (start, end.max(start.saturating_add(insn_sz)), name)
            })
            .collect();

        Ok(Self { spans })
    }

    /// Resolve a VMA inside `.text` to a function symbol name, if covered by a span.
    fn resolve_vaddr(&self, vma: u64) -> Option<&str> {
        let i = self.spans.partition_point(|(start, _, _)| *start <= vma);
        if i == 0 {
            return None;
        }
        let (start, end, name) = &self.spans[i - 1];
        if vma >= *start && vma < *end {
            Some(name.as_str())
        } else {
            None
        }
    }
}

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
        insn
            .ptr
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
        usize::try_from(u64::from(key)).map_err(|_| anyhow::anyhow!("CALL_IMM target PC overflow"))?
    } else if let Some((_name, target_pc)) =
        executable.get_function_registry().lookup_by_key(key)
    {
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

fn loader_syscall_name<C: ContextObject>(
    loader: &BuiltinProgram<C>,
    key: u32,
) -> Option<String> {
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

    if let Some(dest_byte) = call_imm_destination_byte_offset(executable, insn, text_section_vma)?
    {
        if trimmed.starts_with("syscall") {
            if trimmed.ends_with("[invalid]")
                || trimmed == "syscall"
                || trimmed == "syscall "
            {
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

fn embellish_syscall_line<C: ContextObject>(executable: &Executable<C>, insn: &ebpf::Insn) -> String {
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
fn jmp_destination_byte_offset(text_section_vma: u64, insn: &ebpf::Insn) -> Result<Option<u64>> {
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
        ebpf::CALL_IMM => embellish_call_imm_line(executable, insn, text_section_vma, text_syms, line),
        ebpf::CALL_REG => Ok(embellish_call_reg_line(line)),
        ebpf::EXIT => Ok(line.to_string()),
        _ => embellish_pc_relative_jump_line(text_section_vma, insn, line),
    }
}

const CHUNK_INSTRUCTIONS: usize = 1000;

fn main() {
    if let Err(e) = run() {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .context("usage: disasm <path-to-program.so>")?;
    let path = Path::new(&path);
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let text_vma = text_section_vma_from_elf(&bytes)?;
    let text_syms = TextFnSymbols::build(&bytes)?;
    let feature_set = SVMFeatureSet::all_enabled();
    let compute_budget = SVMTransactionExecutionBudget::new_with_defaults(false);
    let loader = create_program_runtime_environment_v1(
        &feature_set,
        &compute_budget,
        false,
        true,
    )
    .map_err(|e| anyhow::anyhow!("create_program_runtime_environment_v1: {e:?}"))?;
    let loader = Arc::new(loader);
    let executable = Executable::<InvokeContext>::load(&bytes, loader)
        .map_err(|e| anyhow::anyhow!("ELF load failed: {e:?}"))?;
    let analysis = Analysis::from_executable(&executable)
        .map_err(|e| anyhow::anyhow!("analysis failed: {e:?}"))?;

    let out_dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("input path must have a UTF-8 filename")?;

    for (_chunk_idx, chunk) in analysis.instructions.chunks(CHUNK_INSTRUCTIONS).enumerate() {
        let start_byte = instruction_objdump_byte_offset(text_vma, chunk.first().unwrap())?;
        let end_byte = instruction_objdump_byte_offset(text_vma, chunk.last().unwrap())?;

        let mut map = Map::new();
        for insn in chunk {
            let byte_offset = instruction_objdump_byte_offset(text_vma, insn)?;
            let line = analysis.disassemble_instruction(insn, insn.ptr);
            let line = embellish_instruction_line(&executable, insn, text_vma, &text_syms, &line)?;
            map.insert(byte_offset.to_string(), Value::String(line));
        }

        let out_name = format!("{stem}_{start_byte}_{end_byte}.json");
        let out_path = out_dir.join(out_name);
        let file = File::create(&out_path)
            .with_context(|| format!("create {}", out_path.display()))?;
        let mut w = BufWriter::new(file);
        serde_json::to_writer(&mut w, &Value::Object(map))
            .with_context(|| format!("write {}", out_path.display()))?;
        w.flush().with_context(|| format!("flush {}", out_path.display()))?;
    }

    Ok(())
}
