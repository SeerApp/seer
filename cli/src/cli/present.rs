use anyhow::{Context, Result};
use solana_pubkey::Pubkey;
use storage::{RunRow, Storage};

use super::format::{ls_select, ls_tree_json, run_json, slice_json_array, state_catalog, LsStatus};
use crate::state_accounts::StateAccounts;

pub(super) fn emit_run(storage: &Storage, id: i64, short: bool) -> Result<()> {
    let row = storage.db.get_run(id)?;
    emit(
        run_json(&row),
        &[
            format!("seer show {id}"),
            format!("seer run --from {id} --account <PUBKEY> --lamports 0"),
            "seer ls".into(),
        ],
        short,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ls(
    storage: &Storage,
    tree: bool,
    head: usize,
    skip: usize,
    from: Option<i64>,
    status: Option<LsStatus>,
    short: bool,
) -> Result<()> {
    let runs = storage.db.list_runs()?;
    if let Some(id) = from {
        let _ = storage.db.get_run(id)?;
    }
    let selected = ls_select(&runs, from, tree, status);
    let values = if tree {
        ls_tree_json(&selected)
    } else {
        let mut flat: Vec<serde_json::Value> = selected.iter().map(run_json).collect();
        flat.reverse();
        flat
    };
    let page = slice_json_array(values, skip, head);
    let first_id = page
        .first()
        .and_then(|v| v.get("id"))
        .and_then(|id| id.as_i64());
    let footer = ls_footer(from, status, tree, skip, head, page.len(), first_id);
    emit(serde_json::Value::Array(page), &footer, short)
}

fn ls_footer(
    from: Option<i64>,
    status: Option<LsStatus>,
    tree: bool,
    skip: usize,
    head: usize,
    page_len: usize,
    first_id: Option<i64>,
) -> Vec<String> {
    let mut flags = String::from("seer ls");
    if let Some(id) = from {
        flags.push_str(&format!(" --from {id}"));
    }
    if let Some(st) = status {
        flags.push_str(&format!(
            " --status {}",
            match st {
                LsStatus::Ok => "ok",
                LsStatus::Error => "error",
            }
        ));
    }
    if tree {
        flags.push_str(" --tree");
    }
    let mut out = Vec::new();
    if head != 0 && page_len == head {
        let next_skip = skip.saturating_add(head);
        out.push(format!("{flags} --skip {next_skip} --head {head}"));
    }
    match first_id {
        Some(id) => out.push(format!("seer show {id}")),
        None if out.is_empty() => out.push("seer run --sig <SIGNATURE> --url <RPC>".into()),
        None => {}
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    storage: &Storage,
    id: i64,
    tx: bool,
    state: bool,
    account: &[Pubkey],
    data: &[Pubkey],
    trace: bool,
    program: Option<&[Pubkey]>,
    short: bool,
) -> Result<()> {
    let row = storage.db.get_run(id)?;
    if !tx && !state && account.is_empty() && data.is_empty() && !trace && program.is_none() {
        return emit(
            run_json(&row),
            &[
                format!("seer show {id} --tx"),
                format!("seer show {id} --state"),
                format!("seer show {id} --account <PUBKEY>"),
                format!("seer show {id} --data <PUBKEY>"),
                format!("seer show {id} --trace"),
                format!("seer show {id} --program"),
                format!("seer run --from {id} --account <PUBKEY> --lamports 0"),
            ],
            short,
        );
    }
    let mut value = serde_json::Map::new();
    if tx {
        value.insert(
            "tx".into(),
            crate::print::transaction(&storage.blob.read(&row.transaction_blob_hash)?)?,
        );
    }
    if state {
        let accounts = StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash)?)?;
        value.insert("state".into(), state_catalog(&accounts));
    }
    if !account.is_empty() {
        let mut map = serde_json::Map::new();
        for pk in account {
            map.insert(pk.to_string(), account_meta(storage, &row, pk)?);
        }
        value.insert("accounts".into(), serde_json::Value::Object(map));
    }
    if !data.is_empty() {
        let mut map = serde_json::Map::new();
        for pk in data {
            map.insert(
                pk.to_string(),
                serde_json::Value::String(account_hex(storage, &row, pk)?),
            );
        }
        value.insert("data".into(), serde_json::Value::Object(map));
    }
    if trace {
        let state = StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash)?)?;
        let accounts = crate::runs::store::account_datas(storage, &state)?;
        let mut traces = Vec::new();
        for (ix, hash) in storage.db.list_run_ix(id)? {
            let Some(hash) = hash else {
                continue;
            };
            let tree = idl::decorate_bytes(&storage.blob.read(&hash)?, storage, &accounts)?;
            traces.push(serde_json::json!({
                "ix": ix,
                "tree": serde_json::to_value(&tree)?,
            }));
        }
        value.insert("trace".into(), serde_json::Value::Array(traces));
    }
    if let Some(subset) = program {
        value.insert(
            "program".into(),
            super::program::show_programs(storage, &row, subset)?,
        );
    }
    let mut footer = vec![
        format!("seer show {id}"),
        format!("seer run --from {id} --account <PUBKEY> --lamports 0"),
    ];
    if let Ok(ixs) = storage.db.list_run_ix(id) {
        if let Some((ix, _)) = ixs.first() {
            footer.insert(1, format!("seer glassbox {id} --ix {ix}"));
        }
    }
    if program.is_some() {
        footer.insert(1, "seer program <HASH> --disasm".into());
    }
    emit(serde_json::Value::Object(value), &footer, short)
}

pub(super) fn diff(storage: &Storage, a: i64, b: i64, short: bool) -> Result<()> {
    let a = storage.db.get_run(a)?;
    let b = storage.db.get_run(b)?;
    let value = serde_json::json!({
        "a": run_json(&a),
        "b": run_json(&b),
        "tx": if a.transaction_blob_hash == b.transaction_blob_hash { "same" } else { "different" },
        "state": if a.state_blob_hash == b.state_blob_hash { "same" } else { "different" },
    });
    emit(
        value,
        &[format!("seer show {}", a.id), format!("seer show {}", b.id)],
        short,
    )
}

fn account_meta(storage: &Storage, row: &RunRow, pk: &Pubkey) -> Result<serde_json::Value> {
    let (acct, data) = load_account(storage, row, pk)?;
    Ok(serde_json::json!({
        "lamports": acct.lamports.to_string(),
        "owner": acct.owner.to_string(),
        "executable": acct.executable,
        "data_len": data.len(),
    }))
}

fn account_hex(storage: &Storage, row: &RunRow, pk: &Pubkey) -> Result<String> {
    let (_, data) = load_account(storage, row, pk)?;
    Ok(hex::encode(data))
}

fn load_account(
    storage: &Storage,
    row: &RunRow,
    pk: &Pubkey,
) -> Result<(crate::state_accounts::StateAccount, Vec<u8>)> {
    let state = StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash)?)?;
    let acct = state
        .0
        .get(pk)
        .cloned()
        .with_context(|| format!("{pk} not in run {}", row.id))?;
    let data = storage.blob.read(&acct.data)?;
    Ok((acct, data))
}

pub(in crate::cli) fn emit(value: serde_json::Value, footer: &[String], short: bool) -> Result<()> {
    if short {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
        for line in footer {
            println!("next: {line}");
        }
    }
    Ok(())
}
