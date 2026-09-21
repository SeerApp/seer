use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use solana_account::Account;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

use super::helpers::{GetProgramdata, MergeFetched};
use crate::network::get_multiple_accounts;
use crate::state_accounts::{StateAccount, StateAccounts};
use storage::{blobs::{read_blob, store_blob}, db::insert_simulation};

#[allow(dead_code)]
pub fn hash_data(data: &[u8]) -> Result<[u8; 32]> {
    store_blob(data)
}

pub fn hash_state(state: &StateAccounts) -> Result<[u8; 32]> {
    state.verify()?;
    store_blob(&state.to_bytes()?)
}

pub fn hash_transaction(tx: &VersionedTransaction) -> Result<[u8; 32]> {
    tx.sanitize().context("malformed transaction")?;
    store_blob(&bincode::serialize(tx)?)
}

pub fn hash_accounts(
    keys: &[Pubkey],
    url: &str,
    already: Option<&BTreeMap<Pubkey, Account>>,
) -> Result<StateAccounts> {
    let mut raw = BTreeMap::new();
    let mut missing = Vec::new();
    for key in keys {
        if raw.contains_key(key) {
            continue;
        }
        match already.and_then(|cache| cache.get(key)) {
            Some(account) => {
                raw.insert(*key, account.clone());
            }
            None => missing.push(*key),
        }
    }
    raw.merge_fetched(&missing, url)?;
    let mut extra: Vec<Pubkey> = raw
        .values()
        .filter_map(GetProgramdata::get_programdata)
        .filter(|programdata| !raw.contains_key(programdata))
        .collect();
    extra.sort();
    extra.dedup();
    raw.merge_fetched(&extra, url)?;
    raw.into_iter()
        .map(|(key, account)| {
            Ok((
                key,
                StateAccount {
                    lamports: account.lamports,
                    data: store_blob(&account.data)?,
                    owner: Pubkey::from(account.owner.to_bytes()),
                    executable: account.executable,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()
        .map(StateAccounts)
}

pub fn hash_transaction_accounts(tx: &VersionedTransaction, url: &str) -> Result<[u8; 32]> {
    tx.sanitize().context("malformed transaction")?;
    let mut keys: Vec<Pubkey> = tx
        .message
        .static_account_keys()
        .iter()
        .map(|key| Pubkey::from(key.to_bytes()))
        .collect();
    let lookups = tx.message.address_table_lookups().unwrap_or(&[]);
    let mut cache = BTreeMap::new();
    if !lookups.is_empty() {
        let mut table_keys: Vec<Pubkey> = lookups
            .iter()
            .map(|lookup| Pubkey::from(lookup.account_key.to_bytes()))
            .collect();
        table_keys.sort();
        table_keys.dedup();
        let fetched = get_multiple_accounts(url, &table_keys)?;
        for (key, account) in table_keys.iter().zip(fetched) {
            let Some(account) = account else {
                bail!("missing address lookup table {key}");
            };
            if !solana_sdk_ids::address_lookup_table::check_id(&account.owner) {
                bail!("{key} is not an address lookup table");
            }
            cache.insert(*key, account);
        }
        for lookup in lookups {
            let key = Pubkey::from(lookup.account_key.to_bytes());
            let account = cache
                .get(&key)
                .with_context(|| format!("missing address lookup table {key}"))?;
            let table = AddressLookupTable::deserialize(&account.data)
                .map_err(|e| anyhow::anyhow!("invalid address lookup table {key}: {e}"))?;
            for index in lookup.writable_indexes.iter().chain(&lookup.readonly_indexes) {
                let addr = table.addresses.get(usize::from(*index)).with_context(|| {
                    format!("lookup index {index} out of range in {key}")
                })?;
                keys.push(Pubkey::from(addr.to_bytes()));
            }
            keys.push(key);
        }
    }
    keys.sort();
    keys.dedup();
    let already = (!cache.is_empty()).then_some(&cache);
    hash_state(&hash_accounts(&keys, url, already)?)
}

pub fn hash_simulation(conn: &Connection, tx_hash: &[u8; 32], state_hash: &[u8; 32]) -> Result<()> {
    let tx: VersionedTransaction = bincode::deserialize(&read_blob(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&read_blob(state_hash)?)?;
    tx.sanitize().context("malformed transaction")?;
    state.verify()?;
    insert_simulation(conn, tx_hash, state_hash)
}
