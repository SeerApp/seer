use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use storage::Storage;

use super::run::{execute, Request};

fn hex32(s: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(s)?;
    anyhow::ensure!(bytes.len() == 32, "need 32 bytes, got {}", bytes.len());
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn load_blobs(storage: &Storage, dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir.join("blobs"))? {
        let entry = entry?;
        let bytes = fs::read(entry.path())?;
        let hash = storage.blob.store(&bytes)?;
        anyhow::ensure!(
            hex::encode(hash) == entry.file_name().to_string_lossy(),
            "blob {} hashed as {}",
            entry.file_name().to_string_lossy(),
            hex::encode(hash)
        );
    }
    Ok(())
}

fn replay(name: &str, dir: &Path) -> Result<(i64, Arc<Storage>, PathBuf)> {
    let tmp = std::env::temp_dir().join(format!("seer-golden-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    let storage = Arc::new(Storage::open_at(&tmp)?);
    load_blobs(&storage, dir)?;
    let meta: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("meta.json"))?)?;
    let tx_hash = hex32(meta["tx_hash"].as_str().context("tx_hash")?)?;
    let state_hash = hex32(meta["state_hash"].as_str().context("state_hash")?)?;
    storage.db.insert_simulation(&tx_hash, &state_hash)?;
    let parent = storage
        .db
        .insert_run(&tx_hash, &state_hash, false, false, None, "golden")?;
    storage.db.finish_run(parent, None)?;
    let child = execute(
        Arc::clone(&storage),
        Request {
            tx: None,
            signature: None,
            from: Some(parent),
            url: None,
            historical: false,
            server_url: crate::captures::CAPTURES_URL.into(),
            sigverify: None,
            blockhash_check: None,
            patch: None,
        },
    )?;
    Ok((child, storage, tmp))
}

fn traces_json(storage: &Storage, run_id: i64) -> Result<serde_json::Value> {
    let row = storage.db.get_run(run_id)?;
    let state = crate::state_accounts::StateAccounts::from_bytes(
        &storage.blob.read(&row.state_blob_hash)?,
    )?;
    let accounts = super::store::account_datas(storage, &state)?;
    let mut out = Vec::new();
    for (ix, hash) in storage.db.list_run_ix(run_id)? {
        let tree = match hash {
            Some(hash) => {
                let decorated =
                    idl::decorate_bytes(&storage.blob.read(&hash)?, storage, &accounts)?;
                serde_json::to_value(&decorated)?
            }
            None => serde_json::Value::Null,
        };
        out.push(serde_json::json!({"ix": ix, "tree": tree}));
    }
    Ok(serde_json::Value::Array(out))
}

/// SHA-256 of `[]`. persist_reg must not store this.
const EMPTY_ELF: [u8; 32] = [
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
    0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
];

fn regs_json(storage: &Storage, run_id: i64) -> Result<serde_json::Value> {
    let mut out = Vec::new();
    for (ix, start, end, self_h, prog, pk) in storage.db.list_reg(run_id)? {
        anyhow::ensure!(
            prog != EMPTY_ELF,
            "ix {ix} program_blob_hash is sha256 of empty bytes"
        );
        let chunk: serde_json::Value = serde_json::from_slice(&storage.blob.read(&self_h)?)?;
        out.push(serde_json::json!({
            "ix": ix,
            "start_step": start,
            "end_step": end,
            "pubkey": hex::encode(pk),
            "chunk": chunk,
        }));
    }
    Ok(serde_json::Value::Array(out))
}

fn glassbox_summary(storage: &Storage, run_id: i64, ix: i64) -> Result<serde_json::Value> {
    let bytes = glassbox::store(storage, run_id, ix, true)?;
    let report: glassbox::Report = serde_json::from_slice(&bytes)?;
    let orders: Vec<u64> = report.path_conditions.iter().map(|pc| pc.order).collect();
    let mut segments = Vec::new();
    let mut start = 0;
    for i in 1..orders.len() {
        if orders[i] < orders[i - 1] {
            segments.push(serde_json::json!({
                "first": orders[start],
                "last": orders[i - 1],
                "n": i - start,
            }));
            start = i;
        }
    }
    if !orders.is_empty() {
        segments.push(serde_json::json!({
            "first": orders[start],
            "last": orders[orders.len() - 1],
            "n": orders.len() - start,
        }));
    }
    Ok(serde_json::json!({
        "ix": report.ix,
        "stepsApplied": report.steps_applied,
        "orderSegments": segments,
    }))
}

fn assert_golden(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens")
        .join(name);
    let (run_id, storage, tmp) = replay(name, &dir).unwrap();
    let row = storage.db.get_run(run_id).unwrap();
    let meta: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("meta.json")).unwrap()).unwrap();
    let want_error = meta.get("error").and_then(|v| {
        if v.is_null() {
            None
        } else {
            v.as_str().map(str::to_string)
        }
    });
    assert_eq!(row.error, want_error, "{name} status {:?}", row.error);
    let got_traces = traces_json(&storage, run_id).unwrap();
    let got_regs = regs_json(&storage, run_id).unwrap();
    if std::env::var("SEER_TEST_SAVE").is_ok() {
        fs::write(
            dir.join("traces.json"),
            serde_json::to_vec(&got_traces).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("regs.json"),
            serde_json::to_vec(&got_regs).unwrap(),
        )
        .unwrap();
    }
    let want_traces: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("traces.json")).unwrap()).unwrap();
    assert_eq!(got_traces, want_traces, "{name} traces");
    let want_regs: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("regs.json")).unwrap()).unwrap();
    assert_eq!(got_regs, want_regs, "{name} regs");
    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn golden_a_simple_v0() {
    assert_golden("a-simple-v0");
}

#[test]
fn golden_b_simple_legacy() {
    assert_golden("b-simple-legacy");
}

#[test]
fn golden_c_complex_legacy_cpi() {
    assert_golden("c-complex-legacy-cpi");
}

/// pAMM ix on golden C. After CPI nesting, recorded orders are one
/// increasing sequence (inner programs sit in the caller hole). SAT
/// hide does not drop rows, so `n` is the capture set.
#[test]
fn golden_c_glassbox_pamm() {
    let name = "c-complex-legacy-cpi";
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens")
        .join(name);
    let (run_id, storage, tmp) = replay(name, &dir).unwrap();
    let got = glassbox_summary(&storage, run_id, 5).unwrap();
    let path = dir.join("glassbox-5.json");
    if std::env::var("SEER_TEST_SAVE").is_ok() {
        fs::write(&path, serde_json::to_vec_pretty(&got).unwrap()).unwrap();
    }
    let want: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(got, want, "{name} glassbox ix 5");
    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn golden_d_token_insufficient_funds() {
    assert_golden("d-token-insufficient-funds");
}
