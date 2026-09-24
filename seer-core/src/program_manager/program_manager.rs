//! Purpose: orchestrate program manager behavior across module layers.

use std::{
    collections::{hash_map::Entry, HashMap},
    path::Path,
    sync::{Arc, Mutex},
};

use solana_pubkey::Pubkey;

use crate::{
    errors::IrrecoverableError, path_resolver::PathResolver, program_manager::types::ProgramInfo,
    seer_warn, target_reader::get_targets,
};

impl super::types::GlobalProgramContext {
    /// `runtime_dir` — project (or fixture) root: program artifacts are loaded from
    /// `runtime_dir/target/deploy` (`*.debug`, keypairs, etc.).
    ///
    /// `dwarf_compile_dir` — root of the tree referenced by paths embedded in DWARF (the build
    /// workspace). Used with `runtime_dir` when resolving sources at runtime.
    pub fn init(runtime_dir: &Path, dwarf_compile_dir: &Path) -> Result<Self, IrrecoverableError> {
        let mut inner: HashMap<Pubkey, ProgramInfo> = HashMap::new();

        let target_dir = runtime_dir.join("target");
        let targets = get_targets(&target_dir)?;
        let path_resolver =
            PathResolver::new(dwarf_compile_dir.to_path_buf(), runtime_dir.to_path_buf());

        for (key, target) in &targets {
            let key_str = key.to_string();
            let program_name = target.base.clone();
            let (new_entrypoint_lookup, entrypoint_detail) = match super::entrypoints::get_entrypoint(
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
                seer_warn!(
                    "Your debug file is not parsable for program {} ({}): {}",
                    key_str,
                    program_name,
                    entrypoint_detail
                );
            }

            match inner.entry(*key) {
                Entry::Occupied(mut occupied) => {
                    let existing = occupied.get_mut();
                    if existing.entrypoint_lookup.is_none() && new_entrypoint_lookup.is_some() {
                        existing.entrypoint_lookup = new_entrypoint_lookup.map(Arc::new);
                    }
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(ProgramInfo::new(new_entrypoint_lookup));
                }
            }
        }

        Ok(Self {
            inner: Mutex::new(inner),
        })
    }

    pub fn get_entrypoint_lookup(
        &self,
        key: &Pubkey,
    ) -> Option<Arc<crate::entrypoint_lookup::EntrypointLookup>> {
        self.inner
            .lock()
            .expect("program manager inner lock should not be poisoned")
            .get(key)
            .and_then(|program_info| program_info.entrypoint_lookup.clone())
    }
}
