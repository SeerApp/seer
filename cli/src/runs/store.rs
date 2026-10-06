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
    crate::sysvars::require(|key| raw.get(key).map(|account| account.data.as_slice()))?;
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

pub fn store_captured_accounts(
    storage: &Storage,
    tx: &VersionedTransaction,
    captured: &BTreeMap<Pubkey, Option<Account>>,
) -> Result<[u8; 32]> {
    tx.sanitize().context("malformed transaction")?;
    let lookups = tx.message.address_table_lookups().unwrap_or(&[]);
    let loaded = if lookups.is_empty() {
        None
    } else {
        Some(loaded_from_captured(captured, lookups)?)
    };
    let programs = program_ids(tx, loaded.as_ref());
    let mut required: BTreeSet<Pubkey> = tx
        .message
        .static_account_keys()
        .iter()
        .map(pk)
        .filter(|key| !instructions_sysvar(key))
        .collect();
    if let Some(loaded) = &loaded {
        for addr in loaded.writable.iter().chain(&loaded.readonly) {
            let key = pk(addr);
            if !instructions_sysvar(&key) {
                required.insert(key);
            }
        }
        for lookup in lookups {
            required.insert(pk(&lookup.account_key));
        }
    }
    for key in required.clone() {
        if let Some(Some(account)) = captured.get(&key) {
            if let Some(programdata) = programdata(account) {
                required.insert(programdata);
            }
        }
    }
    let mut raw = BTreeMap::new();
    for (key, slot) in captured {
        if instructions_sysvar(key) {
            continue;
        }
        let account = match slot {
            Some(account) => account.clone(),
            None => absent_account(key, "account", &programs)?,
        };
        raw.insert(*key, account);
    }
    for key in &required {
        if !raw.contains_key(key) {
            bail!("missing account {key}");
        }
    }
    crate::sysvars::require(|key| {
        captured
            .get(key)
            .and_then(|slot| slot.as_ref())
            .map(|account| account.data.as_slice())
    })?;
    let state = raw
        .into_iter()
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
        .map(StateAccounts)?;
    ingest_programs(storage, &state)?;
    store_state(storage, &state)
}

fn loaded_from_captured(
    accounts: &BTreeMap<Pubkey, Option<Account>>,
    lookups: &[MessageAddressTableLookup],
) -> Result<LoadedAddresses> {
    let mut writable = Vec::new();
    let mut readonly = Vec::new();
    for lookup in lookups {
        let key = pk(&lookup.account_key);
        let Some(Some(account)) = accounts.get(&key) else {
            bail!("missing lookup table {key}");
        };
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
    let programs = program_ids(tx, loaded.as_ref());
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
        loaded_keys.retain(|key| !instructions_sysvar(key));
        for (key, account) in fetch(url, &loaded_keys, "loaded account", &programs)? {
            already.insert(key, account);
        }
    }
    keys.sort();
    keys.dedup();
    keys.retain(|key| !instructions_sysvar(key));
    crate::sysvars::include(&mut keys);
    let already = (!already.is_empty()).then_some(&already);
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

fn instructions_sysvar(key: &Pubkey) -> bool {
    solana_sdk_ids::sysvar::instructions::check_id(key)
}

fn absent_account(key: &Pubkey, label: &str, programs: &BTreeSet<Pubkey>) -> Result<Account> {
    if (label == "account" || label == "loaded account") && !programs.contains(key) {
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

pub(crate) fn program_elf_hash(storage: &Storage, run_id: i64, pk: &Pubkey) -> Result<[u8; 32]> {
    if let Some(hash) = storage
        .db
        .program_blob_hash_for_run(run_id, &pk.to_bytes())?
    {
        return Ok(hash);
    }
    let row = storage.db.get_run(run_id)?;
    let state = StateAccounts::from_bytes(&storage.blob.read(&row.state_blob_hash)?)?;
    let account = state
        .0
        .get(pk)
        .with_context(|| format!("no {pk} in run {run_id}"))?;
    let data = storage.blob.read(&account.data)?;
    let mut accounts = vec![(*pk, data)];
    if let Ok(UpgradeableLoaderState::Program {
        programdata_address,
    }) = bincode::deserialize(&accounts[0].1)
    {
        let pd = Pubkey::from(programdata_address.to_bytes());
        if let Some(row) = state.0.get(&pd) {
            accounts.push((pd, storage.blob.read(&row.data)?));
        }
    }
    let elf = trace::program_elf::program_elf_bytes(&accounts, pk);
    if elf.is_empty() {
        bail!("no ELF for {pk} in run {run_id}");
    }
    let hash = storage.blob.store(&elf)?;
    storage.db.insert_program(&hash)?;
    Ok(hash)
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
    fn captured_accounts_fail_when_a_required_key_is_absent() {
        let payer = Pubkey::from([7u8; 32]);
        let tx = VersionedTransaction::from(solana_transaction::Transaction::new_unsigned(
            solana_message::Message::new(&[], Some(&payer)),
        ));
        let root = std::env::temp_dir().join(format!(
            "seer-captured-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Storage::open_at(&root).unwrap();
        let err = store_captured_accounts(&storage, &tx, &BTreeMap::new()).unwrap_err();
        assert!(err.to_string().contains("missing account"), "{err}");
        let captured = BTreeMap::from([(payer, None)]);
        let err = store_captured_accounts(&storage, &tx, &captured).unwrap_err();
        assert!(err.to_string().contains("missing sysvar"), "{err}");
        let mut captured = crate::sysvars::valid_for_test();
        captured.insert(payer, None);
        assert!(store_captured_accounts(&storage, &tx, &captured).is_ok());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn instructions_sysvar_is_not_fetched() {
        let key = Pubkey::from(solana_sdk_ids::sysvar::instructions::id().to_bytes());
        assert!(!instructions_sysvar(&Pubkey::from([4u8; 32])));
        assert!(instructions_sysvar(&key));
    }

    #[test]
    fn absent_account_defaults_non_programs() {
        let key = Pubkey::from([4u8; 32]);
        let got = absent_account(&key, "account", &BTreeSet::new()).unwrap();
        assert_eq!(got.lamports, 0);
        assert!(got.data.is_empty());
        let err = absent_account(&key, "account", &BTreeSet::from([key])).unwrap_err();
        assert!(err.to_string().contains("missing account"));
        let got = absent_account(&key, "loaded account", &BTreeSet::new()).unwrap();
        assert_eq!(got.lamports, 0);
        assert!(got.data.is_empty());
        let err = absent_account(&key, "loaded account", &BTreeSet::from([key])).unwrap_err();
        assert!(err.to_string().contains("missing loaded account"));
        let err = absent_account(&key, "lookup table", &BTreeSet::new()).unwrap_err();
        assert!(err.to_string().contains("missing lookup table"));
        let err = absent_account(&key, "programdata", &BTreeSet::new()).unwrap_err();
        assert!(err.to_string().contains("missing programdata"));
    }

    #[test]
    fn program_elf_hash_does_not_read_other_blobs() {
        let root = std::env::temp_dir().join(format!(
            "seer-prog-elf-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Storage::open_at(&root).unwrap();
        let program = Pubkey::from([1u8; 32]);
        let programdata = Pubkey::from([2u8; 32]);
        let decoy = Pubkey::from([9u8; 32]);
        let elf = b"\x7FELFpayload";
        let header = bincode::serialize(&UpgradeableLoaderState::Program {
            programdata_address: programdata,
        })
        .unwrap();
        let mut pd = vec![0u8; UpgradeableLoaderState::size_of_programdata_metadata()];
        let pd_header = bincode::serialize(&UpgradeableLoaderState::ProgramData {
            slot: 1,
            upgrade_authority_address: Some(Pubkey::from([3u8; 32])),
        })
        .unwrap();
        pd[..pd_header.len()].copy_from_slice(&pd_header);
        pd.extend_from_slice(elf);
        let h_prog = storage.blob.store(&header).unwrap();
        let h_pd = storage.blob.store(&pd).unwrap();
        let mut state = StateAccounts::default();
        state.0.insert(
            program,
            StateAccount {
                lamports: 1,
                data: h_prog,
                owner: Pubkey::default(),
                executable: true,
            },
        );
        state.0.insert(
            programdata,
            StateAccount {
                lamports: 1,
                data: h_pd,
                owner: Pubkey::default(),
                executable: false,
            },
        );
        state.0.insert(
            decoy,
            StateAccount {
                lamports: 1,
                data: [0xee; 32],
                owner: Pubkey::default(),
                executable: false,
            },
        );
        assert!(account_datas(&storage, &state).is_err());
        let state_hash = storage.blob.store(&state.to_bytes().unwrap()).unwrap();
        let tx_hash = storage.blob.store(b"tx").unwrap();
        storage.db.insert_simulation(&tx_hash, &state_hash).unwrap();
        let id = storage
            .db
            .insert_run(&tx_hash, &state_hash, false, false, None, "")
            .unwrap();
        let got = program_elf_hash(&storage, id, &program).unwrap();
        assert_eq!(got, storage.blob.hash(elf));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn program_elf_hash_uses_reg_without_reading_elf() {
        let root = std::env::temp_dir().join(format!(
            "seer-prog-reg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Storage::open_at(&root).unwrap();
        let program = Pubkey::from([1u8; 32]);
        let elf_hash = storage.blob.store(b"already-hashed-elf").unwrap();
        storage.db.insert_program(&elf_hash).unwrap();
        let self_hash = storage.blob.store(b"self").unwrap();
        let state_hash = storage
            .blob
            .store(&StateAccounts::default().to_bytes().unwrap())
            .unwrap();
        let tx_hash = storage.blob.store(b"tx").unwrap();
        storage.db.insert_simulation(&tx_hash, &state_hash).unwrap();
        let id = storage
            .db
            .insert_run(&tx_hash, &state_hash, false, false, None, "")
            .unwrap();
        storage.db.insert_run_ix(id, 0).unwrap();
        storage
            .db
            .insert_reg(id, 0, 0, 1, &self_hash, &elf_hash, &program.to_bytes())
            .unwrap();
        let got = program_elf_hash(&storage, id, &program).unwrap();
        assert_eq!(got, elf_hash);
        std::fs::remove_dir_all(root).ok();
    }
}
