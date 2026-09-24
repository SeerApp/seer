use std::fs;
use std::path::{Path, PathBuf};

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

fn replay(name: &str, dir: &Path) -> Result<(i64, Storage, PathBuf)> {
    let tmp = std::env::temp_dir().join(format!("seer-golden-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    let storage = Storage::open_at(&tmp)?;
    load_blobs(&storage, dir)?;
    let meta: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("meta.json"))?)?;
    let tx_hash = hex32(meta["tx_hash"].as_str().context("tx_hash")?)?;
    let state_hash = hex32(meta["state_hash"].as_str().context("state_hash")?)?;
    let environment = meta["environment"].as_str().unwrap_or("{}");
    storage.db.insert_simulation(&tx_hash, &state_hash)?;
    let parent = storage
        .db
        .insert_run(&tx_hash, &state_hash, environment, None, "[]", "golden")?;
    storage.db.finish_run(parent, None)?;
    let child = execute(
        &storage,
        Request {
            tx: None,
            signature: None,
            from: Some(parent),
            url: None,
            environment: None,
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

fn regs_json(storage: &Storage, run_id: i64) -> Result<serde_json::Value> {
    let mut out = Vec::new();
    for (ix, start, end, self_h, pk) in storage.db.list_reg(run_id)? {
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

fn assert_golden(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens")
        .join(name);
    let (run_id, storage, tmp) = replay(name, &dir).unwrap();
    let row = storage.db.get_run(run_id).unwrap();
    assert_eq!(row.error, None, "{name} status {:?}", row.error);
    let want_traces: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("traces.json")).unwrap()).unwrap();
    let got_traces = traces_json(&storage, run_id).unwrap();
    assert_eq!(got_traces, want_traces, "{name} traces");
    let want_regs: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("regs.json")).unwrap()).unwrap();
    let got_regs = regs_json(&storage, run_id).unwrap();
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
