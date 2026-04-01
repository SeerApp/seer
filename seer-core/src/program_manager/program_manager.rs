use std::{
    collections::{hash_map::Entry, HashMap},
    path::PathBuf,
};

use solana_pubkey::Pubkey;

use crate::{
    entrypoint_lookup::EntrypointLookup,
    errors::{IrrecoverableError, Warning},
    idl::{IdlLoadError, IdlLookup},
    path_resolver::PathResolver,
    program_manager::{entrypoints::get_entrypoint, known_programs::add_known_programs},
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
    pub fn init(
        runtime_dir: &PathBuf,
        dwarf_compile_dir: &PathBuf,
    ) -> Result<(Self, Vec<Warning>), IrrecoverableError> {
        let mut inner: HashMap<Pubkey, ProgramInfo> = HashMap::new();
        let mut warnings = vec![];

        add_known_programs(&mut inner);

        let targets = get_targets(runtime_dir)?;
        let path_resolver = PathResolver::new(dwarf_compile_dir.clone(), runtime_dir.clone());

        for (key, target) in &targets {
            let key_str = key.to_string();
            let program_name = target.base.clone();
            let (new_entrypoint_lookup, entrypoint_detail) = match get_entrypoint(
                target,
                path_resolver.clone(),
            ) {
                Ok(entrypoint_lookup) => (
                    entrypoint_lookup,
                    "Debug data was impossible to parse. Make sure you are using Solana CLI v3+."
                        .to_string(),
                ),
                Err(err) => (None, err.to_string()),
            };
            if new_entrypoint_lookup.is_none() {
                warnings.push(Warning::UnparsableDebugFile {
                    key: key_str.clone(),
                    program: program_name.clone(),
                    detail: entrypoint_detail,
                });
            }

            let (new_idl_lookup, idl_detail) = match IdlLookup::new_from_target(target) {
                Ok(idl_lookup) => (Some(idl_lookup), "IDL was impossible to parse. Make sure you are using Anchor v0.30.0+ or Codama IDL.".to_string()),
                Err(IdlLoadError::Warning(warning_detail)) => (None, warning_detail),
                Err(IdlLoadError::Irrecoverable(err)) => (None, err.to_string()),
            };
            if new_idl_lookup.is_none() {
                warnings.push(Warning::UnparsableIdlFile {
                    key: key_str,
                    program: program_name,
                    detail: idl_detail,
                });
            };

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

        Ok((Self { inner }, warnings))
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
