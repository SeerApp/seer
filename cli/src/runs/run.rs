use std::sync::Arc;

use anyhow::{Context, Result};
use solana_account::Account;
use solana_address::Address;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use storage::Storage;

use super::source::Source;
use crate::state_accounts::{Patch, StateAccounts};

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub tx: Option<VersionedTransaction>,
    pub signature: Option<Signature>,
    pub from: Option<i64>,
    pub url: Option<String>,
    pub historical: bool,
    pub server_url: String,
    pub sigverify: Option<bool>,
    pub blockhash_check: Option<bool>,
    pub patch: Option<Patch>,
}

struct Resolved {
    tx_hash: [u8; 32],
    state_hash: [u8; 32],
    parent_id: Option<i64>,
    sigverify: bool,
    blockhash_check: bool,
    source: Source,
}

pub fn execute(storage: Arc<Storage>, req: Request) -> Result<i64> {
    let mut got = resolve(&storage, &req)?;
    if let Some(patch) = &req.patch {
        let mut state = StateAccounts::from_bytes(&storage.blob.read(&got.state_hash)?)?;
        state.patch(&storage.blob, patch)?;
        got.state_hash = super::store::store_state(&storage, &state)?;
    }
    run_simulation(
        storage,
        &got.tx_hash,
        &got.state_hash,
        req.sigverify.unwrap_or(got.sigverify),
        req.blockhash_check.unwrap_or(got.blockhash_check),
        got.parent_id,
        &got.source.to_string(),
    )
}

fn resolve_historical(storage: &Storage, req: &Request, signature: &Signature) -> Result<Resolved> {
    let sig = signature_bytes(signature);
    if let Some((tx_hash, state_hash)) = storage.db.lookup_sig(&sig, Some("mainnet"))? {
        return Ok(Resolved {
            tx_hash,
            state_hash,
            parent_id: None,
            sigverify: false,
            blockhash_check: false,
            source: Source::Sig {
                signature: *signature,
                historical: true,
            },
        });
    }
    let url = req.url.as_deref().context("--sig requires --url")?;
    let (tx, _) = crate::network::get_transaction(url, signature)?;
    let captured = crate::captures::fetch_capture(&req.server_url, signature)?;
    let tx_hash = super::store::store_transaction(storage, &tx)?;
    let state_hash = super::store::store_captured_accounts(storage, &tx, &captured)?;
    super::store::store_simulation(storage, &tx_hash, &state_hash)?;
    storage
        .db
        .insert_historical(&tx_hash, &state_hash, "mainnet", &sig)?;
    Ok(Resolved {
        tx_hash,
        state_hash,
        parent_id: None,
        sigverify: false,
        blockhash_check: false,
        source: Source::Sig {
            signature: *signature,
            historical: true,
        },
    })
}

fn signature_bytes(signature: &Signature) -> [u8; 64] {
    let bytes: &[u8] = signature.as_ref();
    bytes.try_into().expect("ed25519 signature is 64 bytes")
}

fn resolve(storage: &Storage, req: &Request) -> Result<Resolved> {
    if let Some(signature) = &req.signature {
        if req.historical {
            return resolve_historical(storage, req, signature);
        }
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
            sigverify: false,
            blockhash_check: false,
            source: Source::Sig {
                signature: *signature,
                historical: false,
            },
        });
    }
    if let Some(from) = req.from {
        let parent = storage.db.get_run(from)?;
        let tx_hash = match &req.tx {
            Some(tx) => super::store::store_transaction(storage, tx)?,
            None => parent.transaction_blob_hash,
        };
        let source = if req.tx.is_some() {
            Source::TxFrom(from)
        } else {
            Source::From(from)
        };
        return Ok(Resolved {
            tx_hash,
            state_hash: parent.state_blob_hash,
            parent_id: Some(from),
            sigverify: parent.sigverify,
            blockhash_check: parent.blockhash_check,
            source,
        });
    }
    let tx = req.tx.as_ref().context("need --sig, --tx, or --from")?;
    let url = req.url.as_deref().context("pass --url")?;
    Ok(Resolved {
        tx_hash: super::store::store_transaction(storage, tx)?,
        state_hash: super::store::store_transaction_accounts(storage, tx, url, None)?,
        parent_id: None,
        sigverify: false,
        blockhash_check: false,
        source: Source::Tx,
    })
}

fn run_simulation(
    storage: Arc<Storage>,
    tx_hash: &[u8; 32],
    state_hash: &[u8; 32],
    sigverify: bool,
    blockhash_check: bool,
    parent_id: Option<i64>,
    source: &str,
) -> Result<i64> {
    super::store::store_simulation(&storage, tx_hash, state_hash)?;
    let tx: VersionedTransaction = bincode::deserialize(&storage.blob.read(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&storage.blob.read(state_hash)?)?;
    let mut svm = litesvm::LiteSVM::new()
        .with_sigverify(sigverify)
        .with_blockhash_check(blockhash_check);
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
    let fee_payer = tx
        .message
        .static_account_keys()
        .first()
        .context("transaction has no account keys")?;
    let run_id = storage.db.insert_run(
        tx_hash,
        state_hash,
        sigverify,
        blockhash_check,
        parent_id,
        source,
    )?;
    let drops_before = trace::dropped_hooks();
    trace::init(fee_payer.to_bytes(), Arc::clone(&storage))?;
    trace::set(run_id);
    let error = svm
        .send_transaction(tx)
        .err()
        .map(|failed| failed.err.to_string());
    trace::unset();
    storage.db.finish_run(run_id, error.as_deref())?;
    let n = trace::dropped_hooks().saturating_sub(drops_before);
    if n > 0 {
        trace::seer_warn!("dropped_hooks {n}");
    }
    match error.as_deref() {
        Some(err) => trace::seer_warn!("run {run_id} {err}"),
        None => trace::seer_warn!("run {run_id}"),
    }
    Ok(run_id)
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use super::*;
    use solana_pubkey::Pubkey;

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
            .insert_run(
                &tx_hash,
                &state_hash,
                true,
                false,
                None,
                &Source::Tx.to_string(),
            )
            .unwrap();
        storage.db.finish_run(parent, None).unwrap();
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
        assert!(row.sigverify);
        assert!(!row.blockhash_check);
        assert_eq!(row.parent_id, Some(parent));
        assert_eq!(row.source, Source::From(parent).to_string());
        assert_ne!(row.state_blob_hash, state_hash);
        let patched =
            StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash).unwrap()).unwrap();
        assert_eq!(patched.0[&payer].lamports, 0);
        assert_eq!(
            crate::state_accounts::changes(&state, &patched),
            serde_json::json!([{
                "account": payer.to_string(),
                "lamports": 0,
            }])
        );
        let overridden = execute(
            Arc::clone(&storage),
            Request {
                tx: None,
                signature: None,
                from: Some(parent),
                url: None,
                historical: false,
                server_url: crate::captures::CAPTURES_URL.into(),
                sigverify: Some(false),
                blockhash_check: Some(true),
                patch: None,
            },
        )
        .unwrap();
        let row = storage.db.get_run(overridden).unwrap();
        assert!(!row.sigverify);
        assert!(row.blockhash_check);
        std::fs::remove_dir_all(dir).ok();
    }
}
