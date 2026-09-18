use anyhow::{Context, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use solana_transaction::versioned::VersionedTransaction;

use crate::storage::blobs::{read_blob, store_blob};
use crate::storage::state_accounts::StateAccounts;

pub fn hash_state(state: &StateAccounts) -> Result<[u8; 32]> {
    state.verify()?;
    store_blob(&state.to_bytes()?)
}

pub fn hash_transaction(tx: &VersionedTransaction) -> Result<[u8; 32]> {
    tx.sanitize().context("malformed transaction")?;
    store_blob(&bincode::serialize(tx)?)
}

pub fn hash_simulation(
    conn: &Connection,
    tx_hash: &[u8; 32],
    state_hash: &[u8; 32],
) -> Result<[u8; 32]> {
    let tx: VersionedTransaction = bincode::deserialize(&read_blob(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&read_blob(state_hash)?)?;
    super::verify::verify(&tx, &state)?;
    let hash: [u8; 32] = Sha256::new()
        .chain_update(tx_hash)
        .chain_update(state_hash)
        .finalize()
        .into();
    crate::storage::db::insert_simulation(conn, &hash, tx_hash, state_hash)?;
    Ok(hash)
}
