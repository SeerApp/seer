//! Register-chunk JSON: snapshot + sparse deltas → [`Step`]s.
//!
//! First snapshot wins; later snapshots are ignored. Same schema as persist_reg blobs.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::step::Step;

/// One program invocation's register timeline, after chunk files are merged.
pub(crate) struct RegisterTrace {
    snapshot: [u64; 11],
    events: BTreeMap<u64, TraceEvent>,
}

impl RegisterTrace {
    /// Merge chunks by order. Only the first chunk's snapshot seeds registers.
    pub(crate) fn from_chunks(chunks: &[RegisterTraceChunk]) -> Result<Self> {
        if chunks.is_empty() {
            return Ok(Self {
                snapshot: [0; 11],
                events: BTreeMap::new(),
            });
        }
        let snapshot = snapshot_regs(&chunks[0].snapshot)?;
        let mut events = BTreeMap::new();
        for chunk in chunks {
            for (order_s, entry) in &chunk.trace {
                let order: u64 = order_s.parse()?;
                let updates = match &entry.reg {
                    Some(delta) => parse_updates(delta)?,
                    None => Vec::new(),
                };
                events.insert(
                    order,
                    TraceEvent {
                        pc: entry.pc,
                        updates,
                    },
                );
            }
        }
        Ok(Self { snapshot, events })
    }

    pub(crate) fn pcs(&self) -> Vec<u64> {
        self.events.values().map(|e| e.pc).collect()
    }

    pub(crate) fn into_steps(self, disasm: &BTreeMap<u64, String>) -> Vec<Step> {
        let mut regs = self.snapshot;
        let events: Vec<(u64, TraceEvent)> = self.events.into_iter().collect();
        let mut steps = Vec::with_capacity(events.len());
        for (idx, (order, event)) in events.iter().enumerate() {
            let pre_regs = regs;
            apply_updates(&mut regs, &event.updates);
            let next_pc = events.get(idx + 1).map(|(_, e)| e.pc);
            steps.push(Step {
                order: *order,
                pc: event.pc,
                next_pc,
                disasm: disasm.get(&event.pc).cloned().unwrap_or_default(),
                pre_regs,
                post_regs: regs,
            });
        }
        steps
    }
}

struct TraceEvent {
    pc: u64,
    updates: Vec<(usize, u64)>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RegisterTraceChunk {
    snapshot: RegisterSnapshot,
    trace: BTreeMap<String, RegisterTraceEntry>,
}

#[derive(Debug, Deserialize)]
struct RegisterSnapshot {
    reg: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RegisterTraceEntry {
    pc: u64,
    #[serde(default)]
    reg: Option<BTreeMap<String, String>>,
}

fn parse_u64_str(s: &str) -> Result<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).with_context(|| format!("parse hex {s}"))
    } else {
        s.parse::<u64>().with_context(|| format!("parse u64 {s}"))
    }
}

fn parse_updates(delta: &BTreeMap<String, String>) -> Result<Vec<(usize, u64)>> {
    let mut out = Vec::new();
    for (k, v) in delta {
        let idx: usize = k.parse().with_context(|| format!("reg index {k}"))?;
        if idx >= 11 {
            continue;
        }
        out.push((idx, parse_u64_str(v)?));
    }
    Ok(out)
}

fn apply_updates(regs: &mut [u64; 11], updates: &[(usize, u64)]) {
    for &(idx, val) in updates {
        regs[idx] = val;
    }
}

fn snapshot_regs(snap: &RegisterSnapshot) -> Result<[u64; 11]> {
    let mut regs = [0u64; 11];
    apply_updates(&mut regs, &parse_updates(&snap.reg)?);
    Ok(regs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(
        snap: BTreeMap<String, String>,
        trace: BTreeMap<String, RegisterTraceEntry>,
    ) -> RegisterTraceChunk {
        RegisterTraceChunk {
            snapshot: RegisterSnapshot { reg: snap },
            trace,
        }
    }

    fn entry(pc: u64, delta: Option<BTreeMap<String, String>>) -> RegisterTraceEntry {
        RegisterTraceEntry { pc, reg: delta }
    }

    #[test]
    fn steps_apply_deltas_and_join_disasm() {
        let first = chunk(
            BTreeMap::from([("1".into(), "10".into())]),
            BTreeMap::from([
                (
                    "0".into(),
                    entry(0, Some(BTreeMap::from([("1".into(), "11".into())]))),
                ),
                ("1".into(), entry(8, None)),
            ]),
        );
        let second = chunk(
            BTreeMap::from([("1".into(), "999".into())]),
            BTreeMap::from([("2".into(), entry(16, None))]),
        );
        let trace = RegisterTrace::from_chunks(&[first, second]).unwrap();
        let disasm = BTreeMap::from([(0, "mov64 r1, 1".into()), (8, "add64 r1, 2".into())]);
        let steps = trace.into_steps(&disasm);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].pre_regs[1], 10);
        assert_eq!(steps[0].post_regs[1], 11);
        assert_eq!(steps[0].disasm, "mov64 r1, 1");
        assert_eq!(steps[0].next_pc, Some(8));
        assert_eq!(steps[1].pre_regs[1], 11);
        assert_eq!(steps[1].post_regs[1], 11);
        assert_eq!(steps[2].pc, 16);
        assert_eq!(steps[2].disasm, "");
        assert_eq!(
            steps[2].pre_regs[1], 11,
            "later chunk snapshots are ignored"
        );
        assert_eq!(steps[2].next_pc, None);
    }
}
