use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use anyhow::{bail, Context, Result};
use storage::{ProgramChunk, Storage};

use crate::coverage::Skip;
use crate::reg::{RegisterTrace, RegisterTraceChunk};
use crate::step::Step;
use crate::vm::Vm;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterStep {
    pub order: u64,
    pub pc: u64,
    pub pubkey: [u8; 32],
    pub pre_regs: [u64; 11],
    pub post_regs: [u64; 11],
}

pub fn register_timeline(storage: &Storage, run_id: i64, ix: i64) -> Result<Vec<RegisterStep>> {
    let _ = storage.db.get_run(run_id)?;
    let rows: Vec<_> = storage
        .db
        .list_reg(run_id)?
        .into_iter()
        .filter(|(row_ix, _, _, _, _, _)| *row_ix == ix)
        .collect();
    if rows.is_empty() {
        bail!("no register trace for run {run_id} ix {ix}");
    }
    let mut out = Vec::new();
    for (_ix, _start, _end, self_h, _elf, pk) in rows {
        let bytes = storage.blob.read(&self_h)?;
        let chunk: RegisterTraceChunk =
            serde_json::from_slice(&bytes).context("parse register chunk")?;
        for step in RegisterTrace::from_chunks(&[chunk])?.into_steps(&BTreeMap::new()) {
            out.push(RegisterStep {
                order: step.order,
                pc: step.pc,
                pubkey: pk,
                pre_regs: step.pre_regs,
                post_regs: step.post_regs,
            });
        }
    }
    out.sort_by_key(|s| s.order);
    Ok(out)
}

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

    let mut skips = Vec::new();
    let mut tagged = Vec::new();
    for (_ix, _start, _end, self_h, elf, pk) in rows {
        let program_id = bs58::encode(pk).into_string();
        match load_chunk(storage, &elf, &self_h, &mut skips) {
            Ok(steps) => {
                tagged.extend(
                    steps
                        .into_iter()
                        .map(|step| TaggedStep { pubkey: pk, step }),
                );
            }
            Err(e) => {
                let ix_u8 = u8::try_from(ix).unwrap_or(u8::MAX);
                skips.push(Skip::program_load(ix_u8, &program_id, format!("{e:#}")));
            }
        }
    }
    tagged.sort_by_key(|t| t.step.order);
    let stretches = group_clock(tagged);

    let mut vm = Vm::new();
    if logger::level_enabled(logger::SeerLoggerLevel::Debug) {
        vm = vm.debug(true);
    }
    let mut steps_applied = 0;
    let mut current_pk: Option<[u8; 32]> = None;
    for stretch in &stretches {
        if let Some(cur) = current_pk {
            if cur != stretch.pubkey {
                if vm.parked_pubkey() == Some(stretch.pubkey) {
                    vm.return_cpi();
                } else {
                    vm.enter_cpi(cur, &stretch.steps);
                }
            }
        }
        steps_applied += stretch.steps.len();
        let _ = vm.run_stretch(stretch.steps.iter());
        current_pk = Some(stretch.pubkey);
    }

    let missing_disasm: usize = skips
        .iter()
        .filter(|s| s.reason == crate::coverage::SkipReason::EmptyDisasm)
        .count();
    let analysis = vm.into_analysis();
    let mut out = Cursor::new(Vec::new());
    analysis.write(&mut out, run_id, ix, steps_applied, missing_disasm)?;
    Ok(out.into_inner())
}

struct TaggedStep {
    pubkey: [u8; 32],
    step: Step,
}

struct Stretch {
    pubkey: [u8; 32],
    steps: Vec<Step>,
}

/// Caller chunks often keep a hole (CPI orders live in other programs’ files).
/// Play by `step.order`, not by `reg.start_step` row runs.
fn group_clock(tagged: Vec<TaggedStep>) -> Vec<Stretch> {
    let mut out: Vec<Stretch> = Vec::new();
    for t in tagged {
        if let Some(last) = out.last_mut() {
            if last.pubkey == t.pubkey {
                last.steps.push(t.step);
                continue;
            }
        }
        out.push(Stretch {
            pubkey: t.pubkey,
            steps: vec![t.step],
        });
    }
    out
}

fn load_chunk(
    storage: &Storage,
    elf: &[u8; 32],
    blob: &[u8; 32],
    skips: &mut Vec<Skip>,
) -> Result<Vec<Step>> {
    let bytes = storage.blob.read(blob)?;
    let chunk: RegisterTraceChunk =
        serde_json::from_slice(&bytes).context("parse register chunk")?;
    let trace = RegisterTrace::from_chunks(&[chunk])?;
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

fn disasm_for_pcs(storage: &Storage, elf: &[u8; 32], pcs: &[u64]) -> Result<BTreeMap<u64, String>> {
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

    fn put_reg(
        storage: &storage::Storage,
        run_id: i64,
        ix: i64,
        pk: [u8; 32],
        chunk: serde_json::Value,
    ) {
        let bytes = serde_json::to_vec(&chunk).unwrap();
        let self_h = storage.blob.store(&bytes).unwrap();
        let elf = storage.blob.store(&pk).unwrap();
        storage.db.insert_program(&elf).unwrap();
        let orders: Vec<i64> = chunk["trace"]
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.parse().unwrap())
            .collect();
        let start = *orders.iter().min().unwrap();
        let end = *orders.iter().max().unwrap();
        storage
            .db
            .insert_reg(run_id, ix, start, end, &self_h, &elf, &pk)
            .unwrap();
    }

    #[test]
    fn register_timeline_is_clock_order_without_the_vm() {
        let (storage, root) = tmp_storage();
        let id = new_run(&storage);
        let a = [1u8; 32];
        let b = [2u8; 32];
        put_reg(
            &storage,
            id,
            0,
            a,
            json!({
                "snapshot": { "reg": { "1": "10" } },
                "trace": {
                    "10": { "pc": 100, "reg": { "1": "11" } },
                    "40": { "pc": 400 }
                }
            }),
        );
        put_reg(
            &storage,
            id,
            0,
            b,
            json!({
                "snapshot": { "reg": { "1": "50" } },
                "trace": { "20": { "pc": 200, "reg": { "0": "7" } } }
            }),
        );
        let steps = register_timeline(&storage, id, 0).unwrap();
        assert_eq!(
            steps.iter().map(|s| s.order).collect::<Vec<_>>(),
            vec![10, 20, 40]
        );
        assert_eq!(steps[0].pubkey, a);
        assert_eq!(steps[0].pc, 100);
        assert_eq!(steps[0].pre_regs[1], 10);
        assert_eq!(steps[0].post_regs[1], 11);
        assert_eq!(steps[1].pubkey, b);
        assert_eq!(steps[1].pre_regs[1], 50);
        assert_eq!(steps[1].post_regs[0], 7);
        assert_eq!(steps[2].pre_regs[1], 11);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn stretches_follow_clock_through_a_hole_in_one_chunk() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let step = |order, pk| TaggedStep {
            pubkey: pk,
            step: Step {
                order,
                pc: 0,
                next_pc: None,
                disasm: "mov64 r0, 1".into(),
                pre_regs: [0; 11],
                post_regs: [0; 11],
            },
        };
        // Caller chunk lists 10 then 40; callee owns 20–30. Row start_step
        // would play 10,40 before 20. Clock order nests the callee.
        let mut tagged = vec![step(10, a), step(40, a), step(20, b), step(30, b)];
        tagged.sort_by_key(|t| t.step.order);
        let s = group_clock(tagged);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].pubkey, a);
        assert_eq!(s[0].steps[0].order, 10);
        assert_eq!(s[1].pubkey, b);
        assert_eq!(s[1].steps[0].order, 20);
        assert_eq!(s[2].pubkey, a);
        assert_eq!(s[2].steps[0].order, 40);
    }
}
