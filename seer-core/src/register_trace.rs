use std::collections::BTreeMap;

use serde::Serialize;

pub const REGISTER_COUNT: usize = 12;
pub const TRACE_REGISTER_COUNT: usize = 11;
/// How many VM `record()` calls (instruction steps) go into one persisted register chunk.
///
/// This is **not** `trace.len()`; the first step may elide a redundant trace row, so a full chunk
/// can hold `REGISTER_TRACE_CHUNK_SIZE - 1` JSON trace entries while still spanning exactly this
/// many VM steps.
pub const REGISTER_TRACE_CHUNK_SIZE: usize = 1000;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterSnapshot {
    pub reg: BTreeMap<usize, u64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterTraceEntry {
    pub pc: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reg: Option<BTreeMap<usize, u64>>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterTraceChunk {
    pub snapshot: RegisterSnapshot,
    pub trace: Vec<RegisterTraceEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedRegisterTraceChunk {
    pub tree_uid: u64,
    pub min_order: u64,
    pub max_order: u64,
    pub chunk: RegisterTraceChunk,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveRegisterTraceChunk {
    tree_uid: u64,
    min_order: u64,
    max_order: u64,
    snapshot: [u64; REGISTER_COUNT],
    previous: [u64; REGISTER_COUNT],
    trace: Vec<RegisterTraceEntry>,
}

impl ActiveRegisterTraceChunk {
    fn new(tree_uid: u64, order: u64, reg: &[u64; REGISTER_COUNT]) -> Self {
        Self::new_carry(tree_uid, order, reg, reg)
    }

    /// `reg` seeds the chunk snapshot (first observation in this file); `previous_for_delta` is
    /// the pre-state before the first trace row so the first delta spans the VM step boundary
    /// when continuing from a rolled chunk (otherwise `previous_for_delta == reg`).
    fn new_carry(
        tree_uid: u64,
        order: u64,
        reg: &[u64; REGISTER_COUNT],
        previous_for_delta: &[u64; REGISTER_COUNT],
    ) -> Self {
        Self {
            tree_uid,
            min_order: order,
            max_order: order,
            snapshot: *reg,
            previous: *previous_for_delta,
            trace: Vec::with_capacity(REGISTER_TRACE_CHUNK_SIZE),
        }
    }

    fn push(&mut self, order: u64, pc: u64, reg: &[u64; REGISTER_COUNT]) {
        self.max_order = order;
        self.trace.push(RegisterTraceEntry {
            pc,
            reg: changed_registers(&self.previous, reg),
        });
        self.previous = *reg;
    }

    fn into_persisted(self) -> PersistedRegisterTraceChunk {
        PersistedRegisterTraceChunk {
            tree_uid: self.tree_uid,
            min_order: self.min_order,
            max_order: self.max_order,
            chunk: RegisterTraceChunk {
                snapshot: RegisterSnapshot {
                    reg: serialize_registers(&self.snapshot),
                },
                trace: self.trace,
            },
        }
    }
}

pub struct RegisterTraceCollector {
    active: Option<ActiveRegisterTraceChunk>,
    /// PC passed to the previous `record` call (VM hook runs **before** each instruction).
    ///
    /// The register delta for the current row compares pre-state at this step to pre-state at
    /// the previous step, i.e. the effect of the instruction that **just began** on the prior
    /// hook — so that row must be labeled with the **prior** hook's PC, not the current one.
    ///
    /// Together with skipping the first all-null row (see `record`), this yields one trace row
    /// per executed instruction at that instruction's PC, without duplicate PCs.
    last_hook_pc: Option<u64>,
    /// VM `record` calls finished for the current persisted chunk (including skipped snapshot rows).
    ///
    /// Chunk rollover uses this — not `trace.len()` — so eliding the first redundant row still
    /// closes a chunk every [`REGISTER_TRACE_CHUNK_SIZE`] VM steps.
    records_this_chunk: u32,
}

impl RegisterTraceCollector {
    pub fn new() -> Self {
        Self {
            active: None,
            last_hook_pc: None,
            records_this_chunk: 0,
        }
    }

    /// Emit the deferred final trace row (PC = [`Self::last_hook_pc`], `reg: None`) when the VM
    /// stops before the next hook would have done so — e.g. root `EXIT` does not change traced
    /// registers, so the delta is always absent.
    ///
    /// Skipped when [`Self::last_hook_pc`] already matches the last row's PC (first row after
    /// [`Self::open_chunk_eager`] uses the current hook's PC, so there is nothing left to defer).
    fn drain_final_deferred_row(&mut self) {
        let Some(pc) = self.last_hook_pc else {
            return;
        };
        let Some(active) = self.active.as_mut() else {
            self.last_hook_pc = None;
            return;
        };
        if active.trace.last().is_some_and(|e| e.pc == pc) {
            self.last_hook_pc = None;
            return;
        }
        let order = active.max_order.saturating_add(1);
        let prev = active.previous;
        active.push(order, pc, &prev);
        self.records_this_chunk = self.records_this_chunk.saturating_add(1);
        self.last_hook_pc = None;
    }

    /// Drop any in-progress chunk (e.g. CPI exit / enter). Returns serialized chunk if one existed.
    ///
    /// When `emit_final_deferred_row` is true (normal program end), append the pending row for
    /// [`Self::last_hook_pc`] with a null register delta before persisting. When false (CPI
    /// entry), omit — the caller's next instruction after CPI is not necessarily register-neutral.
    pub fn flush_on_invocation_boundary(
        &mut self,
        emit_final_deferred_row: bool,
    ) -> Option<PersistedRegisterTraceChunk> {
        if emit_final_deferred_row {
            self.drain_final_deferred_row();
        } else {
            self.last_hook_pc = None;
        }
        self.records_this_chunk = 0;
        self.active.take().map(ActiveRegisterTraceChunk::into_persisted)
    }

    /// After a CPI boundary flush, open a chunk for the callee without a trace row yet.
    ///
    /// The first real `record` for this `tree_uid` appends the first trace line. This ensures
    /// callees with zero VM steps still produce a snapshot-only artifact with the correct uid.
    pub fn open_chunk_eager(
        &mut self,
        tree_uid: u64,
        order: u64,
        reg: &[u64; REGISTER_COUNT],
    ) {
        debug_assert!(
            self.active.is_none(),
            "open_chunk_eager expects no active chunk after flush_on_invocation_boundary"
        );
        self.last_hook_pc = None;
        self.records_this_chunk = 0;
        self.active = Some(ActiveRegisterTraceChunk::new(tree_uid, order, reg));
    }

    pub fn record(
        &mut self,
        order: u64,
        pc: u64,
        reg: &[u64; REGISTER_COUNT],
        tree_uid: u64,
    ) -> Option<PersistedRegisterTraceChunk> {
        let mut completed = None;

        let carried_previous = self.active.as_ref().map(|a| a.previous);

        if let Some(active) = self.active.as_ref() {
            let uid_mismatch = active.tree_uid != tree_uid;
            let step_bucket_full =
                self.records_this_chunk >= REGISTER_TRACE_CHUNK_SIZE as u32;
            if uid_mismatch || step_bucket_full {
                completed = self.active.take().map(ActiveRegisterTraceChunk::into_persisted);
                if uid_mismatch {
                    self.last_hook_pc = None;
                }
                self.records_this_chunk = 0;
            }
        }

        self.active.get_or_insert_with(|| {
            let previous_for_delta = carried_previous.unwrap_or(*reg);
            ActiveRegisterTraceChunk::new_carry(tree_uid, order, reg, &previous_for_delta)
        });
        let chunk = self.active.as_mut().expect("active chunk");
        debug_assert_eq!(chunk.tree_uid, tree_uid);
        let trace_pc = self.last_hook_pc.unwrap_or(pc);
        let delta = changed_registers(&chunk.previous, reg);

        // Pre-execution hook: the first row would only repeat the snapshot (null delta) at the same
        // PC as the next row (once we defer PC to the prior hook). Skip it — one row per insn.
        let skip_redundant_snapshot_row =
            chunk.trace.is_empty() && delta.is_none() && self.last_hook_pc.is_none();

        if skip_redundant_snapshot_row {
            self.last_hook_pc = Some(pc);
            self.records_this_chunk += 1;
            return completed;
        }

        chunk.push(order, trace_pc, reg);
        self.last_hook_pc = Some(pc);
        self.records_this_chunk += 1;

        completed
    }

    pub fn finalize(&mut self) -> Option<PersistedRegisterTraceChunk> {
        self.drain_final_deferred_row();
        self.last_hook_pc = None;
        self.records_this_chunk = 0;
        self.active.take().map(ActiveRegisterTraceChunk::into_persisted)
    }
}

impl Default for RegisterTraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

fn changed_registers(
    previous: &[u64; REGISTER_COUNT],
    current: &[u64; REGISTER_COUNT],
) -> Option<BTreeMap<usize, u64>> {
    let mut changed = BTreeMap::new();
    let mut has_change = false;

    for index in 0..TRACE_REGISTER_COUNT {
        if previous[index] != current[index] {
            changed.insert(index, current[index]);
            has_change = true;
        }
    }

    has_change.then_some(changed)
}

fn serialize_registers(registers: &[u64; REGISTER_COUNT]) -> BTreeMap<usize, u64> {
    let mut reg = BTreeMap::new();

    for (index, value) in registers.iter().copied().enumerate().take(TRACE_REGISTER_COUNT) {
        reg.insert(index, value);
    }

    reg
}

#[cfg(test)]
mod tests {
    use super::{
        RegisterTraceCollector, REGISTER_COUNT, REGISTER_TRACE_CHUNK_SIZE, TRACE_REGISTER_COUNT,
    };

    const UID: u64 = 42;

    fn regs(seed: u64) -> [u64; REGISTER_COUNT] {
        let mut reg = [0; REGISTER_COUNT];
        for (index, slot) in reg.iter_mut().enumerate() {
            *slot = seed + index as u64;
        }
        reg
    }

    #[test]
    fn first_step_skips_redundant_row_matching_snapshot() {
        let mut collector = RegisterTraceCollector::new();
        let reg = regs(10);

        let completed = collector.record(0, 36, &reg, UID);
        assert!(completed.is_none());

        let chunk = collector.finalize().expect("final chunk");
        assert_eq!(chunk.tree_uid, UID);
        assert_eq!(chunk.min_order, 0);
        assert_eq!(chunk.max_order, 1);
        assert_eq!(chunk.chunk.snapshot.reg.len(), TRACE_REGISTER_COUNT);
        assert_eq!(chunk.chunk.snapshot.reg[&0], 10);
        assert_eq!(chunk.chunk.snapshot.reg[&10], 20);
        assert!(!chunk.chunk.snapshot.reg.contains_key(&11));
        assert_eq!(chunk.chunk.trace.len(), 1);
        assert_eq!(chunk.chunk.trace[0].pc, 36);
        assert!(chunk.chunk.trace[0].reg.is_none());
    }

    #[test]
    fn changed_steps_only_emit_modified_registers() {
        let mut collector = RegisterTraceCollector::new();
        let reg = regs(20);
        collector.record(5, 40, &reg, UID);

        let mut changed = reg;
        changed[3] = 999;
        changed[8] = 1234;
        changed[11] = 5555;
        collector.record(6, 44, &changed, UID);

        let chunk = collector.finalize().expect("final chunk");
        assert_eq!(chunk.chunk.trace.len(), 2);
        let delta = chunk.chunk.trace[0].reg.as_ref().expect("delta");
        assert_eq!(delta.get(&3), Some(&999));
        assert_eq!(delta.get(&8), Some(&1234));
        assert_eq!(delta.len(), 2);
        assert!(!delta.contains_key(&11));
        assert_eq!(chunk.chunk.trace[0].pc, 40);
        assert_eq!(chunk.chunk.trace[1].pc, 44);
        assert!(chunk.chunk.trace[1].reg.is_none());
        assert_eq!(chunk.max_order, 7);
    }

    #[test]
    fn chunk_rollover_streams_after_thousand_entries() {
        let mut collector = RegisterTraceCollector::new();
        let mut reg = regs(100);

        for order in 0..REGISTER_TRACE_CHUNK_SIZE as u64 {
            if order == 500 {
                reg[1] += 1;
            }
            let completed = collector.record(order, order + 1000, &reg, UID);
            assert!(completed.is_none());
        }

        reg[2] += 7;
        let completed = collector
            .record(REGISTER_TRACE_CHUNK_SIZE as u64, 9000, &reg, UID)
            .expect("completed chunk");

        assert_eq!(completed.tree_uid, UID);
        assert_eq!(completed.min_order, 0);
        assert_eq!(completed.max_order, REGISTER_TRACE_CHUNK_SIZE as u64 - 1);
        assert_eq!(completed.chunk.trace.len(), REGISTER_TRACE_CHUNK_SIZE - 1);
        assert_eq!(completed.chunk.snapshot.reg[&0], 100);
        assert!(!completed.chunk.snapshot.reg.contains_key(&11));

        let tail = collector.finalize().expect("tail chunk");
        assert_eq!(tail.tree_uid, UID);
        assert_eq!(tail.min_order, REGISTER_TRACE_CHUNK_SIZE as u64);
        assert_eq!(tail.max_order, REGISTER_TRACE_CHUNK_SIZE as u64 + 1);
        assert_eq!(tail.chunk.snapshot.reg[&2], reg[2]);
        assert!(!tail.chunk.snapshot.reg.contains_key(&11));
        assert_eq!(tail.chunk.trace.len(), 2);
        let d0 = tail.chunk.trace[0].reg.as_ref().expect("rollover carries prior regs");
        assert_eq!(d0.get(&2), Some(&(reg[2])));
        assert_eq!(tail.chunk.trace[1].pc, 9000);
        assert!(tail.chunk.trace[1].reg.is_none());
    }

    #[test]
    fn uid_change_finalizes_previous_chunk() {
        let mut collector = RegisterTraceCollector::new();
        let reg = regs(1);
        collector.record(0, 10, &reg, 1);
        let done = collector.record(1, 11, &reg, 2).expect("flush on uid change");
        assert_eq!(done.tree_uid, 1);
        assert_eq!(done.min_order, 0);
        assert_eq!(done.max_order, 0);

        let tail = collector.finalize().expect("tail");
        assert_eq!(tail.tree_uid, 2);
    }

    #[test]
    fn open_chunk_eager_produces_snapshot_only_until_first_record() {
        let mut collector = RegisterTraceCollector::new();
        let reg = regs(7);
        collector.open_chunk_eager(99, 3, &reg);

        let tail = collector.finalize().expect("chunk");
        assert_eq!(tail.tree_uid, 99);
        assert_eq!(tail.min_order, 3);
        assert_eq!(tail.max_order, 3);
        assert_eq!(tail.chunk.trace.len(), 0);
        assert_eq!(tail.chunk.snapshot.reg[&0], 7);
    }

    #[test]
    fn open_chunk_eager_then_record_appends_trace() {
        let mut collector = RegisterTraceCollector::new();
        let reg0 = regs(0);
        collector.open_chunk_eager(5, 10, &reg0);

        let mut reg1 = reg0;
        reg1[2] = 42;
        assert!(collector.record(11, 99, &reg1, 5).is_none());

        let tail = collector.finalize().expect("chunk");
        assert_eq!(tail.tree_uid, 5);
        assert_eq!(tail.chunk.trace.len(), 1);
        assert_eq!(tail.chunk.trace[0].pc, 99);
        let delta = tail.chunk.trace[0].reg.as_ref().expect("delta");
        assert_eq!(delta.get(&2), Some(&42));
        assert_eq!(tail.max_order, 11);
    }
}
