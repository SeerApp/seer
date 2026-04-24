use std::{
    collections::{hash_map::Entry, HashMap},
    io::Read,
    fs,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bincode::serialized_size;
use flate2::read::ZlibDecoder;
use reqwest::blocking::Client;
use serde_json::json;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use tempfile::tempdir;

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
    entrypoint_lookup: Option<Arc<EntrypointLookup>>,
    idl_lookup: Option<Arc<IdlLookup>>,
}

impl ProgramInfo {
    pub fn new(entrypoint_lookup: Option<EntrypointLookup>, idl_lookup: Option<IdlLookup>) -> Self {
        Self {
            entrypoint_lookup: entrypoint_lookup.map(Arc::new),
            idl_lookup: idl_lookup.map(Arc::new),
        }
    }

    pub fn with_idl_lookup(idl_lookup: IdlLookup) -> Self {
        Self {
            entrypoint_lookup: None,
            idl_lookup: Some(Arc::new(idl_lookup)),
        }
    }

    pub fn set_idl_lookup(&mut self, idl_lookup: IdlLookup) {
        self.idl_lookup = Some(Arc::new(idl_lookup));
    }
}

/// Collects ProgramInfo from 4 sources:
/// 1. Known hardcoded (immutable) program IDLs at Seer initialization.
/// 2. The targets provided by the user at Seer initialization.
/// 3. External programs from the Seer storage lazily during execution.
/// 4. Mainnet programs (IDLs only) from the Solana RPC as fallback lazily during execution.
pub struct ProgramManager {
    inner: Mutex<HashMap<Pubkey, ProgramInfo>>,
    disasm_requests_tx: mpsc::Sender<Pubkey>,
    disasm_status: Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    network_rpc_url: Option<String>,
    idl_fetch_failed: Mutex<HashMap<Pubkey, ()>>,
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

        let disasm_status = Arc::new(Mutex::new(HashMap::new()));
        let (disasm_requests_tx, disasm_requests_rx) = mpsc::channel::<Pubkey>();
        let disasm_output_dir = runtime_dir.join("seer").join("disasm");
        start_disasm_worker(
            disasm_requests_rx,
            disasm_status.clone(),
            network_rpc_url.clone(),
            disasm_output_dir,
        );

        Ok((
            Self {
                inner: Mutex::new(inner),
                disasm_requests_tx,
                disasm_status,
                network_rpc_url,
                idl_fetch_failed: Mutex::new(HashMap::new()),
            },
            warnings,
        ))
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

        match fetch_anchor_idl_lookup_from_rpc(rpc_url, key) {
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

fn fetch_anchor_idl_lookup_from_rpc(rpc_url: &str, program_id: &Pubkey) -> Result<IdlLookup, String> {
    let idl_address = derive_anchor_idl_address(program_id)?;
    let idl_account = fetch_account_info(rpc_url, &idl_address)?;
    let compressed = extract_anchor_idl_compressed_bytes(&idl_account.data)?;
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut idl_json = Vec::new();
    decoder
        .read_to_end(&mut idl_json)
        .map_err(|e| format!("decompress anchor idl for {}: {}", program_id, e))?;
    let idl_json = String::from_utf8(idl_json)
        .map_err(|e| format!("utf8 decode anchor idl for {}: {}", program_id, e))?;
    IdlLookup::new(&idl_json, &format!("rpc:anchor:idl:{}", program_id))
        .map_err(|e| format!("parse anchor idl for {}: {}", program_id, e))
}

fn derive_anchor_idl_address(program_id: &Pubkey) -> Result<Pubkey, String> {
    let program_signer = Pubkey::find_program_address(&[], program_id).0;
    Pubkey::create_with_seed(&program_signer, "anchor:idl", program_id)
        .map_err(|e| format!("derive anchor idl address: {}", e))
}

fn extract_anchor_idl_compressed_bytes(account_data: &[u8]) -> Result<Vec<u8>, String> {
    const DISCRIMINATOR_LEN: usize = 8;
    const AUTHORITY_LEN: usize = 32;
    const DATA_LEN_LEN: usize = 4;
    const HEADER_LEN: usize = DISCRIMINATOR_LEN + AUTHORITY_LEN + DATA_LEN_LEN;

    if account_data.len() < HEADER_LEN {
        return Err(format!(
            "anchor idl account too small: {} bytes (need >= {})",
            account_data.len(),
            HEADER_LEN
        ));
    }
    let data_len_offset = DISCRIMINATOR_LEN + AUTHORITY_LEN;
    let data_len = u32::from_le_bytes(
        account_data[data_len_offset..data_len_offset + DATA_LEN_LEN]
            .try_into()
            .map_err(|_| "anchor idl data length decode failed".to_string())?,
    ) as usize;
    let data_start = HEADER_LEN;
    let data_end = data_start + data_len;
    if account_data.len() < data_end {
        return Err(format!(
            "anchor idl account truncated: {} bytes (need >= {})",
            account_data.len(),
            data_end
        ));
    }
    Ok(account_data[data_start..data_end].to_vec())
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

    let temp_dir = tempdir()
        .map_err(|e| format!("create temporary disasm directory for {}: {}", program_id, e))?;
    let so_path = temp_dir.path().join(format!("{program_id}.so"));
    let elf_bytes = fetch_program_elf_from_rpc(rpc_url, program_id)
        .map_err(|e| format!("fetch ELF for {}: {}", program_id, e))?;
    fs::write(&so_path, &elf_bytes)
        .map_err(|e| format!("write {}: {}", so_path.display(), e))?;

    disasm::disassemble_to_json_chunks(&so_path, disasm_output_dir)
        .map_err(|e| format!("disassemble {}: {}", so_path.display(), e))
}

fn fetch_program_elf_from_rpc(rpc_url: &str, program_id: &Pubkey) -> Result<Vec<u8>, String> {
    const UPGRADEABLE_LOADER_V3_OWNER: &str = "BPFLoaderUpgradeab1e11111111111111111111111";
    const ELF_MAGIC: &[u8; 4] = b"\x7FELF";

    let program_account = fetch_account_info(rpc_url, program_id)?;
    if !program_account.executable {
        return Err(format!("program account {} is not executable", program_id));
    }

    if program_account.owner != UPGRADEABLE_LOADER_V3_OWNER {
        if program_account.data.starts_with(ELF_MAGIC) {
            return Ok(program_account.data);
        }
        return Err(format!(
            "unsupported program loader owner {} for {}; account data is not raw ELF",
            program_account.owner, program_id
        ));
    }

    let program_account_data = program_account.data;
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

    let programdata_account = fetch_account_info(rpc_url, &programdata_address)?;
    let programdata_bytes = programdata_account.data;
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

struct RpcAccountInfo {
    owner: String,
    executable: bool,
    data: Vec<u8>,
}

fn fetch_account_info(rpc_url: &str, account: &Pubkey) -> Result<RpcAccountInfo, String> {
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
    let account_value = response_value
        .get("result")
        .and_then(|v| v.get("value"))
        .ok_or_else(|| "missing result.value in rpc response".to_string())?;
    let owner = account_value
        .get("owner")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing owner in rpc response".to_string())?
        .to_string();
    let executable = account_value
        .get("executable")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "missing executable in rpc response".to_string())?;
    let data_b64 = response_value
        .get("result")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing account data in rpc response".to_string())?;
    let data = BASE64_STANDARD
        .decode(data_b64)
        .map_err(|e| format!("base64 decode account data: {}", e))?;
    Ok(RpcAccountInfo {
        owner,
        executable,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::{fetch_anchor_idl_lookup_from_rpc, fetch_program_elf_from_rpc, DisasmStatus, ProgramManager, ProgramInfo};
    use crate::idl::IdlLookup;
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
        let manager = ProgramManager {
            inner: Mutex::new(HashMap::<Pubkey, ProgramInfo>::new()),
            disasm_requests_tx: tx,
            disasm_status: Arc::new(Mutex::new(HashMap::new())),
            // Guaranteed-to-fail endpoint for fast failure in tests.
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

        // Ensure other control flow continues smoothly after failure.
        manager.queue_disasm_if_needed(program_id);
        let status = manager
            .disasm_status
            .lock()
            .expect("disasm status lock should not be poisoned")
            .get(&program_id)
            .copied();
        assert!(
            matches!(status, Some(DisasmStatus::Pending) | Some(DisasmStatus::Failed)),
            "program manager should keep functioning after IDL RPC failure"
        );
    }
}
