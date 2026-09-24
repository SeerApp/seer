use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use object::{File as ObjectFile, Object, ObjectSection, ObjectSymbol, SymbolKind, SymbolSection};
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
            sc = sc.saturating_add(100);
        }
        if s.starts_with('.') {
            sc = sc.saturating_sub(30);
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
            let next_start = entries
                .get(i.saturating_add(1))
                .map(|e| e.0)
                .unwrap_or(text_end);
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
        let (start, end, name) = &self.spans[i.saturating_sub(1)];
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
        ebpf::CALL_IMM => {
            embellish_call_imm_line(executable, insn, text_section_vma, text_syms, line)
        }
        ebpf::CALL_REG => Ok(embellish_call_reg_line(line)),
        ebpf::EXIT => Ok(line.to_string()),
        _ => embellish_pc_relative_jump_line(text_section_vma, insn, line),
    }
}

#[derive(Clone)]
struct LiftRow {
    pc: u64,
    rust_line: String,
    jump_target: Option<u64>,
    is_cond_jump: bool,
    is_uncond_jump: bool,
    is_exit: bool,
}

fn is_unconditional_jump(opc: u8) -> bool {
    opc == ebpf::JA
}

fn is_any_jump(opc: u8) -> bool {
    jmp_destination_byte_offset(
        0,
        &ebpf::Insn {
            ptr: 0,
            opc,
            dst: 0,
            src: 0,
            off: 0,
            imm: 0,
        },
    )
    .ok()
    .flatten()
    .is_some()
}

fn jump_cmp(op: &str) -> Option<&'static str> {
    match op {
        "jeq" | "jeq32" | "jeq64" => Some("=="),
        "jne" | "jne32" | "jne64" => Some("!="),
        "jgt" | "jgt32" | "jgt64" => Some(">"),
        "jge" | "jge32" | "jge64" => Some(">="),
        "jlt" | "jlt32" | "jlt64" => Some("<"),
        "jle" | "jle32" | "jle64" => Some("<="),
        "jset" | "jset32" | "jset64" => Some("&"),
        "jsgt" | "jsgt32" | "jsgt64" => Some(">"),
        "jsge" | "jsge32" | "jsge64" => Some(">="),
        "jslt" | "jslt32" | "jslt64" => Some("<"),
        "jsle" | "jsle32" | "jsle64" => Some("<="),
        _ => None,
    }
}

fn split_op_and_args(line: &str) -> (&str, &str) {
    let trimmed = line.trim();
    match trimmed.split_once(' ') {
        Some((op, rest)) => (op.trim(), rest.trim()),
        None => (trimmed, ""),
    }
}

fn split_two_args(args: &str) -> Option<(&str, &str)> {
    let (a, b) = args.split_once(',')?;
    Some((a.trim(), b.trim()))
}

fn parse_mem_operand(operand: &str) -> Option<(String, String)> {
    let inner = operand.strip_prefix('[')?.strip_suffix(']')?.trim();
    if let Some((base, off)) = inner.split_once('+') {
        return Some((base.trim().to_string(), off.trim().to_string()));
    }
    if let Some((base, off)) = inner.rsplit_once('-') {
        return Some((base.trim().to_string(), format!("-{}", off.trim())));
    }
    Some((inner.to_string(), "0".to_string()))
}

fn width_suffix_to_c_type(sfx: &str) -> Option<&'static str> {
    match sfx {
        "b" => Some("u8"),
        "h" => Some("u16"),
        "w" => Some("u32"),
        "dw" => Some("u64"),
        _ => None,
    }
}

fn lift_rust_like_line(line: &str) -> String {
    let (op, args) = split_op_and_args(line);
    if op.is_empty() {
        return "// <empty>".to_string();
    }

    if op == "exit" {
        return "return;".to_string();
    }
    if op == "callx" {
        return format!("call_indirect({args});");
    }
    if op == "call" {
        return format!("call(/* {args} */);");
    }
    if op == "syscall" {
        return format!("syscall(/* {args} */);");
    }
    if op == "ja" {
        return format!("goto {args};");
    }

    if let Some(sfx) = op.strip_prefix("ldx") {
        if let Some((dst, src_mem)) = split_two_args(args) {
            if let Some((base, off)) = parse_mem_operand(src_mem) {
                if let Some(c_ty) = width_suffix_to_c_type(sfx) {
                    return format!("{dst} = *({c_ty}*)({base} + {off}) as u64;");
                }
            }
        }
    }
    if let Some(sfx) = op.strip_prefix("stx") {
        if let Some((dst_mem, src)) = split_two_args(args) {
            if let Some((base, off)) = parse_mem_operand(dst_mem) {
                if let Some(c_ty) = width_suffix_to_c_type(sfx) {
                    return format!("*({c_ty}*)({base} + {off}) = {src} as {c_ty};");
                }
            }
        }
    }
    if let Some(sfx) = op.strip_prefix("st") {
        if let Some((dst_mem, imm)) = split_two_args(args) {
            if let Some((base, off)) = parse_mem_operand(dst_mem) {
                if let Some(c_ty) = width_suffix_to_c_type(sfx) {
                    return format!("*({c_ty}*)({base} + {off}) = {imm} as {c_ty};");
                }
            }
        }
    }

    if op.starts_with('j') && op != "ja" {
        let parts: Vec<&str> = args.split(',').map(|s| s.trim()).collect();
        if parts.len() >= 3 {
            let lhs = parts[0];
            let rhs = parts[1];
            let target = parts[2];
            if let Some(cmp) = jump_cmp(op) {
                if cmp == "&" {
                    return format!("if (({lhs} & {rhs}) != 0) {{ goto {target}; }}");
                }
                return format!("if ({lhs} {cmp} {rhs}) {{ goto {target}; }}");
            }
        }
    }

    if let Some((dst, rhs)) = split_two_args(args) {
        // Covers mov/add/sub/mul/div/mod/or/and/xor/lsh/rsh/arsh/neg with width suffixes.
        if op.starts_with("mov") {
            return format!("{dst} = {rhs};");
        }
        if op.starts_with("add") {
            return format!("{dst} = {dst}.wrapping_add({rhs});");
        }
        if op.starts_with("sub") {
            return format!("{dst} = {dst}.wrapping_sub({rhs});");
        }
        if op.starts_with("mul") {
            return format!("{dst} = {dst}.wrapping_mul({rhs});");
        }
        if op.starts_with("div") {
            return format!("{dst} = {dst} / {rhs};");
        }
        if op.starts_with("mod") {
            return format!("{dst} = {dst} % {rhs};");
        }
        if op.starts_with("and") {
            return format!("{dst} &= {rhs};");
        }
        if op.starts_with("or") {
            return format!("{dst} |= {rhs};");
        }
        if op.starts_with("xor") {
            return format!("{dst} ^= {rhs};");
        }
        if op.starts_with("lsh") {
            return format!("{dst} <<= {rhs};");
        }
        if op.starts_with("rsh") {
            return format!("{dst} >>= {rhs};");
        }
        if op.starts_with("arsh") {
            return format!("{dst} = (({dst} as i64) >> {rhs}) as u64;");
        }
    }

    format!("/* {line} */")
}

fn build_block_starts(rows: &[LiftRow]) -> Vec<u64> {
    let mut leaders = std::collections::BTreeSet::new();
    if let Some(first) = rows.first() {
        leaders.insert(first.pc);
    }
    for (i, row) in rows.iter().enumerate() {
        if let Some(t) = row.jump_target {
            leaders.insert(t);
        }
        if (row.is_cond_jump || row.is_uncond_jump || row.is_exit)
            && i.saturating_add(1) < rows.len()
        {
            leaders.insert(rows[i.saturating_add(1)].pc);
        }
    }
    leaders.into_iter().collect()
}

const CHUNK_INSTRUCTIONS: usize = 1000;
const CHUNK_BLOCKS: usize = 1000;

pub struct JsonChunk {
    pub start_pc: u64,
    pub end_pc: u64,
    pub json: Value,
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

fn decode_program(bytes: &[u8]) -> Result<(Vec<JsonChunk>, Vec<LiftRow>)> {
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

pub fn lifted_chunks(elf: &[u8]) -> Result<Vec<JsonChunk>> {
    let (_, lift_rows) = decode_program(elf)?;
    let chunks = lift_rows_to_chunks(&lift_rows)?;
    check_lifted_coverage(&lift_rows, &chunks)?;
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

pub fn store_lifted(storage: &storage::Storage, elf_hash: &[u8; 32]) -> Result<()> {
    if !storage.db.program_lifted_chunks(elf_hash)?.is_empty() {
        return Ok(());
    }
    let elf = storage.blob.read(elf_hash)?;
    storage.db.insert_program(elf_hash)?;
    write_chunks(storage, elf_hash, &lifted_chunks(&elf)?, false)
}

fn write_chunks(
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

fn lift_rows_to_chunks(lift_rows: &[LiftRow]) -> Result<Vec<JsonChunk>> {
    let blocks = build_lifted_blocks_json(lift_rows);
    let keys: Vec<String> = blocks.keys().cloned().collect();
    let mut chunks = Vec::new();
    for slice in keys.chunks(CHUNK_BLOCKS) {
        let mut part = Map::new();
        for key in slice {
            if let Some(block) = blocks.get(key) {
                part.insert(key.clone(), block.clone());
            }
        }
        let start_pc = slice
            .first()
            .and_then(|k| k.parse::<u64>().ok())
            .context("lifted chunk start")?;
        let end_pc = part
            .values()
            .filter_map(|b| b.get("end").and_then(Value::as_u64))
            .max()
            .unwrap_or(start_pc);
        chunks.push(JsonChunk {
            start_pc,
            end_pc,
            json: serde_json::json!({ "blocks": Value::Object(part) }),
        });
    }
    Ok(chunks)
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

fn check_lifted_coverage(lift_rows: &[LiftRow], chunks: &[JsonChunk]) -> Result<()> {
    let want: Vec<String> = build_lifted_blocks_json(lift_rows)
        .keys()
        .cloned()
        .collect();
    let mut got = Vec::new();
    for chunk in chunks {
        let blocks = chunk
            .json
            .get("blocks")
            .and_then(Value::as_object)
            .context("lifted chunk has blocks")?;
        if blocks.len() > CHUNK_BLOCKS {
            anyhow::bail!("lifted chunk exceeds {CHUNK_BLOCKS} blocks");
        }
        let mut keys: Vec<String> = blocks.keys().cloned().collect();
        keys.sort_by_key(|k| k.parse::<u64>().unwrap_or(0));
        got.extend(keys);
    }
    if got != want {
        anyhow::bail!("lifted chunks do not cover decoded blocks");
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

fn build_lifted_blocks_json(lift_rows: &[LiftRow]) -> Map<String, Value> {
    let mut blocks_json = Map::new();
    let leaders = build_block_starts(lift_rows);

    for (i, start) in leaders.iter().enumerate() {
        let end_exclusive = leaders
            .get(i.saturating_add(1))
            .copied()
            .unwrap_or(u64::MAX);
        let block_rows: Vec<&LiftRow> = lift_rows
            .iter()
            .filter(|r| r.pc >= *start && r.pc < end_exclusive)
            .collect();
        if block_rows.is_empty() {
            continue;
        }

        let mut succ = Vec::new();
        if let Some(last) = block_rows.last() {
            if let Some(t) = last.jump_target {
                succ.push(format!("bb_0x{t:x}"));
            }
            if last.is_cond_jump || (!last.is_uncond_jump && !last.is_exit) {
                if let Some(fallthrough_pc) =
                    lift_rows.iter().find(|r| r.pc > last.pc).map(|r| r.pc)
                {
                    succ.push(format!("bb_0x{fallthrough_pc:x}"));
                }
            }
        }

        blocks_json.insert(
            start.to_string(),
            serde_json::json!({
                "label": format!("bb_0x{start:x}"),
                "start": start,
                "end": block_rows.last().map(|r| r.pc).unwrap_or(*start),
                "succ": succ,
                "lines": block_rows.iter().map(|r| r.rust_line.clone()).collect::<Vec<_>>(),
                "line_pcs": block_rows.iter().map(|r| r.pc).collect::<Vec<_>>(),
            }),
        );
    }

    blocks_json
}
