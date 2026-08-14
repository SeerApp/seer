//! Path layout, atomic-write staging paths, and read-side helpers (no writes).

use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use solana_pubkey::Pubkey;

use crate::{
    contexts::register::TransactionRegisterContext,
    get_cwd,
    tree::nodes::root::{RootViewChildren, TreeRoot},
};

/// Literal path segments and standard filename stems/extensions for on-disk Seer output.
pub mod disk {
    pub const ROOT_SEG: &str = "seer";
    pub const TX: &str = "tx";
    pub const READS: &str = "reads";
    pub const REG: &str = "reg";
    pub const META_JSON: &str = "meta.json";
    pub const FAILURE_JSON: &str = "failure.json";
    pub const TRACE_JSON: &str = "trace.json";
    pub const JSON_EXT: &str = "json";
    /// In-progress atomic write beside the final file (`meta.json.tmp` → `meta.json`).
    pub const STAGING_TMP_SUFFIX: &str = ".tmp";
}

/// `meta.json` → `meta.json.tmp` in the same directory as `dest`.
pub(crate) fn staging_path_for(dest: &Path) -> PathBuf {
    let parent = dest
        .parent()
        .expect("destination path must have a parent directory for atomic write");
    let file_name = dest
        .file_name()
        .expect("destination path must have a file name for atomic write");
    let mut staging_name = file_name.to_os_string();
    staging_name.push(disk::STAGING_TMP_SUFFIX);
    parent.join(staging_name)
}

/// True when `path` is an in-progress atomic write (`*.tmp` beside its final name).
pub fn is_staging_tmp_path(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.ends_with(disk::STAGING_TMP_SUFFIX))
}

pub(crate) fn seer_root_from_cwd() -> PathBuf {
    let mut path = PathBuf::from(get_cwd());
    path.push(disk::ROOT_SEG);
    fs::create_dir_all(&path).unwrap();
    path
}

pub(crate) fn path_seer_root_file(filename: &str) -> PathBuf {
    seer_root_from_cwd().join(filename)
}

pub(crate) fn path_tx_signature(seer_or_parent: &Path, signature: &str) -> PathBuf {
    seer_or_parent.join(disk::TX).join(signature)
}

fn path_tx_under_cwd(signature: &str) -> PathBuf {
    path_tx_signature(&seer_root_from_cwd(), signature)
}

pub(crate) fn path_instruction(signature: &str, instruction: u8) -> PathBuf {
    path_tx_under_cwd(signature).join(instruction.to_string())
}

pub(crate) fn path_program(signature: &str, instruction: u8, program_pubkey: &Pubkey) -> PathBuf {
    path_instruction(signature, instruction)
        .join(program_pubkey.to_string())
}

pub(crate) fn path_meta_json(signature: &str) -> PathBuf {
    path_tx_under_cwd(signature).join(disk::META_JSON)
}

pub(crate) fn path_tx_failure_json(signature: &str) -> PathBuf {
    path_tx_under_cwd(signature).join(disk::FAILURE_JSON)
}

pub(crate) fn path_run_failure_json() -> PathBuf {
    path_seer_root_file(disk::FAILURE_JSON)
}

pub(crate) fn path_trace_json(signature: &str, instruction: u8) -> PathBuf {
    path_instruction(signature, instruction).join(disk::TRACE_JSON)
}

pub(crate) fn path_register_dir(signature: &str, instruction: u8, program_pubkey: &Pubkey) -> PathBuf {
    path_program(signature, instruction, program_pubkey).join(disk::REG)
}

pub(crate) fn path_register_chunk_file(
    signature: &str,
    instruction: u8,
    program_pubkey: &Pubkey,
    min_order: u64,
    max_order: u64,
) -> PathBuf {
    path_register_dir(signature, instruction, program_pubkey)
        .join(register_trace_chunk_filename(min_order, max_order))
}

pub(crate) fn register_trace_chunk_filename(min_order: u64, max_order: u64) -> String {
    format!("{}_{}.{}", min_order, max_order, disk::JSON_EXT)
}

fn account_reads_subdir(program_dir: &Path) -> PathBuf {
    program_dir.join(disk::READS)
}

pub(crate) fn register_trace_steps_on_disk(rx: &TransactionRegisterContext) -> (u64, u64) {
    match (rx.trace.first_key_value(), rx.trace.last_key_value()) {
        (Some((&first, _)), Some((&last, _))) => (first, last),
        _ => (rx.min_order, rx.min_order),
    }
}

/// Instruction-scoped directory under the cwd-based output tree (trace, per-program [`disk::READS`], [`disk::REG`], …).
pub fn instruction_dir(signature: &str, instruction: u8) -> PathBuf {
    path_instruction(signature, instruction)
}

/// Account-reads chunk directory (same layout as account-reads chunk writes on [`super::writer::AtomicFileWriter`]).
pub fn account_reads_dir(signature: &str, instruction: u8, program_pubkey: &Pubkey) -> PathBuf {
    account_reads_subdir(&path_program(signature, instruction, program_pubkey))
}

/// Parses the `{min}_{max}` stem used by account-reads chunk filenames (without `.json`).
pub fn account_reads_chunk_step_bounds_from_file_stem(stem: &str) -> Option<(u64, u64)> {
    let mut parts = stem.split('_');
    let min_s = parts.next()?;
    let max_s = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((min_s.parse().ok()?, max_s.parse().ok()?))
}

/// Every on-disk account-reads chunk JSON for this instruction: `(program_pubkey, chunk_path)`.
pub fn account_reads_chunk_paths(signature: &str, instruction: u8) -> Vec<(Pubkey, PathBuf)> {
    let mut out = Vec::new();
    let ix_dir = path_instruction(signature, instruction);
    let Ok(entries) = fs::read_dir(&ix_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let program_dir = entry.path();
        if !program_dir.is_dir() {
            continue;
        }
        let Some(dir_name) = program_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(program_pubkey) = dir_name.parse::<Pubkey>() else {
            continue;
        };
        let reads_dir = account_reads_subdir(&program_dir);
        if !reads_dir.is_dir() {
            continue;
        }
        let Ok(read_files) = fs::read_dir(&reads_dir) else {
            continue;
        };
        for rf in read_files.flatten() {
            let chunk_path = rf.path();
            if chunk_path.extension().and_then(|e| e.to_str()) != Some(disk::JSON_EXT) {
                continue;
            }
            out.push((program_pubkey, chunk_path));
        }
    }
    out
}

pub(crate) fn path_instruction_under_folder(folder: &Path, signature: &str, instruction: u8) -> PathBuf {
    path_tx_signature(folder, signature).join(instruction.to_string())
}

pub fn load_trace_tree(
    folder: &Path,
    instruction: u8,
    signature: &str,
) -> TreeRoot<RootViewChildren> {
    let input_path =
        path_instruction_under_folder(folder, signature, instruction).join(disk::TRACE_JSON);
    let mut file = File::open(input_path).expect("Failed to open file");

    let mut json = String::new();
    file.read_to_string(&mut json).expect("Failed to read file");

    serde_json::from_str(&json).expect("Failed to deserialize tree")
}

#[cfg(test)]
mod tests {
    use super::{
        account_reads_chunk_step_bounds_from_file_stem, disk, is_staging_tmp_path,
        staging_path_for,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn staging_path_appends_tmp_suffix() {
        let dest = PathBuf::from("/work/seer/tx/sig/meta.json");
        assert_eq!(
            staging_path_for(&dest),
            PathBuf::from("/work/seer/tx/sig/meta.json.tmp")
        );
        assert_eq!(disk::STAGING_TMP_SUFFIX, ".tmp");
    }

    #[test]
    fn is_staging_tmp_path_matches_suffix() {
        assert!(is_staging_tmp_path(Path::new("meta.json.tmp")));
        assert!(is_staging_tmp_path(Path::new("0_42.json.tmp")));
        assert!(!is_staging_tmp_path(Path::new("meta.json")));
        assert!(!is_staging_tmp_path(Path::new("file.tmporarily")));
    }

    #[test]
    fn account_reads_chunk_stem_round_trip_bounds() {
        assert_eq!(
            account_reads_chunk_step_bounds_from_file_stem("0_42"),
            Some((0, 42))
        );
        assert_eq!(
            account_reads_chunk_step_bounds_from_file_stem("245_4056"),
            Some((245, 4056))
        );
        assert_eq!(account_reads_chunk_step_bounds_from_file_stem("bad"), None);
        assert_eq!(
            account_reads_chunk_step_bounds_from_file_stem("1_2_3"),
            None
        );
    }
}
