use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use anyhow::{Context, Result, bail};
use storage::{ProgramChunk, Storage};

use crate::coverage::Skip;
use crate::reg::{RegisterTrace, RegisterTraceChunk};
use crate::step::Step;
use crate::vm::Vm;

pub fn store(storage: &Storage, run_id: i64, ix: i64, force: bool) -> Result<Vec<u8>> {
    let _ = storage.db.get_run(run_id)?;
    let ixs = storage.db.list_run_ix(run_id)?;
    if !ixs.iter().any(|(i, _)| *i == ix) {
        let listed: Vec<String> = ixs.iter().map(|(i, _)| i.to_string()).collect();
        let have = if listed.is_empty() {
            "none".into()
        } else {
            listed.join(", ")
        };
        bail!("no ix {ix} on run {run_id}; have {have}");
    }
    if !force {
        if let Some(hash) = storage.db.glassbox_blob_hash(run_id, ix)? {
            return storage.blob.read(&hash);
        }
    }
    let bytes = analyze_ix(storage, run_id, ix)?;
    let hash = storage.blob.store(&bytes)?;
    storage.db.upsert_glassbox(run_id, ix, &hash)?;
    Ok(bytes)
}

fn analyze_ix(storage: &Storage, run_id: i64, ix: i64) -> Result<Vec<u8>> {
    let rows: Vec<_> = storage
        .db
        .list_reg(run_id)?
        .into_iter()
        .filter(|(row_ix, _, _, _, _, _)| *row_ix == ix)
        .collect();
    if rows.is_empty() {
        bail!("no register trace for run {run_id} ix {ix}");
    }

    let mut groups: BTreeMap<String, ([u8; 32], Vec<[u8; 32]>)> = BTreeMap::new();
    for (_ix, _start, _end, self_h, prog_h, pk) in rows {
        let key = bs58::encode(pk).into_string();
        groups.entry(key).or_insert((prog_h, Vec::new())).1.push(self_h);
    }

    let mut skips = Vec::new();
    let mut runs = Vec::new();
    for (program_id, (elf, blobs)) in &groups {
        match load_program(storage, ix, program_id, elf, blobs, &mut skips) {
            Ok(steps) => runs.push(steps),
            Err(e) => {
                let ix_u8 = u8::try_from(ix).unwrap_or(u8::MAX);
                skips.push(Skip::program_load(ix_u8, program_id, format!("{e:#}")));
            }
        }
    }

    let missing_disasm: usize = skips
        .iter()
        .filter(|s| s.reason == crate::coverage::SkipReason::EmptyDisasm)
        .count();
    let steps: Vec<Step> = runs.into_iter().flatten().collect();
    let steps_applied = steps.len();
    let mut vm = Vm::new();
    let _ = vm.run(steps.iter());

    let analysis = vm.into_analysis();
    let mut out = Cursor::new(Vec::new());
    analysis.write(&mut out, run_id, ix, steps_applied, missing_disasm)?;
    Ok(out.into_inner())
}

fn load_program(
    storage: &Storage,
    _ix: i64,
    _program_id: &str,
    elf: &[u8; 32],
    blobs: &[[u8; 32]],
    skips: &mut Vec<Skip>,
) -> Result<Vec<Step>> {
    let mut chunks = Vec::with_capacity(blobs.len());
    for hash in blobs {
        let bytes = storage.blob.read(hash)?;
        let chunk: RegisterTraceChunk =
            serde_json::from_slice(&bytes).context("parse register chunk")?;
        chunks.push(chunk);
    }
    let trace = RegisterTrace::from_chunks(&chunks)?;
    let pcs = trace.pcs();
    let disasm = disasm_for_pcs(storage, elf, &pcs)?;
    let mut kept = Vec::new();
    for step in trace.into_steps(&disasm) {
        if step.disasm.is_empty() {
            skips.push(Skip::empty_disasm(&step));
        } else {
            kept.push(step);
        }
    }
    Ok(kept)
}

fn disasm_for_pcs(
    storage: &Storage,
    elf: &[u8; 32],
    pcs: &[u64],
) -> Result<BTreeMap<u64, String>> {
    decode::store_disasm(storage, elf)?;
    let rows = storage.db.program_disasm_chunks(elf)?;
    if rows.is_empty() {
        bail!("no disasm chunks");
    }
    let mut want = BTreeSet::new();
    for pc in pcs {
        let pc_i = i64::try_from(*pc).unwrap_or(i64::MAX);
        for row in &rows {
            if chunk_covers(row, pc_i) {
                want.insert(row.blob_hash);
            }
        }
    }
    let mut map = BTreeMap::new();
    for row in &rows {
        if !want.contains(&row.blob_hash) {
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(&storage.blob.read(&row.blob_hash)?)?;
        let obj = value.as_object().context("disasm chunk is an object")?;
        for (k, v) in obj {
            let pc: u64 = k.parse().context("disasm pc")?;
            if let Some(line) = v.as_str() {
                map.insert(pc, line.to_string());
            }
        }
    }
    Ok(map)
}

fn chunk_covers(row: &ProgramChunk, pc: i64) -> bool {
    row.start_pc <= pc && pc <= row.end_pc
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp_storage() -> (storage::Storage, std::path::PathBuf) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "seer-glassbox-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        (storage::Storage::open_at(&p).unwrap(), p)
    }

    fn new_run(storage: &storage::Storage) -> i64 {
        let h = storage.blob.store(b"x").unwrap();
        storage.db.insert_simulation(&h, &h).unwrap();
        let id = storage.db.insert_run(&h, &h, "{}", None, "[]", "").unwrap();
        storage.db.insert_run_ix(id, 0).unwrap();
        id
    }

    #[test]
    fn missing_ix_lists_what_exists() {
        let (storage, root) = tmp_storage();
        let id = new_run(&storage);
        let err = store(&storage, id, 1, false).unwrap_err().to_string();
        assert!(err.contains("have 0"), "{err}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn no_reg_is_an_error() {
        let (storage, root) = tmp_storage();
        let id = new_run(&storage);
        let err = store(&storage, id, 0, false).unwrap_err().to_string();
        assert!(err.contains("no register trace"), "{err}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn force_replaces_stored_blob() {
        let (storage, root) = tmp_storage();
        let id = new_run(&storage);
        let a = storage.blob.store(b"{\"run\":1}").unwrap();
        storage.db.upsert_glassbox(id, 0, &a).unwrap();
        assert_eq!(store(&storage, id, 0, false).unwrap(), b"{\"run\":1}");
        let err = store(&storage, id, 0, true).unwrap_err().to_string();
        assert!(err.contains("no register trace"), "{err}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn first_snapshot_wins_across_chunks() {
        let first = json!({
            "snapshot": { "reg": { "1": "10" } },
            "trace": {
                "0": { "pc": 0, "reg": { "1": "11" } },
                "1": { "pc": 8 }
            }
        });
        let second = json!({
            "snapshot": { "reg": { "1": "999" } },
            "trace": { "2": { "pc": 16 } }
        });
        let chunks = [
            serde_json::from_value(first).unwrap(),
            serde_json::from_value(second).unwrap(),
        ];
        let trace = RegisterTrace::from_chunks(&chunks).unwrap();
        let disasm = BTreeMap::from([(0, "mov64 r1, 1".into()), (8, "add64 r1, 2".into())]);
        let steps = trace.into_steps(&disasm);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[2].pre_regs[1], 11);
        assert_eq!(steps[2].disasm, "");
    }
}
