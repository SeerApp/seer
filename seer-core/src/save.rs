use std::{
    fs::{self, create_dir_all, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::Local;
use solana_pubkey::Pubkey;

use crate::{
    contexts::register::TransactionRegisterContext,
    get_cwd,
    meta::TxMetadata,
    seer_debug,
    tree::nodes::root::{RootViewChildren, TreeRoot},
};

/// Every literal path segment and standard filename stem/extension used in on-disk Seer output.
/// Each name exists here and only here.
pub(crate) mod disk {
    pub(crate) const ROOT_SEG: &str = "seer";
    pub(crate) const TX: &str = "tx";
    pub(crate) const READS: &str = "reads";
    pub(crate) const REG: &str = "reg";
    pub(crate) const META_JSON: &str = "meta.json";
    pub(crate) const TRACE_JSON: &str = "trace.json";
    pub(crate) const JSON_EXT: &str = "json";
}

// --- Path composition (no string literals for segments outside `disk`) ---

fn seer_root_from_cwd() -> PathBuf {
    let mut path = PathBuf::from(get_cwd());
    path.push(disk::ROOT_SEG);
    create_dir_all(&path).unwrap();
    path
}

/// [`disk::ROOT_SEG`] + `<filename>` (used by generic `save` for loose files under the output root).
fn path_seer_root_file(filename: &str) -> PathBuf {
    seer_root_from_cwd().join(filename)
}

fn path_tx_signature(seer_or_parent: &Path, signature: &str) -> PathBuf {
    seer_or_parent.join(disk::TX).join(signature)
}

/// Cwd-based tree: output root, then [`disk::TX`], then `signature`.
fn path_tx_under_cwd(signature: &str) -> PathBuf {
    path_tx_signature(&seer_root_from_cwd(), signature)
}

/// `…/tx/<signature>/<instruction>/` under the cwd-based Seer tree.
fn path_instruction(signature: &str, instruction: u8) -> PathBuf {
    path_tx_under_cwd(signature).join(instruction.to_string())
}

/// `…/tx/<signature>/<instruction>/<program_pubkey>/`
fn path_program(signature: &str, instruction: u8, program_pubkey: &Pubkey) -> PathBuf {
    path_instruction(signature, instruction).join(program_pubkey.to_string())
}

fn path_meta_json(signature: &str) -> PathBuf {
    path_tx_under_cwd(signature).join(disk::META_JSON)
}

fn path_trace_json(signature: &str, instruction: u8) -> PathBuf {
    path_instruction(signature, instruction).join(disk::TRACE_JSON)
}

fn path_register_dir(signature: &str, instruction: u8, program_pubkey: &Pubkey) -> PathBuf {
    path_program(signature, instruction, program_pubkey).join(disk::REG)
}

fn path_register_chunk_file(
    signature: &str,
    instruction: u8,
    program_pubkey: &Pubkey,
    min_order: u64,
    max_order: u64,
) -> PathBuf {
    path_register_dir(signature, instruction, program_pubkey)
        .join(register_trace_chunk_filename(min_order, max_order))
}

fn register_trace_chunk_filename(min_order: u64, max_order: u64) -> String {
    format!("{}_{}.{}", min_order, max_order, disk::JSON_EXT)
}

fn account_reads_subdir(program_dir: &Path) -> PathBuf {
    program_dir.join(disk::READS)
}

fn register_trace_steps_on_disk(rx: &TransactionRegisterContext) -> (u64, u64) {
    match (rx.trace.first_key_value(), rx.trace.last_key_value()) {
        (Some((&first, _)), Some((&last, _))) => (first, last),
        _ => (rx.min_order, rx.min_order),
    }
}

pub fn save_meta(signature: &String, meta: &TxMetadata) {
    let output_path = path_meta_json(signature);
    seer_debug!("Creating meta file: {}", output_path.to_string_lossy());
    create_dir_all(output_path.parent().expect("meta path should have parent"))
        .expect("create tx output dir");
    let json = serde_json::to_string_pretty(&meta).unwrap();
    let mut file = File::create(output_path).expect("Failed to write file");
    file.write_all(json.as_bytes()).ok().unwrap();
}

pub fn save_trace_tree(
    signature: &String,
    instruction: u8,
    trace_tree: TreeRoot<RootViewChildren>,
) {
    let output_path = path_trace_json(signature, instruction);
    seer_debug!("Creating new file: {}", output_path.to_string_lossy());
    create_dir_all(output_path.parent().expect("trace path should have parent"))
        .expect("create instruction output dir");
    let json = serde_json::to_string_pretty(&trace_tree).unwrap();
    let mut file = File::create(output_path).expect("Failed to write file");
    file.write_all(json.as_bytes()).ok().unwrap();
}

/// Persists `{first_step}_{last_step}.json` where bounds equal the min/max **`trace`** map keys on
/// disk. Using the VM counter (`step_order`) for the suffix was incorrect: CPI and chunk rollover
/// strip an empty trailing frontier row into the next chunk, so the serialized max key can lag the
/// live VM counter; filenames use actual map bounds so adjacent program chunks cannot collide.
pub fn save_register_trace_chunk(
    signature: &str,
    instruction: u8,
    key: &Pubkey,
    trace_chunk: &TransactionRegisterContext,
) {
    let (min_order, max_order) = register_trace_steps_on_disk(trace_chunk);
    let reg_dir = path_register_dir(signature, instruction, key);
    create_dir_all(&reg_dir).expect("create reg output dir");
    save_json_file(
        path_register_chunk_file(signature, instruction, key, min_order, max_order),
        trace_chunk,
        false,
    );
}

pub fn save_account_reads_chunk(
    signature: &str,
    instruction: u8,
    key: &Pubkey,
    min_order: u64,
    max_order: u64,
    chunk: &serde_json::Value,
) {
    let reads_dir = account_reads_dir(signature, instruction, key);
    create_dir_all(&reads_dir).expect("create reads output dir");

    save_json_file(
        reads_dir.join(register_trace_chunk_filename(min_order, max_order)),
        chunk,
        true,
    );
}

/// Instruction-scoped directory under the cwd-based output tree (trace, per-program [`disk::READS`], [`disk::REG`], …).
pub fn instruction_dir(signature: &str, instruction: u8) -> PathBuf {
    path_instruction(signature, instruction)
}

/// Account-reads chunk directory (same layout as [`save_account_reads_chunk`]).
pub fn account_reads_dir(signature: &str, instruction: u8, program_pubkey: &Pubkey) -> PathBuf {
    account_reads_subdir(&path_program(signature, instruction, program_pubkey))
}

/// Parses the `{min}_{max}` stem used by [`save_account_reads_chunk`] filenames (without `.json`).
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
/// Paths and layout match [`save_account_reads_chunk`].
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

/// Arbitrary output folder (e.g. test fixtures): same relative shape as [`path_instruction`], rooted at `folder`.
fn path_instruction_under_folder(folder: &Path, signature: &str, instruction: u8) -> PathBuf {
    path_tx_signature(folder, signature).join(instruction.to_string())
}

/// Writes the trace tree JSON file under `folder` (same relative layout as [`save_trace_tree`] under the cwd output root).
pub fn save_trace_tree_to_dir(
    folder: &PathBuf,
    signature: &String,
    instruction: u8,
    trace_tree: TreeRoot<RootViewChildren>,
) {
    let base = path_instruction_under_folder(folder, signature, instruction);
    create_dir_all(&base).expect("create trace tree output dir");
    let output_path = base.join(disk::TRACE_JSON);
    let json = serde_json::to_string_pretty(&trace_tree).expect("serialize trace tree");
    let mut file = File::create(output_path).expect("Failed to write trace tree file");
    file.write_all(json.as_bytes())
        .expect("write trace tree bytes");
}

pub fn load_trace_tree(
    folder: &PathBuf,
    instruction: u8,
    signature: &String,
) -> TreeRoot<RootViewChildren> {
    let input_path =
        path_instruction_under_folder(folder, signature, instruction).join(disk::TRACE_JSON);
    let mut file = File::open(input_path).expect("Failed to open file");

    let mut json = String::new();
    file.read_to_string(&mut json).expect("Failed to read file");

    serde_json::from_str(&json).expect("Failed to deserialize tree")
}

fn save_json_file<T: serde::Serialize>(output_path: PathBuf, value: &T, overwrite: bool) {
    if overwrite || !output_path.exists() {
        seer_debug!("Creating new file: {}", output_path.to_string_lossy());
        let json = serde_json::to_string_pretty(value).expect("serialize json file");
        let mut file = File::create(output_path).expect("Failed to write file");
        file.write_all(json.as_bytes()).ok().unwrap();
    }
}

pub fn save(data: String, filename: String, extension: &str, timestamp: bool) {
    let mut final_filename = filename;

    if timestamp {
        final_filename = add_timestamp(final_filename);
    }

    let output_path = path_seer_root_file(&format!("{}.{}", final_filename, extension));

    if !output_path.exists() {
        seer_debug!("Creating new file: {}", output_path.to_string_lossy());
        let mut file = File::create(output_path).expect("Failed to write file");
        file.write_all(data.as_bytes()).ok().unwrap();
    }
}

fn add_timestamp(filename: String) -> String {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");

    format! {"{}_{}", filename, timestamp}
}

#[cfg(test)]
mod tests {
    use super::account_reads_chunk_step_bounds_from_file_stem;

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
