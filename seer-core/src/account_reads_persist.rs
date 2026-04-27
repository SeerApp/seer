//! Chunked JSON sidecars for aggregated account reads (`*_reads_*` filenames).

use std::collections::BTreeMap;

use crate::{
    account_reads::types::{aggregated_account_read_start_step, TaggedAccountLoadAggregated},
    register_trace::REGISTER_TRACE_CHUNK_SIZE,
    save::{save_account_reads_chunk, save_account_reads_chunk_to_dir},
};

/// Sort by program tree uid then global start step, split into chunks of at most
/// [`REGISTER_TRACE_CHUNK_SIZE`] rows per uid, rolling over uid on CPI boundaries.
pub fn persist_account_reads_chunks(signature: &str, instruction: u8, rows: &[TaggedAccountLoadAggregated]) {
    if rows.is_empty() {
        return;
    }
    let mut sorted: Vec<&TaggedAccountLoadAggregated> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        a.tree_uid.cmp(&b.tree_uid).then(
            aggregated_account_read_start_step(&a.aggregate)
                .cmp(&aggregated_account_read_start_step(&b.aggregate)),
        )
    });

    let mut i = 0usize;
    while i < sorted.len() {
        let chunk_uid = sorted[i].tree_uid;
        let mut chunk_rows: Vec<&TaggedAccountLoadAggregated> = Vec::new();
        while i < sorted.len() && sorted[i].tree_uid == chunk_uid {
            if chunk_rows.len() >= REGISTER_TRACE_CHUNK_SIZE {
                break;
            }
            chunk_rows.push(sorted[i]);
            i += 1;
        }
        if chunk_rows.is_empty() {
            break;
        }
        let min_order = chunk_rows
            .iter()
            .map(|r| aggregated_account_read_start_step(&r.aggregate))
            .min()
            .unwrap_or(0);
        let max_order = chunk_rows
            .iter()
            .map(|r| aggregated_account_read_start_step(&r.aggregate))
            .max()
            .unwrap_or(0);
        let chunk = build_reads_chunk_json(&chunk_rows);
        save_account_reads_chunk(
            signature,
            instruction,
            chunk_uid,
            min_order,
            max_order,
            &chunk,
        );
    }
}

/// Same as [`persist_account_reads_chunks`] but writes into `folder` (used by tests).
pub fn persist_account_reads_chunks_to_dir(
    folder: &std::path::PathBuf,
    signature: &str,
    instruction: u8,
    rows: &[TaggedAccountLoadAggregated],
) {
    if rows.is_empty() {
        return;
    }
    let mut sorted: Vec<&TaggedAccountLoadAggregated> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        a.tree_uid.cmp(&b.tree_uid).then(
            aggregated_account_read_start_step(&a.aggregate)
                .cmp(&aggregated_account_read_start_step(&b.aggregate)),
        )
    });

    let mut i = 0usize;
    while i < sorted.len() {
        let chunk_uid = sorted[i].tree_uid;
        let mut chunk_rows: Vec<&TaggedAccountLoadAggregated> = Vec::new();
        while i < sorted.len() && sorted[i].tree_uid == chunk_uid {
            if chunk_rows.len() >= REGISTER_TRACE_CHUNK_SIZE {
                break;
            }
            chunk_rows.push(sorted[i]);
            i += 1;
        }
        if chunk_rows.is_empty() {
            break;
        }
        let min_order = chunk_rows
            .iter()
            .map(|r| aggregated_account_read_start_step(&r.aggregate))
            .min()
            .unwrap_or(0);
        let max_order = chunk_rows
            .iter()
            .map(|r| aggregated_account_read_start_step(&r.aggregate))
            .max()
            .unwrap_or(0);
        let chunk = build_reads_chunk_json(&chunk_rows);
        save_account_reads_chunk_to_dir(
            folder,
            signature,
            instruction,
            chunk_uid,
            min_order,
            max_order,
            &chunk,
        );
    }
}

fn build_reads_chunk_json(chunk_rows: &[&TaggedAccountLoadAggregated]) -> serde_json::Value {
    // Keep deterministic numeric ordering by start step, then stable tie-breaks.
    let mut entries: Vec<(u64, String, serde_json::Value)> = Vec::with_capacity(chunk_rows.len());
    let mut collisions_per_step: BTreeMap<u64, usize> = BTreeMap::new();
    for r in chunk_rows {
        let start = aggregated_account_read_start_step(&r.aggregate);
        let collision_idx = collisions_per_step.entry(start).or_insert(0usize);
        let key = if *collision_idx == 0 {
            start.to_string()
        } else {
            format!("{start}_{}", *collision_idx)
        };
        *collision_idx += 1;
        let mut v = serde_json::to_value(&r.aggregate).expect("serialize aggregated account read");
        normalize_read_kind_step_fields(&mut v, start);
        entries.push((start, key, v));
    }

    entries.sort_by(|(a_start, a_key, _), (b_start, b_key, _)| {
        a_start.cmp(b_start).then(a_key.cmp(b_key))
    });

    let mut obj = serde_json::Map::with_capacity(entries.len());
    for (_, key, value) in entries {
        obj.insert(key, value);
    }
    serde_json::Value::Object(obj)
}

fn normalize_read_kind_step_fields(aggregate_json: &mut serde_json::Value, start_step: u64) {
    let Some(aggregate_obj) = aggregate_json.as_object_mut() else {
        return;
    };
    aggregate_obj.insert(
        "start_step".to_string(),
        serde_json::Value::Number(serde_json::Number::from(start_step)),
    );
    let Some(read_kind) = aggregate_obj.get_mut("read_kind") else {
        return;
    };
    let Some(read_kind_obj) = read_kind.as_object_mut() else {
        return;
    };

    // `start_step` is already encoded in the outer object key, so we only keep `step_order_end`.
    let had_step_order = read_kind_obj.remove("step_order").is_some();
    let has_step_order_end = read_kind_obj.contains_key("step_order_end");
    if had_step_order && !has_step_order_end {
        read_kind_obj.insert(
            "step_order_end".to_string(),
            serde_json::Value::Number(serde_json::Number::from(start_step)),
        );
    }
}
