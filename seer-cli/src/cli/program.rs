use anyhow::{bail, Context, Result};
use solana_pubkey::Pubkey;
use storage::{ProgramChunk, Storage};

use crate::state_accounts::StateAccounts;

use super::format::slice_skip_head_tail;
use super::parse_pubkey;

pub(super) struct ProgramRequest {
    pub hash: [u8; 32],
    pub disasm: bool,
    pub skip: usize,
    pub head: usize,
    pub tail: Option<usize>,
    pub start: Option<u64>,
    pub end: Option<u64>,
}

pub(super) fn resolve_hash(
    storage: &Storage,
    target: Option<&str>,
    run: Option<i64>,
) -> Result<[u8; 32]> {
    let target = target.context("need ELF hash or --run <RUN> <PUBKEY>")?;
    match run {
        Some(id) => elf_hash_for_run(storage, id, &parse_pubkey(target)?),
        None => parse_elf_hash(target),
    }
}

pub(super) fn show_programs(
    storage: &Storage,
    row: &storage::RunRow,
    subset: &[Pubkey],
) -> Result<serde_json::Value> {
    let state = StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash)?)?;
    let accounts = crate::runs::store::account_datas(storage, &state)?;
    let mut map = serde_json::Map::new();
    for (key, account) in &state.0 {
        if !account.executable {
            continue;
        }
        if !subset.is_empty() && !subset.contains(key) {
            continue;
        }
        let elf = seer_core::program_elf::program_elf_bytes(&accounts, key);
        if elf.is_empty() {
            continue;
        }
        let elf_hash = storage.blob.hash(&elf);
        let idl = storage
            .db
            .program_idl_blob_hash(&elf_hash)?
            .map(hex::encode);
        map.insert(
            key.to_string(),
            serde_json::json!({
                "elf": hex::encode(elf_hash),
                "idl": idl,
            }),
        );
    }
    Ok(serde_json::Value::Object(map))
}

pub(super) fn emit_body(storage: &Storage, req: &ProgramRequest) -> Result<serde_json::Value> {
    if req.disasm {
        disasm::store_disasm(storage, &req.hash)?;
        let rows = storage.db.program_disasm_chunks(&req.hash)?;
        let entries = collect_disasm(storage, &rows, req)?;
        let sliced = slice_skip_head_tail(entries, req.skip, req.head, req.tail);
        let mut obj = serde_json::Map::new();
        for (pc, line) in sliced {
            obj.insert(pc.to_string(), serde_json::Value::String(line));
        }
        Ok(serde_json::Value::Object(obj))
    } else {
        disasm::store_lifted(storage, &req.hash)?;
        let rows = storage.db.program_lifted_chunks(&req.hash)?;
        let entries = collect_lifted(storage, &rows, req)?;
        let sliced = slice_skip_head_tail(entries, req.skip, req.head, req.tail);
        let mut blocks = serde_json::Map::new();
        for (pc, block) in sliced {
            blocks.insert(pc.to_string(), block);
        }
        Ok(serde_json::json!({ "blocks": serde_json::Value::Object(blocks) }))
    }
}

fn collect_disasm(
    storage: &Storage,
    rows: &[ProgramChunk],
    req: &ProgramRequest,
) -> Result<Vec<(u64, String)>> {
    let mut out = Vec::new();
    let need = needed_count(req);
    for row in rows {
        if !overlaps(row, req.start, req.end) {
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(&storage.blob.read(&row.blob_hash)?)?;
        let obj = value.as_object().context("disasm chunk is an object")?;
        let mut pcs: Vec<(u64, String)> = obj
            .iter()
            .map(|(k, v)| {
                let pc = k.parse::<u64>().context("disasm pc")?;
                let line = v.as_str().context("disasm line")?.to_string();
                Ok((pc, line))
            })
            .collect::<Result<Vec<_>>>()?;
        pcs.sort_by_key(|(pc, _)| *pc);
        for (pc, line) in pcs {
            if !pc_in_window(pc, req.start, req.end) {
                continue;
            }
            out.push((pc, line));
            if let Some(n) = need {
                if out.len() >= n {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

fn collect_lifted(
    storage: &Storage,
    rows: &[ProgramChunk],
    req: &ProgramRequest,
) -> Result<Vec<(u64, serde_json::Value)>> {
    let mut out = Vec::new();
    let need = needed_count(req);
    for row in rows {
        if !overlaps(row, req.start, req.end) {
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(&storage.blob.read(&row.blob_hash)?)?;
        let blocks = value
            .get("blocks")
            .and_then(serde_json::Value::as_object)
            .context("lifted chunk has blocks")?;
        let mut keys: Vec<(u64, serde_json::Value)> = blocks
            .iter()
            .map(|(k, v)| {
                let pc = k.parse::<u64>().context("block pc")?;
                Ok((pc, v.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        keys.sort_by_key(|(pc, _)| *pc);
        for (pc, block) in keys {
            if !pc_in_window(pc, req.start, req.end) {
                continue;
            }
            out.push((pc, block));
            if let Some(n) = need {
                if out.len() >= n {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

fn needed_count(req: &ProgramRequest) -> Option<usize> {
    if req.tail.is_some() || req.head == 0 {
        return None;
    }
    Some(req.skip.saturating_add(req.head))
}

fn overlaps(row: &ProgramChunk, start: Option<u64>, end: Option<u64>) -> bool {
    let start_pc = u64::try_from(row.start_pc).unwrap_or(0);
    let end_pc = u64::try_from(row.end_pc).unwrap_or(0);
    if let Some(start) = start {
        if end_pc < start {
            return false;
        }
    }
    if let Some(end) = end {
        if start_pc > end {
            return false;
        }
    }
    true
}

fn pc_in_window(pc: u64, start: Option<u64>, end: Option<u64>) -> bool {
    if let Some(start) = start {
        if pc < start {
            return false;
        }
    }
    if let Some(end) = end {
        if pc > end {
            return false;
        }
    }
    true
}

fn elf_hash_for_run(storage: &Storage, run_id: i64, pk: &Pubkey) -> Result<[u8; 32]> {
    let row = storage.db.get_run(run_id)?;
    let state = StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash)?)?;
    let accounts = crate::runs::store::account_datas(storage, &state)?;
    let elf = seer_core::program_elf::program_elf_bytes(&accounts, pk);
    if elf.is_empty() {
        bail!("no ELF for {pk} in run {run_id}");
    }
    let hash = storage.blob.store(&elf)?;
    storage.db.insert_program(&hash)?;
    Ok(hash)
}

fn parse_elf_hash(s: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(s.trim()).context("ELF hash hex")?;
    <[u8; 32]>::try_from(bytes).map_err(|_| anyhow::anyhow!("ELF hash must be 32 bytes"))
}
