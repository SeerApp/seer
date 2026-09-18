use anyhow::{Context, Result};
use rusqlite::Connection;
use solana_transaction::versioned::VersionedTransaction;

use crate::state_accounts::StateAccounts;
use storage::{blobs::{read_blob, store_blob}, db::insert_simulation};

pub fn hash_state(state: &StateAccounts) -> Result<[u8; 32]> {
    state.verify()?;
    store_blob(&state.to_bytes()?)
}

pub fn hash_transaction(tx: &VersionedTransaction) -> Result<[u8; 32]> {
    tx.sanitize().context("malformed transaction")?;
    store_blob(&bincode::serialize(tx)?)
}

pub fn hash_simulation(conn: &Connection, tx_hash: &[u8; 32], state_hash: &[u8; 32]) -> Result<()> {
    let tx: VersionedTransaction = bincode::deserialize(&read_blob(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&read_blob(state_hash)?)?;
    super::verify::verify(&tx, &state)?;
    insert_simulation(conn, tx_hash, state_hash)
}
