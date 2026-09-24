use std::collections::HashMap;
use std::sync::Arc;

use solana_pubkey::Pubkey;
use storage::Storage;
use trace::program_elf::program_elf_bytes;
use trace::tree::nodes::{
    account::TreeAccount,
    entrypoint::{EntrypointViewChildren, TreeEntrypoint},
    error::TreeError,
    fn_call::{FnCallViewChildren, TreeFnCall},
    root::{RootViewChildren, TreeRoot},
};

use crate::{builtin, IdlLookup, IdlTreeParser};

/// Fill `parsed` slots from builtins and `program.idl_blob_hash`. Does not write storage.
pub fn decorate(
    tree: &mut TreeRoot<RootViewChildren>,
    storage: &Storage,
    accounts: &[(Pubkey, Vec<u8>)],
) {
    let mut cache: HashMap<Pubkey, Option<Arc<IdlLookup>>> = HashMap::new();
    decorate_root(tree, storage, accounts, &mut cache);
}

pub fn decorate_bytes(
    bytes: &[u8],
    storage: &Storage,
    accounts: &[(Pubkey, Vec<u8>)],
) -> anyhow::Result<TreeRoot<RootViewChildren>> {
    let mut tree: TreeRoot<RootViewChildren> = serde_json::from_slice(bytes)?;
    decorate(&mut tree, storage, accounts);
    Ok(tree)
}

fn parser_for(
    program_id: &Pubkey,
    storage: &Storage,
    accounts: &[(Pubkey, Vec<u8>)],
    cache: &mut HashMap<Pubkey, Option<Arc<IdlLookup>>>,
) -> Option<Arc<IdlLookup>> {
    if let Some(hit) = cache.get(program_id) {
        return hit.clone();
    }
    let lookup = lookup(program_id, storage, accounts).map(Arc::new);
    cache.insert(*program_id, lookup.clone());
    lookup
}

fn lookup(
    program_id: &Pubkey,
    storage: &Storage,
    accounts: &[(Pubkey, Vec<u8>)],
) -> Option<IdlLookup> {
    if let Some(builtin) = builtin(program_id) {
        return Some(builtin);
    }
    let elf = program_elf_bytes(accounts, program_id);
    if elf.is_empty() {
        return None;
    }
    let hash = storage.blob.hash(&elf);
    let idl_hash = storage.db.program_idl_blob_hash(&hash).ok()??;
    let json = String::from_utf8(storage.blob.read(&idl_hash).ok()?).ok()?;
    IdlLookup::new(&json, &program_id.to_string()).ok()
}

fn decorate_root(
    tree: &mut TreeRoot<RootViewChildren>,
    storage: &Storage,
    accounts: &[(Pubkey, Vec<u8>)],
    cache: &mut HashMap<Pubkey, Option<Arc<IdlLookup>>>,
) {
    let parser = parser_for(&tree.receiver, storage, accounts, cache);
    if let Some(parser) = parser.as_ref() {
        if tree.parsed.is_none() {
            tree.parsed = parser.get_instruction(&tree.data);
        }
    }
    for child in &mut tree.children {
        match child {
            RootViewChildren::Invoke(inner) => decorate_root(inner, storage, accounts, cache),
            RootViewChildren::Entrypoint(e) => {
                decorate_entrypoint(e, parser.as_deref(), storage, accounts, cache)
            }
            RootViewChildren::Account(a) => {
                if let Some(parser) = parser.as_deref() {
                    decorate_account(a, parser);
                }
            }
            RootViewChildren::Error(e) => {
                if let Some(parser) = parser.as_deref() {
                    decorate_error(e, parser);
                }
            }
            RootViewChildren::Log(_) => {}
        }
    }
}

fn decorate_entrypoint(
    node: &mut TreeEntrypoint<EntrypointViewChildren>,
    parser: Option<&IdlLookup>,
    storage: &Storage,
    accounts: &[(Pubkey, Vec<u8>)],
    cache: &mut HashMap<Pubkey, Option<Arc<IdlLookup>>>,
) {
    for child in &mut node.children {
        match child {
            EntrypointViewChildren::Invoke(inner) => decorate_root(inner, storage, accounts, cache),
            EntrypointViewChildren::Entrypoint(e) => {
                decorate_entrypoint(e, parser, storage, accounts, cache)
            }
            EntrypointViewChildren::FnCall(f) => {
                decorate_fn_call(f, parser, storage, accounts, cache)
            }
            EntrypointViewChildren::Account(a) => {
                if let Some(parser) = parser {
                    decorate_account(a, parser);
                }
            }
            EntrypointViewChildren::Error(e) => {
                if let Some(parser) = parser {
                    decorate_error(e, parser);
                }
            }
            EntrypointViewChildren::Log(_) => {}
        }
    }
}

fn decorate_fn_call(
    node: &mut TreeFnCall<FnCallViewChildren>,
    parser: Option<&IdlLookup>,
    storage: &Storage,
    accounts: &[(Pubkey, Vec<u8>)],
    cache: &mut HashMap<Pubkey, Option<Arc<IdlLookup>>>,
) {
    for child in &mut node.children {
        match child {
            FnCallViewChildren::Invoke(inner) => decorate_root(inner, storage, accounts, cache),
            FnCallViewChildren::Entrypoint(e) => {
                decorate_entrypoint(e, parser, storage, accounts, cache)
            }
            FnCallViewChildren::FnCall(f) => decorate_fn_call(f, parser, storage, accounts, cache),
            FnCallViewChildren::Account(a) => {
                if let Some(parser) = parser {
                    decorate_account(a, parser);
                }
            }
            FnCallViewChildren::Error(e) => {
                if let Some(parser) = parser {
                    decorate_error(e, parser);
                }
            }
            FnCallViewChildren::Log(_) => {}
        }
    }
}

fn decorate_account(account: &mut TreeAccount, parser: &IdlLookup) {
    if account.before.parsed().is_none() {
        account
            .before
            .set_parsed(parser.get_account(account.before.data()));
    }
    if account.after.parsed().is_none() {
        account
            .after
            .set_parsed(parser.get_account(account.after.data()));
    }
}

fn decorate_error(error: &mut TreeError, parser: &IdlLookup) {
    if error.parsed.is_some() {
        return;
    }
    let name = parser.get_error(error.instruction_error.clone());
    if name != error.instruction_error.to_string() {
        error.parsed = Some(name);
    }
}
