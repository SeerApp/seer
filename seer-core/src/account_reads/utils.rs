use std::collections::BTreeMap;

use solana_pubkey::Pubkey;

use crate::{
    account_reads::{
        raw::{RawAccountRead, RawAccountReadKind},
        view::{ViewAccountRead, ViewDataRead},
    },
    register_trace::REGISTER_TRACE_CHUNK_SIZE,
    artifacts::AtomicFileWriter,
};

pub fn collect_sequential_data_reads(
    raw_reads: &[RawAccountRead],
    start: usize,
) -> Option<(usize, Vec<ViewDataRead>)> {
    let first = raw_reads.get(start)?;
    let key = first.key;
    let mut i = start;
    let mut reads = Vec::new();

    while let Some(raw_read) = raw_reads.get(i) {
        if raw_read.key != key {
            break;
        }
        let RawAccountReadKind::Data {
            offset,
            bytes_width,
            ..
        } = &raw_read.read_kind
        else {
            break;
        };
        reads.push(ViewDataRead {
            offset: *offset,
            bytes_width: *bytes_width,
            step_order: raw_read.step_order,
            step_order_end: raw_read.step_order,
        });
        i += 1;
    }

    if reads.is_empty() {
        return None;
    }
    Some((i, reads))
}

pub fn merge_contiguous_data_reads(mut reads: Vec<ViewDataRead>) -> Vec<ViewDataRead> {
    if reads.is_empty() {
        return reads;
    }
    reads.sort_by_key(|r| r.offset);
    let mut merged: Vec<ViewDataRead> = Vec::with_capacity(reads.len());

    for read in reads {
        let Some(last) = merged.last_mut() else {
            merged.push(read);
            continue;
        };

        let last_end = last.offset.saturating_add(last.bytes_width);
        let read_end = read.offset.saturating_add(read.bytes_width);

        if read.offset <= last_end {
            let merged_end = last_end.max(read_end);
            last.bytes_width = merged_end.saturating_sub(last.offset);
            last.step_order = last.step_order.min(read.step_order);
            last.step_order_end = last.step_order_end.max(read.step_order_end);
        } else {
            merged.push(read);
        }
    }

    merged
}

pub fn build_view_reads_chunk_json(chunk_rows: &[&ViewAccountRead]) -> serde_json::Value {
    let mut entries: BTreeMap<u64, serde_json::Value> = BTreeMap::new();
    for r in chunk_rows {
        let start = r.view_read_start_step();
        entries
            .entry(start)
            .or_insert_with(|| serde_json::to_value(r).expect("serialize view account read"));
    }

    let mut obj = serde_json::Map::with_capacity(entries.len());
    for (start, value) in entries {
        obj.insert(start.to_string(), value);
    }
    serde_json::Value::Object(obj)
}

pub fn save_view_account_reads_chunks(
    reads: &[ViewAccountRead],
    signature: &str,
    instruction: u8,
    key: &Pubkey,
    file_writer: &AtomicFileWriter,
) {
    if reads.is_empty() {
        return;
    }
    let mut sorted: Vec<&ViewAccountRead> = reads.iter().collect();
    sorted.sort_by_key(|r| r.view_read_start_step());
    let mut i = 0usize;
    while i < sorted.len() {
        let mut chunk_rows: Vec<&ViewAccountRead> = Vec::new();
        while i < sorted.len() {
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
            .map(|r| r.view_read_start_step())
            .min()
            .unwrap_or(0);
        let max_order = chunk_rows
            .iter()
            .map(|r| r.view_read_start_step())
            .max()
            .unwrap_or(0);
        let chunk = build_view_reads_chunk_json(&chunk_rows);
        file_writer.save_account_reads_chunk(
            signature,
            instruction,
            key,
            min_order,
            max_order,
            &chunk,
        );
    }
}
