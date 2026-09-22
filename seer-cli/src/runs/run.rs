use anyhow::{Context, Result};
use solana_account::Account;
use solana_address::Address;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use storage::Storage;

use crate::overrides::Overrides;
use crate::state_accounts::StateAccounts;

pub fn run(
    storage: &Storage,
    tx: &VersionedTransaction,
    url: &str,
    overrides: Overrides,
) -> Result<i64> {
    let state_hash = super::hash::hash_transaction_accounts(storage, tx, url)?;
    let tx_hash = super::hash::hash_transaction(storage, tx)?;
    run_simulation(storage, &tx_hash, &state_hash, overrides)
}

pub fn run_signature(
    storage: &Storage,
    signature: &Signature,
    url: &str,
    overrides: Overrides,
) -> Result<i64> {
    let tx = crate::network::get_transaction(url, signature)?;
    run(storage, &tx, url, overrides)
}

pub fn run_simulation(
    storage: &Storage,
    tx_hash: &[u8; 32],
    state_hash: &[u8; 32],
    overrides: Overrides,
) -> Result<i64> {
    super::hash::hash_simulation(storage, tx_hash, state_hash)?;
    let tx: VersionedTransaction = bincode::deserialize(&storage.blob.read(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&storage.blob.read(state_hash)?)?;
    let mut svm = overrides.apply();
    // LiteSVM compiles upgradeable programs in set_account and looks up programdata then.
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
    overrides.airdrop(&mut svm)?;
    let fee_payer = tx
        .message
        .static_account_keys()
        .first()
        .context("transaction has no account keys")?;
    let run_id = storage
        .db
        .insert_run(tx_hash, state_hash, &serde_json::to_string(&overrides)?)?;
    seer_core::init(fee_payer.to_bytes(), None, storage)?;
    seer_core::set(run_id);
    let error = svm
        .send_transaction(tx)
        .err()
        .map(|failed| failed.err.to_string());
    seer_core::unset();
    storage.db.finish_run(run_id, error.as_deref())?;
    Ok(run_id)
}
