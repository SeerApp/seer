use solana_pubkey::Pubkey;

use crate::account_reads::{
    raw::{RawAccountRead, RawAccountReadKind},
    utils::{collect_sequential_data_reads, merge_contiguous_data_reads},
    view::{ViewAccountRead, ViewAccountReadKind, ViewDataRead},
};

pub fn raw_to_view(raw_reads: Vec<RawAccountRead>) -> Vec<ViewAccountRead> {
    let mut out = vec![];
    let mut i = 0usize;

    while i < raw_reads.len() {
        let raw_read = &raw_reads[i];
        match &raw_read.read_kind {
            RawAccountReadKind::DataLen { len } => {
                out.push(ViewAccountRead {
                    key: raw_read.key,
                    read_kind: ViewAccountReadKind::ReadDataLen {
                        len: *len,
                        step_order: raw_read.step_order,
                    },
                });
                i += 1;
            }
            RawAccountReadKind::Lamports { lamports } => {
                out.push(ViewAccountRead {
                    key: raw_read.key,
                    read_kind: ViewAccountReadKind::ReadLamports {
                        lamports: *lamports,
                        step_order: raw_read.step_order,
                    },
                });
                i += 1;
            }
            RawAccountReadKind::RentEpoch { .. } => {
                i += 1;
            }
            RawAccountReadKind::Key { .. } => {
                if let Some(run) =
                    collect_pubkey_coverage_run(&raw_reads, i, raw_read.key, |kind| match kind {
                        RawAccountReadKind::Key {
                            bytes,
                            offset,
                            bytes_width,
                        } => Some((*offset, *bytes_width, bytes.as_slice())),
                        _ => None,
                    })
                {
                    let key = raw_read.key;
                    out.push(ViewAccountRead {
                        key,
                        read_kind: ViewAccountReadKind::ReadKey {
                            step_order: run[0].step_order,
                            step_order_end: run[run.len() - 1].step_order,
                        },
                    });
                    i += run.len();
                    continue;
                }
                i += 1;
            }
            RawAccountReadKind::Owner { .. } => {
                if let Some(run) =
                    collect_pubkey_coverage_run(&raw_reads, i, raw_read.owner, |kind| match kind {
                        RawAccountReadKind::Owner {
                            bytes,
                            offset,
                            bytes_width,
                        } => Some((*offset, *bytes_width, bytes.as_slice())),
                        _ => None,
                    })
                {
                    let key = raw_read.key;
                    let owner = raw_read.owner;
                    out.push(ViewAccountRead {
                        key,
                        read_kind: ViewAccountReadKind::ReadOwner {
                            owner,
                            step_order: run[0].step_order,
                            step_order_end: run[run.len() - 1].step_order,
                        },
                    });
                    i += run.len();
                    continue;
                }
                i += 1;
            }
            RawAccountReadKind::Data { bytes, .. } => {
                let Some((next_i, reads)) = collect_sequential_data_reads(&raw_reads, i) else {
                    i += 1;
                    continue;
                };
                let reads = merge_contiguous_data_reads(reads);
                let reads = clamp_view_data_reads_to_logical_len(reads, bytes.len());
                if !reads.is_empty() {
                    out.push(ViewAccountRead {
                        key: raw_read.key,
                        read_kind: ViewAccountReadKind::ReadData {
                            bytes: bytes.clone(),
                            reads,
                            parsed: None,
                            parsed_byte_offsets: vec![],
                        },
                    });
                }
                i = next_i;
            }
            _ => {
                i += 1;
            }
        }
    }

    out
}

/// Keeps only the portion of each read that overlaps committed account payload (`bytes.len()`).
/// Drops reads whose offset is past the logical tail, and truncates width when it would extend
/// into the VM realloc runway.
fn clamp_view_data_reads_to_logical_len(
    reads: Vec<ViewDataRead>,
    logical_len: usize,
) -> Vec<ViewDataRead> {
    reads
        .into_iter()
        .filter_map(|mut r| {
            if r.offset > logical_len {
                return None;
            }
            let max_width = logical_len.saturating_sub(r.offset);
            r.bytes_width = r.bytes_width.min(max_width);
            (r.bytes_width > 0).then_some(r)
        })
        .collect()
}

fn collect_pubkey_coverage_run<'a>(
    raw_reads: &'a [RawAccountRead],
    start: usize,
    expected: Pubkey,
    read_range: impl Fn(&'a RawAccountReadKind) -> Option<(usize, usize, &'a [u8])>,
) -> Option<&'a [RawAccountRead]> {
    let key = raw_reads.get(start)?.key;
    let expected = expected.to_bytes();
    let mut assembled = [None; 32];
    let mut covered = 0usize;
    let mut end = start;

    while let Some(read) = raw_reads.get(end) {
        if read.key != key {
            break;
        }
        let Some((offset, bytes_width, bytes)) = read_range(&read.read_kind) else {
            break;
        };

        let start_ix = offset.min(32);
        let end_ix = offset.saturating_add(bytes_width).min(32);
        if end_ix > start_ix {
            let max_span = end_ix - start_ix;
            let span = max_span.min(bytes.len());
            let source = &bytes[..span];
            for (idx, value) in (start_ix..start_ix + span).zip(source.iter().copied()) {
                match assembled[idx] {
                    Some(existing) if existing != value => return None,
                    Some(_) => {}
                    None => {
                        assembled[idx] = Some(value);
                        covered += 1;
                    }
                }
            }
        }

        end += 1;
        if covered == 32 {
            break;
        }
    }

    if covered != 32 {
        return None;
    }
    for i in 0..32 {
        if assembled[i] != Some(expected[i]) {
            return None;
        }
    }

    Some(&raw_reads[start..end])
}
