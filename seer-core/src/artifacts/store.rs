//! Typed Seer artifact persistence on [`super::writer::AtomicFileWriter`].

use std::{fs::create_dir_all, path::Path};

use chrono::Local;
use solana_pubkey::Pubkey;

use crate::{
    contexts::register::TransactionRegisterContext,
    meta::TxMetadata,
    seer_debug,
    tree::nodes::root::{RootViewChildren, TreeRoot},
};

use super::{
    layout::{
        self,
        account_reads_dir, path_instruction_under_folder, path_meta_json, path_register_chunk_file,
        path_register_dir, path_seer_root_file, path_trace_json, register_trace_chunk_filename,
        register_trace_steps_on_disk,
    },
    writer::AtomicFileWriter,
};

impl AtomicFileWriter {
    pub fn save_meta(&self, signature: &str, meta: &TxMetadata) {
        let output_path = path_meta_json(signature);
        seer_debug!("Creating meta file: {}", output_path.to_string_lossy());
        self.write_json(&output_path, meta, true);
    }

    pub fn save_trace_tree(
        &self,
        signature: &str,
        instruction: u8,
        trace_tree: TreeRoot<RootViewChildren>,
    ) {
        let output_path = path_trace_json(signature, instruction);
        seer_debug!("Creating new file: {}", output_path.to_string_lossy());
        self.write_json(&output_path, &trace_tree, true);
    }

    /// Persists `{first_step}_{last_step}.json` where bounds equal the min/max **`trace`** map keys on
    /// disk. Using the VM counter (`step_order`) for the suffix was incorrect: CPI and chunk rollover
    /// strip an empty trailing frontier row into the next chunk, so the serialized max key can lag the
    /// live VM counter; filenames use actual map bounds so adjacent program chunks cannot collide.
    pub fn save_register_trace_chunk(
        &self,
        signature: &str,
        instruction: u8,
        key: &Pubkey,
        trace_chunk: &TransactionRegisterContext,
    ) {
        let (min_order, max_order) = register_trace_steps_on_disk(trace_chunk);
        let reg_dir = path_register_dir(signature, instruction, key);
        create_dir_all(&reg_dir).expect("create reg output dir");
        self.write_json(
            &path_register_chunk_file(signature, instruction, key, min_order, max_order),
            trace_chunk,
            false,
        );
    }

    pub fn save_account_reads_chunk(
        &self,
        signature: &str,
        instruction: u8,
        key: &Pubkey,
        min_order: u64,
        max_order: u64,
        chunk: &serde_json::Value,
    ) {
        let reads_dir = account_reads_dir(signature, instruction, key);
        create_dir_all(&reads_dir).expect("create reads output dir");
        self.write_json(
            &reads_dir.join(register_trace_chunk_filename(min_order, max_order)),
            chunk,
            true,
        );
    }

    /// Writes the trace tree JSON file under `folder` (same relative layout as [`Self::save_trace_tree`]
    /// under the cwd output root).
    pub fn save_trace_tree_to_dir(
        &self,
        folder: &Path,
        signature: &str,
        instruction: u8,
        trace_tree: TreeRoot<RootViewChildren>,
    ) {
        let base = path_instruction_under_folder(folder, signature, instruction);
        create_dir_all(&base).expect("create trace tree output dir");
        let output_path = base.join(layout::disk::TRACE_JSON);
        self.write_json(&output_path, &trace_tree, true);
    }

    pub fn save_loose_file(
        &self,
        data: &str,
        filename: &str,
        extension: &str,
        timestamp: bool,
        overwrite: bool,
    ) {
        let mut final_filename = filename.to_string();
        if timestamp {
            final_filename = add_timestamp(final_filename);
        }
        let output_path = path_seer_root_file(&format!("{}.{}", final_filename, extension));
        seer_debug!("Creating new file: {}", output_path.to_string_lossy());
        self.write_bytes(&output_path, data.as_bytes(), overwrite);
    }

    pub fn save_runbooks(&self, runtime_dir: &Path, txtx: String, main: String) {
        let txtx_yml_path = runtime_dir.join("txtx.yml");
        let runbook_dir = runtime_dir.join("runbooks").join("deployment");
        let main_tx_path = runbook_dir.join("main.tx");

        create_dir_all(&runbook_dir)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", runbook_dir.display()));
        self.write_bytes(&txtx_yml_path, txtx.as_bytes(), true);
        self.write_bytes(&main_tx_path, main.as_bytes(), true);
    }
}

fn add_timestamp(filename: String) -> String {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    format!("{filename}_{timestamp}")
}
