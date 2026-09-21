use anyhow::{Context, Result};
use rusqlite::Connection;
use solana_account::Account;
use solana_address::Address;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;

use crate::overrides::Overrides;
use crate::state_accounts::StateAccounts;
use storage::blobs::read_blob;

pub fn run(
    conn: &Connection,
    tx: &VersionedTransaction,
    url: &str,
    overrides: Overrides,
) -> Result<i64> {
    let state_hash = super::hash::hash_transaction_accounts(tx, url)?;
    let tx_hash = super::hash::hash_transaction(tx)?;
    run_simulation(conn, &tx_hash, &state_hash, overrides)
}

pub fn run_signature(
    conn: &Connection,
    signature: &Signature,
    url: &str,
    overrides: Overrides,
) -> Result<i64> {
    let tx = crate::network::get_transaction(url, signature)?;
    run(conn, &tx, url, overrides)
}

pub fn run_simulation(
    conn: &Connection,
    tx_hash: &[u8; 32],
    state_hash: &[u8; 32],
    overrides: Overrides,
) -> Result<i64> {
    super::hash::hash_simulation(conn, tx_hash, state_hash)?;
    let tx: VersionedTransaction = bincode::deserialize(&read_blob(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&read_blob(state_hash)?)?;
    let mut svm = overrides.apply();
    for (pubkey, account) in &state.0 {
        svm.set_account(
            Address::from(pubkey.to_bytes()),
            Account {
                lamports: account.lamports,
                data: state.get_data(pubkey).map_err(anyhow::Error::from)?,
                owner: account.owner.to_bytes().into(),
                executable: account.executable,
                rent_epoch: u64::MAX,
            },
        )?;
    }
    overrides.airdrop(&mut svm)?;
    let fee_payer = tx
        .message
        .static_account_keys()
        .first()
        .context("transaction has no account keys")?;
    let run_id = storage::db::insert_run(
        conn,
        tx_hash,
        state_hash,
        &serde_json::to_string(&overrides)?,
    )?;
    seer_core::init(fee_payer.to_bytes(), None, conn)?;
    seer_core::set(run_id);
    let error = svm
        .send_transaction(tx)
        .err()
        .map(|failed| failed.err.to_string());
    seer_core::unset();
    storage::db::finish_run(conn, run_id, error.as_deref())?;
    Ok(run_id)
}
