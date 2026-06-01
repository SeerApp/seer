//! Purpose: provide program manager functional utilities.

use std::{
    collections::HashMap,
    fs,
    io::Read,
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
    artifacts::AtomicFileWriter,
    idl::IdlLookup,
    seer_debug,
    seer_warn,
    target_reader::Target,
};

use super::types::{DisasmStatus, RpcAccountInfo};

pub(super) fn preload_local_disasm_for_targets(
    targets: &HashMap<Pubkey, Target>,
    disasm_status: &Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    programs_output_dir: &PathBuf,
    file_writer: &AtomicFileWriter,
) {
    if let Err(err) = ensure_programs_output_dir(programs_output_dir) {
        seer_warn!(
            "failed to create local programs output dir {}: {}",
            programs_output_dir.display(),
            err
        );
        return;
    }

    for (program_id, target) in targets {
        let Some(local_executable_path) = target.executable.as_ref() else {
            continue;
        };

        let result = disassemble_local_elf_for_program(
            program_id,
            local_executable_path,
            programs_output_dir,
            file_writer,
        );
        set_disasm_status(disasm_status, *program_id, result.is_ok());
        if let Err(err) = result {
            seer_warn!(
                "local disasm preload failed for {} ({}): {}",
                program_id,
                local_executable_path.display(),
                err
            );
        } else {
            seer_debug!(
                "local disasm preload completed for {} ({})",
                program_id,
                local_executable_path.display()
            );
        }
    }
}

pub(super) fn fetch_anchor_idl_lookup_from_rpc(
    rpc_url: &str,
    program_id: &Pubkey,
) -> Result<IdlLookup, String> {
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

pub(super) fn start_disasm_worker(
    disasm_requests_rx: mpsc::Receiver<Pubkey>,
    disasm_status: Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    network_rpc_url: Option<String>,
    programs_output_dir: PathBuf,
    file_writer: Arc<Mutex<AtomicFileWriter>>,
) {
    thread::spawn(move || {
        for program_id in disasm_requests_rx {
            let result = {
                let file_writer = file_writer
                    .lock()
                    .expect("file writer lock should not be poisoned");
                maybe_download_and_disassemble(
                    &program_id,
                    network_rpc_url.as_deref(),
                    &programs_output_dir,
                    &file_writer,
                )
            };
            set_disasm_status(&disasm_status, program_id, result.is_ok());
            if let Err(err) = result {
                seer_warn!("background disasm failed for {}: {}", program_id, err);
            } else {
                seer_debug!("background disasm completed for {}", program_id);
            }
        }
    });
}

pub(super) fn fetch_program_elf_from_rpc(
    rpc_url: &str,
    program_id: &Pubkey,
) -> Result<Vec<u8>, String> {
    const UPGRADEABLE_LOADER_V3_OWNER: &str = "BPFLoaderUpgradeab1e11111111111111111111111";
    const ELF_MAGIC: &[u8; 4] = b"\x7FELF";

    if program_id == &Pubkey::default() {
        return Ok(Vec::new());
    }

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

fn maybe_download_and_disassemble(
    program_id: &Pubkey,
    network_rpc_url: Option<&str>,
    programs_output_dir: &PathBuf,
    file_writer: &AtomicFileWriter,
) -> Result<(), String> {
    let Some(rpc_url) = network_rpc_url else {
        return Err("network RPC URL not configured".to_string());
    };
    ensure_programs_output_dir(programs_output_dir)?;
    let elf_bytes = fetch_program_elf_from_rpc(rpc_url, program_id)
        .map_err(|e| format!("fetch ELF for {}: {}", program_id, e))?;
    if elf_bytes.is_empty() {
        return Ok(());
    }
    disassemble_elf_bytes_for_program(program_id, &elf_bytes, programs_output_dir, file_writer)
}

fn disassemble_local_elf_for_program(
    program_id: &Pubkey,
    local_executable_path: &PathBuf,
    programs_output_dir: &PathBuf,
    file_writer: &AtomicFileWriter,
) -> Result<(), String> {
    ensure_programs_output_dir(programs_output_dir)?;
    let elf_bytes = fs::read(local_executable_path)
        .map_err(|e| format!("read local executable {}: {}", local_executable_path.display(), e))?;
    if elf_bytes.is_empty() {
        return Err(format!(
            "local executable {} is empty",
            local_executable_path.display()
        ));
    }
    disassemble_elf_bytes_for_program(program_id, &elf_bytes, programs_output_dir, file_writer)
}

fn disassemble_elf_bytes_for_program(
    program_id: &Pubkey,
    elf_bytes: &[u8],
    programs_output_dir: &PathBuf,
    file_writer: &AtomicFileWriter,
) -> Result<(), String> {
    let temp_dir = tempdir().map_err(|e| {
        format!(
            "create temporary disasm directory for {}: {}",
            program_id, e
        )
    })?;
    let so_path = temp_dir.path().join(format!("{program_id}.so"));
    file_writer.write_bytes(&so_path, elf_bytes, true);

    disasm::disassemble_to_json_chunks(&so_path, programs_output_dir)
        .map_err(|e| format!("disassemble {}: {}", so_path.display(), e))
}

fn ensure_programs_output_dir(programs_output_dir: &PathBuf) -> Result<(), String> {
    fs::create_dir_all(programs_output_dir).map_err(|e| {
        format!(
            "create programs dir {}: {}",
            programs_output_dir.display(),
            e
        )
    })
}

fn set_disasm_status(
    disasm_status: &Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    program_id: Pubkey,
    succeeded: bool,
) {
    let new_status = if succeeded {
        DisasmStatus::Succeeded
    } else {
        DisasmStatus::Failed
    };
    if let Ok(mut guard) = disasm_status.lock() {
        guard.insert(program_id, new_status);
    }
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
