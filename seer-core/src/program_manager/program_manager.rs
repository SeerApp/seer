use std::{
    collections::{hash_map::Entry, HashMap},
    path::PathBuf,
};

use solana_pubkey::Pubkey;

use crate::{
    entrypoint_lookup::EntrypointLookup,
    idl::lookup::IdlLookup,
    path_resolver::PathResolver,
    program_manager::{entrypoints::get_entrypoint, idls::get_idl_from_target, known_programs::add_known_programs},
    target_reader::get_targets,
};

pub struct ProgramInfo {
    entrypoint_lookup: Option<EntrypointLookup>,
    idl_lookup: Option<IdlLookup>,
}

impl ProgramInfo {
    pub fn new(entrypoint_lookup: Option<EntrypointLookup>, idl_lookup: Option<IdlLookup>) -> Self {
        Self {
            entrypoint_lookup,
            idl_lookup,
        }
    }

    pub fn with_idl_lookup(idl_lookup: IdlLookup) -> Self {
        Self {
            entrypoint_lookup: None,
            idl_lookup: Some(idl_lookup),
        }
    }

    pub fn set_idl_lookup(&mut self, idl_lookup: IdlLookup) {
        self.idl_lookup = Some(idl_lookup);
    }
}

/// Collects ProgramInfo from 4 sources:
/// 1. Known hardcoded (immutable) program IDLs at Seer initialization.
/// 2. The targets provided by the user at Seer initialization.
/// 3. External programs from the Seer storage lazily during execution.
/// 4. Mainnet programs (IDLs only) from the Solana RPC as fallback lazily during execution.
pub struct ProgramManager {
    inner: HashMap<Pubkey, ProgramInfo>,
}

impl ProgramManager {
    pub fn init(runtime_dir: &PathBuf, dwarf_compile_dir: &PathBuf) -> Self {
        let mut inner: HashMap<Pubkey, ProgramInfo> = HashMap::new();

        add_known_programs(&mut inner);

        let targets = get_targets(&runtime_dir);
        let path_resolver = PathResolver::new(dwarf_compile_dir.clone(), runtime_dir.clone());

        for (key, target) in &targets {
            let new_entrypoint_lookup = get_entrypoint(target, path_resolver.clone());
            let new_idl_lookup = get_idl_from_target(target);

            match inner.entry(*key) {
                Entry::Occupied(mut occupied) => {
                    let existing = occupied.get_mut();

                    if existing.entrypoint_lookup.is_none() && new_entrypoint_lookup.is_some() {
                        existing.entrypoint_lookup = new_entrypoint_lookup;
                    }
                    if existing.idl_lookup.is_none() && new_idl_lookup.is_some() {
                        existing.idl_lookup = new_idl_lookup;
                    }
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(ProgramInfo::new(new_entrypoint_lookup, new_idl_lookup));
                }
            }
        }

        Self { inner }
    }

    pub fn get_entrypoint_lookup(&self, key: &Pubkey) -> Option<&EntrypointLookup> {
        self.inner
            .get(key)
            .and_then(|program_info| program_info.entrypoint_lookup.as_ref())
    }

    pub fn get_idl_lookup(&self, key: &Pubkey) -> Option<&IdlLookup> {
        self.inner
            .get(key)
            .and_then(|program_info| program_info.idl_lookup.as_ref())
    }
}
