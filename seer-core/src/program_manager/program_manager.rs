//! Purpose: orchestrate program manager behavior across module layers.

use std::{
    collections::{hash_map::Entry, HashMap},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
};

use solana_pubkey::Pubkey;

use crate::{
    artifacts::AtomicFileWriter,
    entrypoint_lookup::EntrypointLookup,
    errors::IrrecoverableError,
    idl::{parsed_arg::collect_parsed_arg_byte_offsets, IdlLoadError, IdlLookup, IdlTreeParser},
    path_resolver::PathResolver,
    program_manager::types::{AccountIdlParseResult, DisasmStatus, ProgramInfo},
    seer_warn,
    target_reader::get_targets,
};

impl super::types::GlobalProgramContext {
    /// `runtime_dir` — project (or fixture) root: program artifacts are loaded from
    /// `runtime_dir/target/deploy` (`*.debug`, keypairs, etc.).
    ///
    /// `dwarf_compile_dir` — root of the tree referenced by paths embedded in DWARF (the build
    /// workspace). Used with `runtime_dir` when resolving sources at runtime.
    pub fn init(
        runtime_dir: &PathBuf,
        dwarf_compile_dir: &PathBuf,
        network_rpc_url: Option<String>,
        file_writer: Arc<Mutex<AtomicFileWriter>>,
    ) -> Result<Self, IrrecoverableError> {
        let mut inner: HashMap<Pubkey, ProgramInfo> = HashMap::new();

        super::known_programs::add_known_programs(&mut inner);

        let target_dir = runtime_dir.join("target");
        let targets = get_targets(&target_dir)?;
        let path_resolver = PathResolver::new(dwarf_compile_dir.clone(), runtime_dir.clone());
        let file_writer_guard = file_writer
            .lock()
            .expect("file writer lock should not be poisoned");

        for (key, target) in &targets {
            let key_str = key.to_string();
            let program_name = target.base.clone();
            let (new_entrypoint_lookup, entrypoint_detail) = match super::entrypoints::get_entrypoint(
                target,
                path_resolver.clone(),
                &file_writer_guard,
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

            let (new_idl_lookup, idl_detail) = match IdlLookup::new_from_target(target) {
                Ok(idl_lookup) => (
                    Some(idl_lookup),
                    "IDL was impossible to parse. Make sure you are using Anchor v0.30.0+ or Codama IDL.".to_string(),
                ),
                Err(IdlLoadError::Warning(warning_detail)) => (None, warning_detail),
                Err(IdlLoadError::Irrecoverable(err)) => (None, err.to_string()),
            };
            if new_idl_lookup.is_none() {
                seer_warn!(
                    "Your idl file is not parsable for program {} ({}): {}",
                    key_str,
                    program_name,
                    idl_detail
                );
            }

            match inner.entry(*key) {
                Entry::Occupied(mut occupied) => {
                    let existing = occupied.get_mut();

                    if existing.entrypoint_lookup.is_none() && new_entrypoint_lookup.is_some() {
                        existing.entrypoint_lookup = new_entrypoint_lookup.map(Arc::new);
                    }
                    if existing.idl_lookup.is_none() && new_idl_lookup.is_some() {
                        existing.idl_lookup = new_idl_lookup.map(Arc::new);
                    }
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(ProgramInfo::new(new_entrypoint_lookup, new_idl_lookup));
                }
            }
        }

        drop(file_writer_guard);

        let disasm_status = Arc::new(Mutex::new(HashMap::new()));
        let (disasm_requests_tx, disasm_requests_rx) = mpsc::channel::<Pubkey>();
        let programs_output_dir = runtime_dir.join("seer").join("programs");
        {
            let file_writer_guard = file_writer
                .lock()
                .expect("file writer lock should not be poisoned");
            super::utils::preload_local_disasm_for_targets(
                &targets,
                &disasm_status,
                &programs_output_dir,
                &file_writer_guard,
            );
        }
        super::utils::start_disasm_worker(
            disasm_requests_rx,
            disasm_status.clone(),
            network_rpc_url.clone(),
            programs_output_dir,
            file_writer,
        );

        Ok(Self {
            inner: Mutex::new(inner),
            disasm_requests_tx,
            disasm_status,
            network_rpc_url,
            idl_fetch_failed: Mutex::new(HashMap::new()),
        })
    }

    pub fn get_entrypoint_lookup(&self, key: &Pubkey) -> Option<Arc<EntrypointLookup>> {
        self.inner
            .lock()
            .expect("program manager inner lock should not be poisoned")
            .get(key)
            .and_then(|program_info| program_info.entrypoint_lookup.clone())
    }

    pub fn get_idl_lookup(&self, key: &Pubkey) -> Option<Arc<IdlLookup>> {
        if let Some(existing) = self
            .inner
            .lock()
            .expect("program manager inner lock should not be poisoned")
            .get(key)
            .and_then(|program_info| program_info.idl_lookup.clone())
        {
            return Some(existing);
        }

        if self
            .idl_fetch_failed
            .lock()
            .expect("idl fetch cache lock should not be poisoned")
            .contains_key(key)
        {
            return None;
        }

        let Some(rpc_url) = self.network_rpc_url.as_deref() else {
            self.idl_fetch_failed
                .lock()
                .expect("idl fetch cache lock should not be poisoned")
                .insert(*key, ());
            return None;
        };

        match super::utils::fetch_anchor_idl_lookup_from_rpc(rpc_url, key) {
            Ok(idl_lookup) => {
                let idl_lookup = Arc::new(idl_lookup);
                let mut guard = self
                    .inner
                    .lock()
                    .expect("program manager inner lock should not be poisoned");
                match guard.entry(*key) {
                    Entry::Occupied(mut existing) => {
                        existing.get_mut().idl_lookup = Some(idl_lookup.clone());
                    }
                    Entry::Vacant(vacant) => {
                        vacant.insert(ProgramInfo {
                            entrypoint_lookup: None,
                            idl_lookup: Some(idl_lookup.clone()),
                        });
                    }
                }
                Some(idl_lookup)
            }
            Err(err) => {
                seer_warn!("rpc idl fetch failed for {}: {}", key, err);
                self.idl_fetch_failed
                    .lock()
                    .expect("idl fetch cache lock should not be poisoned")
                    .insert(*key, ());
                None
            }
        }
    }

    /// Fast-path lookup that never performs RPC/network fetches.
    pub fn get_idl_lookup_cached(&self, key: &Pubkey) -> Option<Arc<IdlLookup>> {
        self.inner
            .lock()
            .expect("program manager inner lock should not be poisoned")
            .get(key)
            .and_then(|program_info| program_info.idl_lookup.clone())
    }

    /// Parses `bytes` with a loaded IDL. Caller resolves which program owns the account.
    pub fn parse_account_with_idl(
        idl_lookup: &Arc<IdlLookup>,
        bytes: &[u8],
    ) -> Option<AccountIdlParseResult> {
        let parsed = idl_lookup.get_account(bytes)?;
        let parsed_byte_offsets = collect_parsed_arg_byte_offsets(&parsed.data);
        Some(AccountIdlParseResult {
            parsed,
            parsed_byte_offsets,
        })
    }

    pub fn queue_disasm_if_needed(&self, program_id: Pubkey) {
        let mut guard = self
            .disasm_status
            .lock()
            .expect("disasm status lock should not be poisoned");
        if guard.contains_key(&program_id) {
            return;
        }
        guard.insert(program_id, DisasmStatus::Pending);
        drop(guard);

        if let Err(err) = self.disasm_requests_tx.send(program_id) {
            seer_warn!(
                "failed to queue background disasm for {}: {}",
                program_id,
                err
            );
            if let Ok(mut status_guard) = self.disasm_status.lock() {
                status_guard.insert(program_id, DisasmStatus::Failed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::utils::{fetch_anchor_idl_lookup_from_rpc, fetch_program_elf_from_rpc};
    use crate::idl::IdlLookup;
    use crate::program_manager::types::{DisasmStatus, GlobalProgramContext, ProgramInfo};
    use solana_pubkey::Pubkey;
    use std::{
        collections::HashMap,
        str::FromStr,
        sync::{mpsc, Arc, Mutex},
    };

    #[test]
    fn fetches_mainnet_associated_token_program_elf() {
        let rpc_url = "https://api.mainnet-beta.solana.com";
        let program_id = Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
            .expect("AToken program id should be a valid pubkey");

        let elf_bytes = fetch_program_elf_from_rpc(rpc_url, &program_id)
            .expect("AToken mainnet account should parse into ELF bytes");

        assert!(
            elf_bytes.starts_with(b"\x7FELF"),
            "expected ELF header in fetched bytes"
        );
        assert!(
            elf_bytes.len() > 4,
            "expected non-trivial ELF payload from mainnet"
        );
    }

    #[test]
    fn skips_system_program_elf_fetch_without_rpc() {
        let rpc_url = "https://api.mainnet-beta.solana.com";
        let elf_bytes = fetch_program_elf_from_rpc(
            rpc_url,
            &crate::program_manager::known_programs::SYSTEM_PROGRAM_PUBKEY,
        )
        .expect("system program should short-circuit, not error");
        assert!(elf_bytes.is_empty());
    }

    #[test]
    fn fetches_mainnet_anchor_idl_for_pamm() {
        let rpc_url = "https://api.mainnet-beta.solana.com";
        let program_id = Pubkey::from_str("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA")
            .expect("pAMM program id should be a valid pubkey");

        let idl_lookup = fetch_anchor_idl_lookup_from_rpc(rpc_url, &program_id)
            .expect("expected Anchor IDL fetch+parse to succeed for pAMM");

        match idl_lookup {
            IdlLookup::Anchor(_) | IdlLookup::Codama(_) => {}
        }
    }

    #[test]
    fn idl_rpc_failure_is_graceful_and_does_not_block_execution() {
        let (tx, _rx) = mpsc::channel();
        let manager = GlobalProgramContext {
            inner: Mutex::new(HashMap::<Pubkey, ProgramInfo>::new()),
            disasm_requests_tx: tx,
            disasm_status: Arc::new(Mutex::new(HashMap::new())),
            network_rpc_url: Some("http://127.0.0.1:1".to_string()),
            idl_fetch_failed: Mutex::new(HashMap::new()),
        };

        let program_id = Pubkey::new_unique();
        let idl_lookup = manager.get_idl_lookup(&program_id);
        assert!(
            idl_lookup.is_none(),
            "IDL fetch failure should return None, not panic"
        );
        assert!(
            manager
                .idl_fetch_failed
                .lock()
                .expect("idl fetch cache lock should not be poisoned")
                .contains_key(&program_id),
            "failed lookups should be memoized to avoid repeated failing RPC calls"
        );

        manager.queue_disasm_if_needed(program_id);
        let status = manager
            .disasm_status
            .lock()
            .expect("disasm status lock should not be poisoned")
            .get(&program_id)
            .copied();
        assert!(
            matches!(
                status,
                Some(DisasmStatus::Pending) | Some(DisasmStatus::Failed)
            ),
            "program manager should keep functioning after IDL RPC failure"
        );
    }
}
