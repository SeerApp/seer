use std::sync::Arc;

use anyhow::{Context, Result};
use solana_account::Account;
use solana_address::Address;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use storage::Storage;

use crate::environment::Environment;
use crate::state_accounts::{Patch, StateAccounts};

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub tx: Option<VersionedTransaction>,
    pub signature: Option<Signature>,
    pub from: Option<i64>,
    pub url: Option<String>,
    pub environment: Option<Environment>,
    pub patch: Option<Patch>,
}

struct Resolved {
    tx_hash: [u8; 32],
    state_hash: [u8; 32],
    parent_id: Option<i64>,
    environment: String,
    source: String,
}

pub fn execute(storage: Arc<Storage>, req: Request) -> Result<i64> {
    let mut got = resolve(&storage, &req)?;
    if let Some(patch) = &req.patch {
        let mut state = StateAccounts::from_bytes(&storage.blob.read(&got.state_hash)?)?;
        state.patch(&storage.blob, patch)?;
        got.state_hash = super::store::store_state(&storage, &state)?;
    }
    let environment = match &req.environment {
        Some(env) => merge_environment(&got.environment, env),
        None => got.environment,
    };
    let patches = match &req.patch {
        Some(patch) => patch.json()?,
        None => "[]".into(),
    };
    run_simulation(
        storage,
        &got.tx_hash,
        &got.state_hash,
        &environment,
        got.parent_id,
        &patches,
        &got.source,
    )
}

fn resolve(storage: &Storage, req: &Request) -> Result<Resolved> {
    if let Some(signature) = &req.signature {
        let url = req.url.as_deref().context("--sig requires --url")?;
        let (tx, loaded) = crate::network::get_transaction(url, signature)?;
        return Ok(Resolved {
            tx_hash: super::store::store_transaction(storage, &tx)?,
            state_hash: super::store::store_transaction_accounts(
                storage,
                &tx,
                url,
                loaded.as_ref(),
            )?,
            parent_id: None,
            environment: "{}".into(),
            source: format!("sig:{signature}"),
        });
    }
    if let Some(from) = req.from {
        let parent = storage.db.get_run(from)?;
        let tx_hash = match &req.tx {
            Some(tx) => super::store::store_transaction(storage, tx)?,
            None => parent.transaction_blob_hash,
        };
        let source = if req.tx.is_some() {
            format!("tx,from:{from}")
        } else {
            format!("from:{from}")
        };
        return Ok(Resolved {
            tx_hash,
            state_hash: parent.state_blob_hash,
            parent_id: Some(from),
            environment: parent.environment,
            source,
        });
    }
    let tx = req.tx.as_ref().context("need --sig, --tx, or --from")?;
    let url = req.url.as_deref().context("pass --url")?;
    Ok(Resolved {
        tx_hash: super::store::store_transaction(storage, tx)?,
        state_hash: super::store::store_transaction_accounts(storage, tx, url, None)?,
        parent_id: None,
        environment: "{}".into(),
        source: "tx".into(),
    })
}

fn run_simulation(
    storage: Arc<Storage>,
    tx_hash: &[u8; 32],
    state_hash: &[u8; 32],
    environment: &str,
    parent_id: Option<i64>,
    patches: &str,
    source: &str,
) -> Result<i64> {
    super::store::store_simulation(&storage, tx_hash, state_hash)?;
    let tx: VersionedTransaction = bincode::deserialize(&storage.blob.read(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&storage.blob.read(state_hash)?)?;
    let parsed: Environment = serde_json::from_str(environment)?;
    let mut svm = parsed.apply();
    for executable in [false, true] {
        for (pubkey, account) in &state.0 {
            if account.executable != executable {
                continue;
            }
            svm.set_account(
                Address::from(pubkey.to_bytes()),
                Account {
                    lamports: account.lamports,
                    data: state
                        .get_data(&storage.blob, pubkey)
                        .map_err(anyhow::Error::from)?,
                    owner: account.owner.to_bytes().into(),
                    executable: account.executable,
                    rent_epoch: u64::MAX,
                },
            )?;
        }
    }
    parsed.airdrop(&mut svm)?;
    let fee_payer = tx
        .message
        .static_account_keys()
        .first()
        .context("transaction has no account keys")?;
    let run_id =
        storage
            .db
            .insert_run(tx_hash, state_hash, environment, parent_id, patches, source)?;
    trace::init(fee_payer.to_bytes(), Arc::clone(&storage))?;
    trace::set(run_id);
    let error = svm
        .send_transaction(tx)
        .err()
        .map(|failed| failed.err.to_string());
    trace::unset();
    storage.db.finish_run(run_id, error.as_deref())?;
    Ok(run_id)
}

fn merge_environment(parent: &str, env: &Environment) -> String {
    let mut base: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(parent).expect("stored environment is a JSON object");
    let serde_json::Value::Object(over) =
        serde_json::to_value(env).expect("Environment serializes")
    else {
        unreachable!("Environment serializes as a JSON object");
    };
    base.extend(over);
    serde_json::Value::Object(base).to_string()
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use super::*;
    use solana_pubkey::Pubkey;

    #[test]
    fn merge_environment_keeps_parent_keys() {
        let parent = r#"{"slot":1,"sigverify":true}"#;
        let env = Environment {
            epoch: Some(2),
            ..Environment::default()
        };
        let got: serde_json::Value =
            serde_json::from_str(&merge_environment(parent, &env)).unwrap();
        assert_eq!(got["slot"], 1);
        assert_eq!(got["epoch"], 2);
        assert_eq!(got["sigverify"], true);
    }

    #[test]
    fn execute_from_patches_parent() {
        let dir = std::env::temp_dir().join(format!(
            "seer-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage = Arc::new(storage::Storage::open_at(&dir).unwrap());
        let payer = Pubkey::from([1u8; 32]);
        let tx = VersionedTransaction::from(solana_transaction::Transaction::new_unsigned(
            solana_message::Message::new(&[], Some(&payer)),
        ));
        let tx_hash = crate::runs::store::store_transaction(&storage, &tx).unwrap();
        let data = storage.blob.store(&[]).unwrap();
        let state = StateAccounts(std::collections::BTreeMap::from([(
            payer,
            crate::state_accounts::StateAccount {
                lamports: 1000,
                data,
                owner: Pubkey::default(),
                executable: false,
            },
        )]));
        let state_hash = crate::runs::store::store_state(&storage, &state).unwrap();
        crate::runs::store::store_simulation(&storage, &tx_hash, &state_hash).unwrap();
        let parent = storage
            .db
            .insert_run(&tx_hash, &state_hash, "{}", None, "[]", "tx")
            .unwrap();
        storage.db.finish_run(parent, None).unwrap();
        let child = execute(
            Arc::clone(&storage),
            Request {
                tx: None,
                signature: None,
                from: Some(parent),
                url: None,
                environment: None,
                patch: Some(Patch {
                    account: payer,
                    lamports: Some(0),
                    owner: None,
                    data: None,
                    executable: None,
                }),
            },
        )
        .unwrap();
        let row = storage.db.get_run(child).unwrap();
        assert_eq!(row.parent_id, Some(parent));
        assert_eq!(row.source, format!("from:{parent}"));
        assert!(row.patches.contains("lamports"));
        assert_ne!(row.state_blob_hash, state_hash);
        let patched =
            StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash).unwrap()).unwrap();
        assert_eq!(patched.0[&payer].lamports, 0);
        std::fs::remove_dir_all(dir).ok();
    }
}
