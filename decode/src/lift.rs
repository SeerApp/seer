use anyhow::{Context, Result};
use serde_json::{Map, Value};
use solana_program_runtime::solana_sbpf::ebpf;

use crate::disasm::{decode_program, jmp_destination_byte_offset, write_chunks};
use crate::JsonChunk;

const CHUNK_BLOCKS: usize = 1000;

#[derive(Clone)]
pub(crate) struct LiftRow {
    pub(crate) pc: u64,
    pub(crate) rust_line: String,
    pub(crate) jump_target: Option<u64>,
    pub(crate) is_cond_jump: bool,
    pub(crate) is_uncond_jump: bool,
    pub(crate) is_exit: bool,
}

pub(crate) fn is_unconditional_jump(opc: u8) -> bool {
    opc == ebpf::JA
}

pub(crate) fn is_any_jump(opc: u8) -> bool {
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

pub(crate) fn lift_rust_like_line(line: &str) -> String {
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

pub fn lifted_chunks(elf: &[u8]) -> Result<Vec<JsonChunk>> {
    let (_, lift_rows) = decode_program(elf)?;
    let chunks = lift_rows_to_chunks(&lift_rows)?;
    check_lifted_coverage(&lift_rows, &chunks)?;
    Ok(chunks)
}

pub fn store_lifted(storage: &storage::Storage, elf_hash: &[u8; 32]) -> Result<()> {
    if !storage.db.program_lifted_chunks(elf_hash)?.is_empty() {
        return Ok(());
    }
    let elf = storage.blob.read(elf_hash)?;
    storage.db.insert_program(elf_hash)?;
    write_chunks(storage, elf_hash, &lifted_chunks(&elf)?, false)
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
