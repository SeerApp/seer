use std::{
    collections::{hash_map::Entry, HashMap},
    fs,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bincode::serialized_size;
use reqwest::blocking::Client;
use serde_json::json;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;

use crate::{
    entrypoint_lookup::EntrypointLookup,
    errors::{IrrecoverableError, Warning},
    idl::{IdlLoadError, IdlLookup},
    path_resolver::PathResolver,
    program_manager::{entrypoints::get_entrypoint, known_programs::add_known_programs},
    seer_debug, seer_warn,
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
    disasm_requests_tx: mpsc::Sender<Pubkey>,
    disasm_status: Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisasmStatus {
    Pending,
    Succeeded,
    Failed,
}

impl ProgramManager {
    /// `runtime_dir` — project (or fixture) root: program artifacts are loaded from
    /// `runtime_dir/target/deploy` (`*.debug`, keypairs, etc.).
    ///
    /// `dwarf_compile_dir` — root of the tree referenced by paths embedded in DWARF (the build
    /// workspace). Used with `runtime_dir` when resolving sources at runtime.
    pub fn init(
        runtime_dir: &PathBuf,
        dwarf_compile_dir: &PathBuf,
        network_rpc_url: Option<String>,
    ) -> Result<(Self, Vec<Warning>), IrrecoverableError> {
        let mut inner: HashMap<Pubkey, ProgramInfo> = HashMap::new();
        let mut warnings = vec![];

        add_known_programs(&mut inner);

        let target_dir = runtime_dir.join("target");
        let targets = get_targets(&target_dir)?;
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

        let disasm_status = Arc::new(Mutex::new(HashMap::new()));
        let (disasm_requests_tx, disasm_requests_rx) = mpsc::channel::<Pubkey>();
        let disasm_output_dir = runtime_dir.join("seer").join("disasm");
        start_disasm_worker(
            disasm_requests_rx,
            disasm_status.clone(),
            network_rpc_url,
            disasm_output_dir,
        );

        Ok((
            Self {
                inner,
                disasm_requests_tx,
                disasm_status,
            },
            warnings,
        ))
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

fn start_disasm_worker(
    disasm_requests_rx: mpsc::Receiver<Pubkey>,
    disasm_status: Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    network_rpc_url: Option<String>,
    disasm_output_dir: PathBuf,
) {
    thread::spawn(move || {
        for program_id in disasm_requests_rx {
            let result = maybe_download_and_disassemble(
                &program_id,
                network_rpc_url.as_deref(),
                &disasm_output_dir,
            );
            let new_status = if result.is_ok() {
                DisasmStatus::Succeeded
            } else {
                DisasmStatus::Failed
            };
            if let Ok(mut guard) = disasm_status.lock() {
                guard.insert(program_id, new_status);
            }
            if let Err(err) = result {
                seer_warn!("background disasm failed for {}: {}", program_id, err);
            } else {
                seer_debug!("background disasm completed for {}", program_id);
            }
        }
    });
}

fn maybe_download_and_disassemble(
    program_id: &Pubkey,
    network_rpc_url: Option<&str>,
    disasm_output_dir: &PathBuf,
) -> Result<(), String> {
    let Some(rpc_url) = network_rpc_url else {
        return Err("network RPC URL not configured".to_string());
    };
    fs::create_dir_all(disasm_output_dir)
        .map_err(|e| format!("create disasm dir {}: {}", disasm_output_dir.display(), e))?;

    let so_path = disasm_output_dir.join(format!("{program_id}.so"));
    if !so_path.exists() {
        let elf_bytes = fetch_program_elf_from_rpc(rpc_url, program_id)
            .map_err(|e| format!("fetch ELF for {}: {}", program_id, e))?;
        fs::write(&so_path, &elf_bytes)
            .map_err(|e| format!("write {}: {}", so_path.display(), e))?;
    }

    disasm::disassemble_to_json_chunks(&so_path, disasm_output_dir)
        .map_err(|e| format!("disassemble {}: {}", so_path.display(), e))
}

fn fetch_program_elf_from_rpc(rpc_url: &str, program_id: &Pubkey) -> Result<Vec<u8>, String> {
    let program_account_data = fetch_account_data(rpc_url, program_id)?;
    let program_state: UpgradeableLoaderState = bincode::deserialize(&program_account_data)
        .map_err(|e| format!("deserialize program account state: {}", e))?;
    let programdata_address = match program_state {
        UpgradeableLoaderState::Program {
            programdata_address,
        } => Pubkey::new_from_array(programdata_address.to_bytes()),
        _ => {
            return Err("program account is not an UpgradeableLoaderState::Program".to_string());
        }
    };

    let programdata_bytes = fetch_account_data(rpc_url, &programdata_address)?;
    let programdata_state: UpgradeableLoaderState = bincode::deserialize(&programdata_bytes)
        .map_err(|e| format!("deserialize programdata state: {}", e))?;
    let offset = match programdata_state {
        UpgradeableLoaderState::ProgramData {
            upgrade_authority_address,
            ..
        } => {
            if upgrade_authority_address.is_some() {
                UpgradeableLoaderState::size_of_programdata_metadata()
            } else {
                UpgradeableLoaderState::size_of_programdata_metadata()
                    - serialized_size(&Pubkey::default()).unwrap_or(0) as usize
            }
        }
        _ => {
            return Err(
                "programdata account is not an UpgradeableLoaderState::ProgramData".to_string(),
            );
        }
    };
    if programdata_bytes.len() <= offset {
        return Err(format!(
            "programdata account too small ({} bytes, need > {})",
            programdata_bytes.len(),
            offset
        ));
    }
    Ok(programdata_bytes[offset..].to_vec())
}

fn fetch_account_data(rpc_url: &str, account: &Pubkey) -> Result<Vec<u8>, String> {
    let client = Client::new();
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getAccountInfo",
        "params": [
            account.to_string(),
            {
                "encoding": "base64",
                "commitment": "confirmed"
            }
        ]
    });
    let response_value: serde_json::Value = client
        .post(rpc_url)
        .json(&payload)
        .send()
        .map_err(|e| format!("rpc request failed: {}", e))?
        .json()
        .map_err(|e| format!("rpc response parse failed: {}", e))?;
    if let Some(err) = response_value.get("error") {
        return Err(format!("rpc returned error: {}", err));
    }
    let data_b64 = response_value
        .get("result")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing account data in rpc response".to_string())?;
    BASE64_STANDARD
        .decode(data_b64)
        .map_err(|e| format!("base64 decode account data: {}", e))
}
