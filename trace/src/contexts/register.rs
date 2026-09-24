use std::collections::BTreeMap;

use serde::Serialize;

pub const TRACE_REGISTER_COUNT: usize = 11;
/// Number of `trace` map entries written per rolled chunk file after the trailing frontier row is
/// stripped into `cross_chunk_stub`. We roll once `trace.len() >= CHUNK_SIZE + 1` so the strip
/// leaves exactly this many keys (the first record after a roll may insert stub + frontier, i.e.
/// two keys, so a record counter alone is not aligned with map size).
pub const REGISTER_TRACE_CHUNK_SIZE: u64 = 1000;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterSnapshot {
    pub reg: BTreeMap<usize, String>,
    pub call_depth: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterTraceEntry {
    pub pc: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reg: Option<BTreeMap<usize, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_depth: Option<u32>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct TransactionRegisterContext {
    pub snapshot: RegisterSnapshot,
    pub trace: BTreeMap<u64, RegisterTraceEntry>,
    #[serde(skip)]
    pub min_order: u64,
}

/// Buffered register-trace state for one Solana program activation (top-level instruction
/// program or a single CPI callee). Stacked so chunk rollover / `cross_chunk_stub` never cross
/// a CPI boundary into another program's on-disk trace.
struct RegisterTraceInvocation {
    /// VM native frame pointers for `call_depth` (unchanged semantics vs pre-stack layout).
    frame_stack: Vec<u64>,
    rx: Option<TransactionRegisterContext>,
    previous: Option<[u64; TRACE_REGISTER_COUNT]>,
    last_trace_call_depth: Option<u32>,
    cross_chunk_stub: Option<(u64, u64)>,
}

impl RegisterTraceInvocation {
    fn new_empty() -> Self {
        Self {
            frame_stack: vec![],
            rx: None,
            previous: None,
            last_trace_call_depth: None,
            cross_chunk_stub: None,
        }
    }
}

pub struct RegisterContext {
    /// Outermost = current instruction's entry program; push on CPI in, pop on callee return.
    invocations: Vec<RegisterTraceInvocation>,
}

impl Default for RegisterContext {
    fn default() -> Self {
        Self::new()
    }
}

impl RegisterContext {
    pub fn new() -> Self {
        Self {
            invocations: vec![RegisterTraceInvocation::new_empty()],
        }
    }

    /// One empty layer for the CPI callee; caller state remains underneath for resume after `end_program`.
    pub fn push_invocation(&mut self) {
        self.invocations.push(RegisterTraceInvocation::new_empty());
    }

    /// After persisting the ending callee's trace; no-op when only the outer invocation remains.
    pub fn pop_invocation_if_nested(&mut self) {
        if self.invocations.len() > 1 {
            self.invocations.pop();
        }
    }

    /// New transaction must not inherit CPI stack or in-flight buffers from the previous tx.
    pub fn reset_for_new_transaction(&mut self) {
        self.invocations.clear();
        self.invocations.push(RegisterTraceInvocation::new_empty());
    }

    fn active_mut(&mut self) -> &mut RegisterTraceInvocation {
        self.invocations
            .last_mut()
            .expect("register trace: invariant at least one invocation layer")
    }

    pub fn record(
        &mut self,
        order: u64,
        pc: u64,
        reg: &[u64; 12],
    ) -> Option<TransactionRegisterContext> {
        let layer = self.active_mut();
        let current = trace_regs(reg);

        if layer
            .rx
            .as_ref()
            .is_some_and(|rx| rx.trace.contains_key(&order))
        {
            let call_depth = update_call_depth(&mut layer.frame_stack, reg[10]);
            layer.last_trace_call_depth = Some(call_depth);
            layer.previous = Some(current);
            return None;
        }

        let call_depth = update_call_depth(&mut layer.frame_stack, reg[10]);
        let call_depth_entry = if layer.last_trace_call_depth == Some(call_depth) {
            None
        } else {
            Some(call_depth)
        };

        if let Some(rx) = layer.rx.as_mut() {
            let reg_delta = layer.previous.and_then(|p| changed_registers(&p, &current));

            let prev_key = order.saturating_sub(1);
            if let Some(entry) = rx.trace.get_mut(&prev_key) {
                entry.reg = reg_delta;
                entry.call_depth = call_depth_entry;
            } else {
                #[cfg(debug_assertions)]
                panic!("register trace missing attribution row {prev_key} for hook order {order}");
                #[cfg(not(debug_assertions))]
                rx.trace.insert(
                    prev_key,
                    RegisterTraceEntry {
                        pc,
                        reg: reg_delta,
                        call_depth: call_depth_entry,
                    },
                );
            }
            rx.trace.insert(
                order,
                RegisterTraceEntry {
                    pc,
                    reg: None,
                    call_depth: None,
                },
            );
        } else {
            let mut trace = BTreeMap::new();

            if let Some((stub_order, stub_pc)) = layer.cross_chunk_stub.take() {
                let stub_reg_delta = layer.previous.and_then(|p| changed_registers(&p, &current));
                trace.insert(
                    stub_order,
                    RegisterTraceEntry {
                        pc: stub_pc,
                        reg: stub_reg_delta,
                        call_depth: call_depth_entry,
                    },
                );
                trace.insert(
                    order,
                    RegisterTraceEntry {
                        pc,
                        reg: None,
                        call_depth: None,
                    },
                );
            } else {
                trace.insert(
                    order,
                    RegisterTraceEntry {
                        pc,
                        reg: None,
                        call_depth: None,
                    },
                );
            }

            layer.rx = Some(TransactionRegisterContext {
                snapshot: RegisterSnapshot {
                    reg: serialize_registers(&current[..]),
                    call_depth,
                },
                min_order: trace.first_key_value().map(|(&k, _)| k).unwrap_or(order),
                trace,
            });
        }

        let roll = {
            let layer = self.active_mut();
            layer.last_trace_call_depth = Some(call_depth);
            layer.previous = Some(current);
            layer
                .rx
                .as_ref()
                .is_some_and(|rx| rx.trace.len() > (REGISTER_TRACE_CHUNK_SIZE as usize))
        };
        if roll {
            self.flush_for_roll()
        } else {
            None
        }
    }

    pub fn flush_for_roll(&mut self) -> Option<TransactionRegisterContext> {
        self.flush_inner(true)
    }

    pub fn flush_finalize(&mut self) -> Option<TransactionRegisterContext> {
        self.flush_inner(false)
    }

    fn flush_inner(&mut self, strip_trailing_frontier: bool) -> Option<TransactionRegisterContext> {
        let layer = self.active_mut();
        if let Some(mut rx) = layer.rx.take() {
            layer.last_trace_call_depth = None;

            if strip_trailing_frontier {
                if let Some((tail_k, tail_v)) = rx.trace.pop_last() {
                    let empty = tail_v.reg.is_none() && tail_v.call_depth.is_none();
                    if empty {
                        layer.cross_chunk_stub = Some((tail_k, tail_v.pc));
                    } else {
                        rx.trace.insert(tail_k, tail_v);
                    }
                }
            }

            Some(rx)
        } else {
            None
        }
    }

    pub fn previous_register(&self) -> Option<&[u64]> {
        self.invocations
            .last()
            .and_then(|layer| layer.previous.as_ref().map(|p| p.as_slice()))
    }
}

fn trace_regs(reg: &[u64; 12]) -> [u64; TRACE_REGISTER_COUNT] {
    core::array::from_fn(|i| reg[i])
}

fn serialize_registers(registers: &[u64]) -> BTreeMap<usize, String> {
    let mut reg = BTreeMap::new();

    for (index, value) in registers
        .iter()
        .copied()
        .enumerate()
        .take(TRACE_REGISTER_COUNT)
    {
        reg.insert(index, value.to_string());
    }

    reg
}

fn update_call_depth(frame_stack: &mut Vec<u64>, frame_ptr: u64) -> u32 {
    if let Some(pos) = frame_stack.iter().rposition(|fp| *fp == frame_ptr) {
        frame_stack.truncate(pos.saturating_add(1));
    } else {
        frame_stack.push(frame_ptr);
    }
    frame_stack.len().saturating_sub(1) as u32
}

fn changed_registers(
    previous: &[u64; TRACE_REGISTER_COUNT],
    current: &[u64; TRACE_REGISTER_COUNT],
) -> Option<BTreeMap<usize, String>> {
    let mut changed = BTreeMap::new();
    let mut has_change = false;

    for index in 0..TRACE_REGISTER_COUNT {
        if previous[index] != current[index] {
            changed.insert(index, current[index].to_string());
            has_change = true;
        }
    }

    has_change.then_some(changed)
}
