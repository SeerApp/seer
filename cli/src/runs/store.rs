use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use solana_account::Account;
use solana_address_lookup_table_interface::state::{
    AddressLookupTable, LookupTableMeta, LOOKUP_TABLE_META_SIZE,
};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_message::v0::{LoadedAddresses, MessageAddressTableLookup};
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
    programs: &BTreeSet<Pubkey>,
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
    for (key, account) in fetch(url, &missing, "account", programs)? {
        raw.insert(key, account);
    }
    let mut extra: Vec<Pubkey> = raw
        .values()
        .filter_map(programdata)
        .filter(|programdata| !raw.contains_key(programdata))
        .collect();
    extra.sort();
    extra.dedup();
    for (key, account) in fetch(url, &extra, "programdata", programs)? {
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
    loaded: Option<&LoadedAddresses>,
) -> Result<[u8; 32]> {
    tx.sanitize().context("malformed transaction")?;
    let mut keys: Vec<Pubkey> = tx.message.static_account_keys().iter().map(pk).collect();
    let lookups = tx.message.address_table_lookups().unwrap_or(&[]);
    let mut already = BTreeMap::new();
    let loaded = if lookups.is_empty() {
        None
    } else if let Some(loaded) = loaded {
        Some(loaded.clone())
    } else {
        Some(loaded_from_live_tables(url, lookups)?)
    };
    if let Some(loaded) = &loaded {
        already = compact_tables(lookups, loaded)?;
        for key in already.keys() {
            keys.push(*key);
        }
        for key in loaded.writable.iter().chain(&loaded.readonly) {
            keys.push(pk(key));
        }
        let mut loaded_keys: Vec<Pubkey> = loaded
            .writable
            .iter()
            .chain(&loaded.readonly)
            .map(pk)
            .collect();
        loaded_keys.sort();
        loaded_keys.dedup();
        for (key, account) in fetch(url, &loaded_keys, "loaded account", &BTreeSet::new())? {
            already.insert(key, account);
        }
    }
    keys.sort();
    keys.dedup();
    let already = (!already.is_empty()).then_some(&already);
    let programs = program_ids(tx, loaded.as_ref());
    let state = store_accounts(storage, &keys, url, already, &programs)?;
    ingest_programs(storage, &state)?;
    ingest_idls(storage, url, &programs, &state)?;
    store_state(storage, &state)
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

fn loaded_from_live_tables(
    url: &str,
    lookups: &[MessageAddressTableLookup],
) -> Result<LoadedAddresses> {
    let mut table_keys: Vec<Pubkey> = lookups
        .iter()
        .map(|lookup| pk(&lookup.account_key))
        .collect();
    table_keys.sort();
    table_keys.dedup();
    let fetched = fetch(url, &table_keys, "lookup table", &BTreeSet::new())?;
    let tables: BTreeMap<Pubkey, Account> = fetched.into_iter().collect();
    let mut writable = Vec::new();
    let mut readonly = Vec::new();
    for lookup in lookups {
        let key = pk(&lookup.account_key);
        let account = tables
            .get(&key)
            .with_context(|| format!("missing lookup table {key}"))?;
        if !solana_sdk_ids::address_lookup_table::check_id(&account.owner) {
            bail!("{key} is not an address lookup table");
        }
        let table = AddressLookupTable::deserialize(&account.data)
            .map_err(|e| anyhow::anyhow!("invalid address lookup table {key}: {e}"))?;
        for index in &lookup.writable_indexes {
            writable.push(
                table
                    .addresses
                    .get(usize::from(*index))
                    .copied()
                    .with_context(|| format!("lookup index {index} out of range in {key}"))?,
            );
        }
        for index in &lookup.readonly_indexes {
            readonly.push(
                table
                    .addresses
                    .get(usize::from(*index))
                    .copied()
                    .with_context(|| format!("lookup index {index} out of range in {key}"))?,
            );
        }
    }
    Ok(LoadedAddresses {
        writable: writable
            .into_iter()
            .map(|addr| addr.to_bytes().into())
            .collect(),
        readonly: readonly
            .into_iter()
            .map(|addr| addr.to_bytes().into())
            .collect(),
    })
}

fn compact_tables(
    lookups: &[MessageAddressTableLookup],
    loaded: &LoadedAddresses,
) -> Result<BTreeMap<Pubkey, Account>> {
    let mut writable = loaded.writable.iter();
    let mut readonly = loaded.readonly.iter();
    let mut slots: BTreeMap<Pubkey, BTreeMap<u8, Pubkey>> = BTreeMap::new();
    for lookup in lookups {
        let table = pk(&lookup.account_key);
        let slot = slots.entry(table).or_default();
        for index in &lookup.writable_indexes {
            let addr = writable.next().with_context(|| {
                format!("loadedAddresses.writable shorter than lookups in {table}")
            })?;
            insert_slot(slot, *index, pk(addr), table)?;
        }
        for index in &lookup.readonly_indexes {
            let addr = readonly.next().with_context(|| {
                format!("loadedAddresses.readonly shorter than lookups in {table}")
            })?;
            insert_slot(slot, *index, pk(addr), table)?;
        }
    }
    if writable.next().is_some() || readonly.next().is_some() {
        bail!("loadedAddresses longer than lookup indexes");
    }
    slots
        .into_iter()
        .map(|(table, indexes)| Ok((table, compact_account(&indexes)?)))
        .collect()
}

fn insert_slot(
    slot: &mut BTreeMap<u8, Pubkey>,
    index: u8,
    addr: Pubkey,
    table: Pubkey,
) -> Result<()> {
    if let Some(got) = slot.insert(index, addr) {
        if got != addr {
            bail!("lookup table {table} index {index} maps to both {got} and {addr}");
        }
    }
    Ok(())
}

fn compact_account(indexes: &BTreeMap<u8, Pubkey>) -> Result<Account> {
    let n = indexes
        .keys()
        .next_back()
        .map(|max| usize::from(*max).saturating_add(1))
        .unwrap_or(0);
    let mut addresses = vec![Pubkey::default(); n];
    for (index, addr) in indexes {
        addresses[usize::from(*index)] = *addr;
    }
    let start = u8::try_from(n).unwrap_or(u8::MAX);
    let meta = LookupTableMeta {
        last_extended_slot_start_index: start,
        ..LookupTableMeta::default()
    };
    let mut data = vec![0u8; LOOKUP_TABLE_META_SIZE.saturating_add(n.saturating_mul(32))];
    AddressLookupTable::overwrite_meta_data(&mut data, meta)
        .map_err(|e| anyhow::anyhow!("lookup table meta: {e}"))?;
    for (i, addr) in addresses.iter().enumerate() {
        let off = LOOKUP_TABLE_META_SIZE.saturating_add(i.saturating_mul(32));
        data[off..off.saturating_add(32)].copy_from_slice(addr.as_ref());
    }
    Ok(Account {
        lamports: 1,
        data,
        owner: solana_sdk_ids::address_lookup_table::id().to_bytes().into(),
        executable: false,
        rent_epoch: u64::MAX,
    })
}

fn program_ids(tx: &VersionedTransaction, loaded: Option<&LoadedAddresses>) -> BTreeSet<Pubkey> {
    let mut keys: Vec<Pubkey> = tx.message.static_account_keys().iter().map(pk).collect();
    if let Some(loaded) = loaded {
        keys.extend(loaded.writable.iter().map(pk));
        keys.extend(loaded.readonly.iter().map(pk));
    }
    tx.message
        .instructions()
        .iter()
        .filter_map(|ix| keys.get(usize::from(ix.program_id_index)).copied())
        .collect()
}

fn fetch(
    url: &str,
    keys: &[Pubkey],
    label: &str,
    programs: &BTreeSet<Pubkey>,
) -> Result<Vec<(Pubkey, Account)>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    get_multiple_accounts(url, keys)?
        .into_iter()
        .zip(keys)
        .map(|(account, key)| match account {
            Some(account) => Ok((*key, account)),
            None => Ok((*key, absent_account(key, label, programs)?)),
        })
        .collect()
}

fn absent_account(key: &Pubkey, label: &str, programs: &BTreeSet<Pubkey>) -> Result<Account> {
    if label == "account" && !programs.contains(key) {
        return Ok(Account::default());
    }
    bail!("missing {label} {key}");
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

fn pk(addr: &solana_address::Address) -> Pubkey {
    Pubkey::from(addr.to_bytes())
}

pub(crate) fn account_datas(
    storage: &Storage,
    state: &StateAccounts,
) -> Result<Vec<(Pubkey, Vec<u8>)>> {
    state
        .0
        .iter()
        .map(|(key, account)| Ok((*key, storage.blob.read(&account.data)?)))
        .collect()
}

fn ingest_programs(storage: &Storage, state: &StateAccounts) -> Result<()> {
    let accounts = account_datas(storage, state)?;
    for (key, account) in &state.0 {
        if !account.executable {
            continue;
        }
        let elf = trace::program_elf::program_elf_bytes(&accounts, key);
        if elf.is_empty() {
            continue;
        }
        let hash = storage.blob.store(&elf)?;
        storage.db.insert_program(&hash)?;
    }
    Ok(())
}

fn ingest_idls(
    storage: &Storage,
    url: &str,
    programs: &BTreeSet<Pubkey>,
    state: &StateAccounts,
) -> Result<()> {
    let accounts = account_datas(storage, state)?;
    let mut missing = Vec::new();
    let mut pending = Vec::new();
    for program_id in programs {
        if idl::builtin(program_id).is_some() {
            continue;
        }
        let elf = trace::program_elf::program_elf_bytes(&accounts, program_id);
        if elf.is_empty() {
            continue;
        }
        let elf_hash = storage.blob.store(&elf)?;
        storage.db.insert_program(&elf_hash)?;
        if storage.db.program_idl_blob_hash(&elf_hash)?.is_some() {
            continue;
        }
        let Ok(addr) = idl::anchor_idl_address(program_id) else {
            continue;
        };
        missing.push(addr);
        pending.push((elf_hash, addr));
    }
    if missing.is_empty() {
        return Ok(());
    }
    let fetched = get_multiple_accounts(url, &missing)?;
    for ((elf_hash, _), account) in pending.into_iter().zip(fetched) {
        let Some(account) = account else {
            continue;
        };
        let Ok(json) = idl::idl_json_from_anchor_account(&account.data) else {
            continue;
        };
        let idl_hash = storage.blob.store(json.as_bytes())?;
        storage.db.set_program_idl_blob_hash(&elf_hash, &idl_hash)?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_message::v0::MessageAddressTableLookup;

    #[test]
    fn compact_tables_roundtrip_indexes() {
        let table = solana_address::Address::from([1u8; 32]);
        let w = solana_address::Address::from([2u8; 32]);
        let r = solana_address::Address::from([3u8; 32]);
        let lookups = [MessageAddressTableLookup {
            account_key: table,
            writable_indexes: vec![2],
            readonly_indexes: vec![0],
        }];
        let loaded = LoadedAddresses {
            writable: vec![w],
            readonly: vec![r],
        };
        let got = compact_tables(&lookups, &loaded).unwrap();
        let account = &got[&pk(&table)];
        let parsed = AddressLookupTable::deserialize(&account.data).unwrap();
        assert_eq!(parsed.addresses[2], pk(&w).to_bytes().into());
        assert_eq!(parsed.addresses[0], pk(&r).to_bytes().into());
        assert_eq!(parsed.addresses.len(), 3);
        assert!(solana_sdk_ids::address_lookup_table::check_id(
            &account.owner
        ));
    }

    #[test]
    fn compact_tables_rejects_short_loaded() {
        let lookups = [MessageAddressTableLookup {
            account_key: solana_address::Address::from([9u8; 32]),
            writable_indexes: vec![0, 1],
            readonly_indexes: vec![],
        }];
        let loaded = LoadedAddresses {
            writable: vec![solana_address::Address::from([8u8; 32])],
            readonly: vec![],
        };
        assert!(compact_tables(&lookups, &loaded).is_err());
    }

    #[test]
    fn absent_account_defaults_non_programs() {
        let key = Pubkey::from([4u8; 32]);
        let got = absent_account(&key, "account", &BTreeSet::new()).unwrap();
        assert_eq!(got.lamports, 0);
        assert!(got.data.is_empty());
        let err = absent_account(&key, "account", &BTreeSet::from([key])).unwrap_err();
        assert!(err.to_string().contains("missing account"));
        let err = absent_account(&key, "loaded account", &BTreeSet::new()).unwrap_err();
        assert!(err.to_string().contains("missing loaded account"));
        let err = absent_account(&key, "lookup table", &BTreeSet::new()).unwrap_err();
        assert!(err.to_string().contains("missing lookup table"));
        let err = absent_account(&key, "programdata", &BTreeSet::new()).unwrap_err();
        assert!(err.to_string().contains("missing programdata"));
    }
}
