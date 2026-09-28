use anyhow::{Context, Result};
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
    pub pc: Option<u64>,
    pub contains: Option<String>,
}

pub(super) fn resolve_hash(
    storage: &Storage,
    target: Option<&str>,
    run: Option<i64>,
) -> Result<[u8; 32]> {
    let target = target.context("need ELF hash or --run <RUN> <PUBKEY>")?;
    match run {
        Some(id) => crate::runs::store::program_elf_hash(storage, id, &parse_pubkey(target)?),
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
        let elf = trace::program_elf::program_elf_bytes(&accounts, key);
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
        decode::store_disasm(storage, &req.hash)?;
        let rows = storage.db.program_disasm_chunks(&req.hash)?;
        let entries = collect_disasm(storage, &rows, req)?;
        let sliced = slice_skip_head_tail(entries, req.skip, req.head, req.tail);
        let mut obj = serde_json::Map::new();
        for (pc, line) in sliced {
            obj.insert(pc.to_string(), serde_json::Value::String(line));
        }
        Ok(serde_json::Value::Object(obj))
    } else {
        decode::store_lifted(storage, &req.hash)?;
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
        if !overlaps(row, req.start, req.end, req.pc) {
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
            if req.pc.is_some_and(|want| pc != want) {
                continue;
            }
            if req.contains.as_ref().is_some_and(|s| !line.contains(s)) {
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
        if !overlaps(row, req.start, req.end, req.pc) {
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
            if let Some(want) = req.pc {
                if !block_contains_pc(pc, &block, want) {
                    continue;
                }
            } else if !pc_in_window(pc, req.start, req.end) {
                continue;
            }
            if req
                .contains
                .as_ref()
                .is_some_and(|s| !block.to_string().contains(s))
            {
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
    if req.pc.is_some() {
        return Some(req.skip.saturating_add(1));
    }
    if req.tail.is_some() || req.head == 0 {
        return None;
    }
    Some(req.skip.saturating_add(req.head))
}

fn overlaps(row: &ProgramChunk, start: Option<u64>, end: Option<u64>, pc: Option<u64>) -> bool {
    let start_pc = u64::try_from(row.start_pc).unwrap_or(0);
    let end_pc = u64::try_from(row.end_pc).unwrap_or(0);
    if let Some(pc) = pc {
        if pc < start_pc || pc > end_pc {
            return false;
        }
    }
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

fn block_contains_pc(start: u64, block: &serde_json::Value, want: u64) -> bool {
    let end = block
        .get("end")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(start);
    start <= want && want <= end
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

fn parse_elf_hash(s: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(s.trim()).context("ELF hash hex")?;
    <[u8; 32]>::try_from(bytes).map_err(|_| anyhow::anyhow!("ELF hash must be 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "seer-prog-pc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn collect_lifted_pc_does_not_read_later_chunks() {
        let root = tmp();
        let storage = Storage::open_at(&root).unwrap();
        let elf = [7u8; 32];
        let first = serde_json::json!({
            "blocks": {
                "288": {
                    "label": "bb_0x120",
                    "start": 288,
                    "end": 296,
                    "succ": [],
                    "lines": ["r3 = *(u64*)(r2 + 0x8) as u64;"],
                    "line_pcs": [288]
                }
            }
        });
        let h1 = storage
            .blob
            .store(&serde_json::to_vec(&first).unwrap())
            .unwrap();
        storage.db.insert_program(&elf).unwrap();
        storage
            .db
            .insert_program_lifted(
                &elf,
                &[
                    ProgramChunk {
                        start_pc: 288,
                        end_pc: 296,
                        blob_hash: h1,
                    },
                    ProgramChunk {
                        start_pc: 400,
                        end_pc: 500,
                        blob_hash: [0xee; 32],
                    },
                ],
            )
            .unwrap();
        let req = ProgramRequest {
            hash: elf,
            disasm: false,
            skip: 0,
            head: 20,
            tail: None,
            start: None,
            end: None,
            pc: Some(288),
            contains: None,
        };
        let rows = storage.db.program_lifted_chunks(&elf).unwrap();
        let got = collect_lifted(&storage, &rows, &req).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 288);
        std::fs::remove_dir_all(root).ok();
    }
}
