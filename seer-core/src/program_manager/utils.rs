//! Purpose: provide program manager functional utilities.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
};

use solana_pubkey::Pubkey;
use tempfile::tempdir;

use crate::{artifacts::AtomicFileWriter, seer_debug, seer_warn, target_reader::Target};

use super::types::DisasmStatus;

pub(super) fn preload_local_disasm_for_targets(
    targets: &HashMap<Pubkey, Target>,
    disasm_status: &Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    programs_output_dir: &Path,
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

pub(super) fn start_disasm_worker(
    disasm_requests_rx: mpsc::Receiver<Pubkey>,
    disasm_status: Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    programs_output_dir: PathBuf,
    file_writer: Arc<Mutex<AtomicFileWriter>>,
) {
    thread::spawn(move || {
        for program_id in disasm_requests_rx {
            let result = {
                let _file_writer = file_writer
                    .lock()
                    .expect("file writer lock should not be poisoned");
                let _ = &programs_output_dir;
                Err::<(), String>("network RPC URL not configured".to_string())
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

fn disassemble_local_elf_for_program(
    program_id: &Pubkey,
    local_executable_path: &Path,
    programs_output_dir: &Path,
    file_writer: &AtomicFileWriter,
) -> Result<(), String> {
    ensure_programs_output_dir(programs_output_dir)?;
    let elf_bytes = fs::read(local_executable_path).map_err(|e| {
        format!(
            "read local executable {}: {}",
            local_executable_path.display(),
            e
        )
    })?;
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
    programs_output_dir: &Path,
    file_writer: &AtomicFileWriter,
) -> Result<(), String> {
    let temp_dir = tempdir().map_err(|e| {
        format!(
            "create temporary disasm directory for {}: {}",
            program_id, e
        )
    })?;
    let so_path = temp_dir.path().join(format!("{program_id}.so"));
    file_writer.replace_bytes(&so_path, elf_bytes);

    #[cfg(feature = "disasm")]
    {
        let stats = disasm::disassemble_to_json_chunks(&so_path, programs_output_dir)
            .map_err(|e| format!("disassemble {}: {}", so_path.display(), e))?;
        seer_debug!(
            "disasm {} insns={} peak_rss_bytes={:?}",
            program_id,
            stats.insn_count,
            stats.peak_rss_bytes
        );
    }
    #[cfg(not(feature = "disasm"))]
    {
        let _ = programs_output_dir;
        seer_debug!("disasm skipped (feature off) for {}", program_id);
    }
    Ok(())
}

fn ensure_programs_output_dir(programs_output_dir: &Path) -> Result<(), String> {
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
