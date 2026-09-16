use anyhow::Result;
use solana_transaction::versioned::VersionedTransaction;

use crate::storage::blobs::store_blob;
use crate::storage::state_accounts::StateAccounts;

pub fn hash_state(state: &StateAccounts) -> Result<[u8; 32]> {
    store_blob(&state.to_bytes()?)
}

pub fn hash_transaction(tx: &VersionedTransaction) -> Result<[u8; 32]> {
    store_blob(&bincode::serialize(tx)?)
}
