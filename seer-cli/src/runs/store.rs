use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use solana_account::Account;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;
use storage::Storage;

use crate::network::get_multiple_accounts;
use crate::state_accounts::{StateAccount, StateAccounts};

pub fn store_state(storage: &Storage, state: &StateAccounts) -> Result<[u8; 32]> {
    state.verify(&storage.blob)?;
    storage.blob.store(&state.to_bytes()?)
}

pub fn store_transaction(storage: &Storage, tx: &VersionedTransaction) -> Result<[u8; 32]> {
    tx.sanitize().context("malformed transaction")?;
    storage.blob.store(&bincode::serialize(tx)?)
}

pub fn store_accounts(
    storage: &Storage,
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
    for (key, account) in fetch(url, &missing)? {
        raw.insert(key, account);
    }
    let mut extra: Vec<Pubkey> = raw
        .values()
        .filter_map(programdata)
        .filter(|programdata| !raw.contains_key(programdata))
        .collect();
    extra.sort();
    extra.dedup();
    for (key, account) in fetch(url, &extra)? {
        raw.insert(key, account);
    }
    raw.into_iter()
        .map(|(key, account)| {
            Ok((
                key,
                StateAccount {
                    lamports: account.lamports,
                    data: storage.blob.store(&account.data)?,
                    owner: Pubkey::from(account.owner.to_bytes()),
                    executable: account.executable,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()
        .map(StateAccounts)
}

pub fn store_transaction_accounts(
    storage: &Storage,
    tx: &VersionedTransaction,
    url: &str,
) -> Result<[u8; 32]> {
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
        for (key, account) in fetch(url, &table_keys)? {
            if !solana_sdk_ids::address_lookup_table::check_id(&account.owner) {
                bail!("{key} is not an address lookup table");
            }
            cache.insert(key, account);
        }
        for lookup in lookups {
            let key = Pubkey::from(lookup.account_key.to_bytes());
            let account = cache
                .get(&key)
                .with_context(|| format!("missing address lookup table {key}"))?;
            let table = AddressLookupTable::deserialize(&account.data)
                .map_err(|e| anyhow::anyhow!("invalid address lookup table {key}: {e}"))?;
            for index in lookup
                .writable_indexes
                .iter()
                .chain(&lookup.readonly_indexes)
            {
                let addr = table
                    .addresses
                    .get(usize::from(*index))
                    .with_context(|| format!("lookup index {index} out of range in {key}"))?;
                keys.push(Pubkey::from(addr.to_bytes()));
            }
            keys.push(key);
        }
    }
    keys.sort();
    keys.dedup();
    let already = (!cache.is_empty()).then_some(&cache);
    store_state(storage, &store_accounts(storage, &keys, url, already)?)
}

pub fn store_simulation(
    storage: &Storage,
    tx_hash: &[u8; 32],
    state_hash: &[u8; 32],
) -> Result<()> {
    let tx: VersionedTransaction = bincode::deserialize(&storage.blob.read(tx_hash)?)?;
    let state = StateAccounts::from_bytes(&storage.blob.read(state_hash)?)?;
    tx.sanitize().context("malformed transaction")?;
    state.verify(&storage.blob)?;
    storage.db.insert_simulation(tx_hash, state_hash)
}

fn fetch(url: &str, keys: &[Pubkey]) -> Result<Vec<(Pubkey, Account)>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    get_multiple_accounts(url, keys)?
        .into_iter()
        .zip(keys)
        .map(|(account, key)| {
            let Some(account) = account else {
                bail!("missing account {key}");
            };
            Ok((*key, account))
        })
        .collect()
}

fn programdata(account: &Account) -> Option<Pubkey> {
    if !solana_sdk_ids::bpf_loader_upgradeable::check_id(&account.owner) {
        return None;
    }
    match bincode::deserialize(&account.data) {
        Ok(UpgradeableLoaderState::Program {
            programdata_address,
        }) => Some(Pubkey::from(programdata_address.to_bytes())),
        _ => None,
    }
}
