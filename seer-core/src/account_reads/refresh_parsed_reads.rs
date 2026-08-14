use std::{collections::HashMap, fs};

use solana_pubkey::Pubkey;

use crate::{
    account_reads::{
        utils::build_view_reads_chunk_json,
        view::{ViewAccountRead, ViewAccountReadKind},
    },
    program_manager::types::GlobalProgramContext,
    artifacts::{
        layout::{account_reads_chunk_paths, account_reads_chunk_step_bounds_from_file_stem},
        AtomicFileWriter,
    },
};

pub fn load_account_reads_chunk_rows(json: &serde_json::Value) -> Option<Vec<(u64, ViewAccountRead)>> {
    let obj = json.as_object()?;
    let mut rows: Vec<(u64, ViewAccountRead)> = Vec::with_capacity(obj.len());
    for (step_key, row_val) in obj {
        let step: u64 = step_key.parse().ok()?;
        let row: ViewAccountRead = serde_json::from_value(row_val.clone()).ok()?;
        rows.push((step, row));
    }
    rows.sort_by_key(|(s, _)| *s);
    Some(rows)
}

fn try_enrich_read_data(
    row: &mut ViewAccountRead,
    receiver_by_account: &HashMap<Pubkey, Pubkey>,
    program_context: &GlobalProgramContext,
) -> bool {
    let ViewAccountReadKind::ReadData {
        bytes,
        parsed,
        parsed_byte_offsets,
        ..
    } = &mut row.read_kind
    else {
        return false;
    };

    if parsed.is_some() || !parsed_byte_offsets.is_empty() {
        return false;
    }

    let Some(idl_receiver) = receiver_by_account.get(&row.key) else {
        return false;
    };

    let Some(idl) = program_context.get_idl_lookup_cached(idl_receiver) else {
        return false;
    };

    let Some(result) = GlobalProgramContext::parse_account_with_idl(&idl, bytes.as_slice()) else {
        return false;
    };

    *parsed = Some(result.parsed);
    *parsed_byte_offsets = result.parsed_byte_offsets;
    true
}

/// Applies IDL parsing to `ReadData` rows using the same rules as the post-instruction refresh pass.
/// Returns how many rows were updated.
pub fn enrich_chunk_rows(
    rows: &mut [(u64, ViewAccountRead)],
    receiver_by_account: &HashMap<Pubkey, Pubkey>,
    program_context: &GlobalProgramContext,
) -> usize {
    let mut n = 0usize;
    for (_, row) in rows.iter_mut() {
        if try_enrich_read_data(row, receiver_by_account, program_context) {
            n += 1;
        }
    }
    n
}

fn refresh_reads_chunk_file(
    chunk_path: &std::path::Path,
    program_pubkey: Pubkey,
    signature: &str,
    instruction: u8,
    receiver_by_account: &HashMap<Pubkey, Pubkey>,
    program_context: &GlobalProgramContext,
    file_writer: &AtomicFileWriter,
) -> Option<()> {
    let stem = chunk_path.file_stem()?.to_str()?;
    let (min_order, max_order) = account_reads_chunk_step_bounds_from_file_stem(stem)?;

    let raw = fs::read_to_string(chunk_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let Some(mut rows) = load_account_reads_chunk_rows(&json) else {
        return Some(());
    };

    if enrich_chunk_rows(&mut rows, receiver_by_account, program_context) == 0 {
        return Some(());
    }

    let refs: Vec<&ViewAccountRead> = rows.iter().map(|(_, r)| r).collect();
    let chunk = build_view_reads_chunk_json(&refs);
    file_writer.save_account_reads_chunk(
        signature,
        instruction,
        &program_pubkey,
        min_order,
        max_order,
        &chunk,
    );

    Some(())
}

/// Re-parses on-disk account read chunks for this instruction using `receiver_by_account` from the
/// finalized trace tree. Only fills `ReadData` when `parsed` is absent and `parsed_byte_offsets` is
/// empty; IDL-only (no sysvar pass). Overwrites chunk JSON via [`AtomicFileWriter::save_account_reads_chunk`].
///
/// All discovery of chunk paths uses [`crate::artifacts::layout::account_reads_chunk_paths`].
pub fn refresh_account_reads_parsed_for_instruction(
    receiver_by_account: &HashMap<Pubkey, Pubkey>,
    signature: &str,
    instruction: u8,
    program_context: &GlobalProgramContext,
    file_writer: &AtomicFileWriter,
) {
    for (program_pubkey, chunk_path) in account_reads_chunk_paths(signature, instruction) {
        let _ = refresh_reads_chunk_file(
            &chunk_path,
            program_pubkey,
            signature,
            instruction,
            receiver_by_account,
            program_context,
            file_writer,
        );
    }
}
