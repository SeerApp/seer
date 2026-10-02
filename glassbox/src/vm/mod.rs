//! Concolic replay of a trace: owns the scratchpad and steps SBPF ops.

use std::collections::HashMap;
use std::time::Instant;

use logger::seer_debug;

use z3::ast::Ast;

use crate::analysis::Analysis;
use crate::coverage::Skip;
use crate::parse::{parse_disasm, BinOp, Op, Operand};
use crate::path_condition::PathCondition;
use crate::regions::{input_offset, is_input};
use crate::state::{InputFrame, Registers, SymbolicState};
use crate::step::Step;
use crate::syscalls;

/// A step slower than this gets a `took` line so a hang is the last
/// `debug N pc=…` with no `took` after it.
const SLOW_STEP_MS: f64 = 10.0;

/// Replay machine. [`SymbolicState`] is the scratchpad; skip counters are
/// interpreter policy and live here. Consume with [`Vm::into_analysis`].
pub struct Vm {
    pub(crate) state: SymbolicState,
    skipped_tautologies: u64,
    skipped_text_noise: u64,
    debug: bool,
    /// Enclosing `call` frames while discovering compiler-rt helper spans.
    call_stack: Vec<HelperFrame>,
    /// Inclusive step-index ranges (`call` through matching `exit`) where a
    /// recovered UIF fired. ponytail: an inlined helper inside user code
    /// hides that whole function's jumps; tighten to the smear/popcount
    /// region if that starts dropping real conditions.
    helper_spans: Vec<(usize, usize)>,
    /// Jumps captured during [`Vm::run`] before helper spans close.
    pending_jumps: Vec<PendingJump>,
    /// When set, jumps wait until helper spans are known (see [`Vm::run`]).
    defer_jumps: bool,
    /// Parked caller activations across CPI.
    cpi_stack: Vec<CpiFrame>,
}

struct CpiFrame {
    pubkey: [u8; 32],
    registers: Registers,
    input: InputFrame,
}

struct HelperFrame {
    entry: usize,
    hit: bool,
}

struct PendingJump {
    index: usize,
    defs_len: usize,
    pc: PathCondition,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    pub fn new() -> Self {
        Self {
            state: SymbolicState::new(),
            skipped_tautologies: 0,
            skipped_text_noise: 0,
            debug: false,
            call_stack: Vec::new(),
            helper_spans: Vec::new(),
            pending_jumps: Vec::new(),
            defer_jumps: false,
            cpi_stack: Vec::new(),
        }
    }

    pub fn debug(mut self, on: bool) -> Self {
        self.debug = on;
        self
    }

    fn in_helper(&self, index: usize) -> bool {
        self.helper_spans
            .iter()
            .any(|&(start, end)| index >= start && index <= end)
    }

    fn mark_helper_hit(&mut self) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.hit = true;
        }
    }

    fn recovered_uif_in(&self, dst: usize) -> bool {
        self.state.registers[dst].as_ref().is_some_and(|s| {
            s.bv.num_children() > 0 && crate::grammar::is_recovered_uif(&s.bv.decl().name())
        })
    }

    /// Apply one step. Returns [`Skip`] only for coverage gaps (`Unknown`, unhandled syscall).
    pub fn step(&mut self, step: &Step) -> Option<Skip> {
        self.apply_step(step, 0)
    }

    fn finish_jump(&mut self, mut pc: PathCondition, defs_len: usize) {
        pc.formula = crate::rewrite::branch(&pc.formula, &self.state.ledger.load_defs[..defs_len]);
        self.state.ledger.assert_path(pc);
    }

    fn commit_pending_jumps(&mut self) {
        let pending = std::mem::take(&mut self.pending_jumps);
        for jump in pending {
            if self.in_helper(jump.index) {
                continue;
            }
            self.finish_jump(jump.pc, jump.defs_len);
        }
    }

    fn apply_step(&mut self, step: &Step, index: usize) -> Option<Skip> {
        let op = parse_disasm(&step.disasm);
        match op {
            Op::Load { width, dst, mem } => {
                let addr = mem.effective_addr(&step.pre_regs);
                self.state.load(dst, addr, width, step.post_regs[dst]);
                None
            }
            Op::StoreReg { width, mem, src } => {
                let addr = mem.effective_addr(&step.post_regs);
                self.state.store_reg(addr, width, src, step.post_regs[src]);
                None
            }
            Op::StoreImm { width, mem, imm } => {
                let addr = mem.effective_addr(&step.post_regs);
                self.state.memory.store_imm(addr, width, imm as u64);
                None
            }
            Op::Lddw { dst, .. } => {
                self.state.registers.clear(dst);
                None
            }
            Op::Alu {
                op,
                dst,
                src,
                bits32,
            } => {
                if op == BinOp::Mov {
                    match src {
                        Operand::Reg(r) if !bits32 => self.state.registers.copy(dst, r),
                        Operand::Imm(_) => self.state.registers.clear(dst),
                        Operand::Reg(r) => self.state.mov32_from_reg(dst, r),
                    }
                    return None;
                }
                self.state.alu(op, dst, &src, bits32, &step.pre_regs);
                if self.recovered_uif_in(dst) {
                    self.mark_helper_hit();
                }
                None
            }
            Op::Neg { dst, bits32 } => {
                self.state.neg(dst, bits32);
                if self.recovered_uif_in(dst) {
                    self.mark_helper_hit();
                }
                None
            }
            Op::Jump {
                rel,
                dst,
                src,
                target,
            } => {
                let taken = step.next_pc == Some(target);
                if let Some(pc) = self.state.branch(
                    rel,
                    dst,
                    &src,
                    taken,
                    &step.pre_regs,
                    step.order,
                    step.pc,
                    step.disasm.clone(),
                ) {
                    let defs_len = self.state.ledger.load_defs.len();
                    if self.defer_jumps {
                        self.pending_jumps.push(PendingJump {
                            index,
                            defs_len,
                            pc,
                        });
                    } else {
                        self.finish_jump(pc, defs_len);
                    }
                }
                None
            }
            Op::Call => {
                self.call_stack.push(HelperFrame {
                    entry: index,
                    hit: false,
                });
                self.state.registers.push_call_frame();
                None
            }
            Op::Exit => {
                if let Some(frame) = self.call_stack.pop() {
                    if self.defer_jumps && frame.hit {
                        self.helper_spans.push((frame.entry, index));
                    }
                }
                self.state.registers.pop_call_frame();
                None
            }
            Op::JumpAlways { .. } | Op::Nop => None,
            Op::Unknown => Some(Skip::unknown_op(step)),
            Op::Syscall { name } => {
                match syscalls::apply_syscall(
                    &mut self.state.memory,
                    &mut self.state.registers,
                    &name,
                    &step.pre_regs,
                    &step.post_regs,
                    step.pc,
                ) {
                    syscalls::SyscallStatus::Handled => None,
                    syscalls::SyscallStatus::Unhandled => {
                        Some(Skip::unhandled_syscall(step, &name))
                    }
                }
            }
        }
    }

    fn replay<'a>(&mut self, steps: &[&'a Step]) -> Vec<Skip> {
        let mut skips = Vec::new();
        self.call_stack.clear();
        for (i, step) in steps.iter().enumerate() {
            if self.debug {
                seer_debug!("debug {} pc=0x{:x} {}", step.order, step.pc, step.disasm);
            }
            let t0 = self.debug.then(Instant::now);
            if let Some(skip) = self.apply_step(step, i) {
                skips.push(skip);
            }
            if let Some(t0) = t0 {
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                if ms >= SLOW_STEP_MS {
                    seer_debug!("debug {} took {ms:.1}ms", step.order);
                }
            }
        }
        skips
    }

    /// One replay: recover helpers, buffer jumps, then keep only jumps
    /// outside recovered `call`/`exit` spans. In-span jumps are dropped,
    /// not counted as tautology skips. Resets the scratchpad.
    pub fn run<'a>(&mut self, steps: impl IntoIterator<Item = &'a Step>) -> Vec<Skip> {
        self.state = SymbolicState::new();
        self.skipped_tautologies = 0;
        self.skipped_text_noise = 0;
        self.cpi_stack.clear();
        self.run_stretch(steps)
    }

    /// Replay one program stretch. Keeps memory and path conditions.
    /// Helper spans are this ELF only.
    pub fn run_stretch<'a>(&mut self, steps: impl IntoIterator<Item = &'a Step>) -> Vec<Skip> {
        let steps: Vec<&Step> = steps.into_iter().collect();
        self.helper_spans.clear();
        self.pending_jumps.clear();
        self.call_stack.clear();
        self.defer_jumps = true;
        let skips = self.replay(&steps);
        self.defer_jumps = false;
        self.commit_pending_jumps();
        skips
    }

    pub(crate) fn enter_cpi(&mut self, caller_pk: [u8; 32], callee_steps: &[Step]) {
        let overlay = harvest_input_concrete(callee_steps);
        let input = self.state.memory.enter_cpi(&overlay);
        let registers = std::mem::replace(&mut self.state.registers, Registers::new());
        self.cpi_stack.push(CpiFrame {
            pubkey: caller_pk,
            registers,
            input,
        });
        self.call_stack.clear();
        self.helper_spans.clear();
        self.pending_jumps.clear();
    }

    pub(crate) fn return_cpi(&mut self) {
        if let Some(frame) = self.cpi_stack.pop() {
            self.state.memory.return_cpi(frame.input);
            self.state.registers = frame.registers;
        }
        self.call_stack.clear();
        self.helper_spans.clear();
        self.pending_jumps.clear();
    }

    pub(crate) fn parked_pubkey(&self) -> Option<[u8; 32]> {
        self.cpi_stack.last().map(|f| f.pubkey)
    }

    /// Consume the machine into display data. Registers and live memory drop.
    pub fn into_analysis(self) -> Analysis {
        let skipped_tautologies = self.skipped_tautologies;
        let skipped_text_noise = self.skipped_text_noise;
        let dump = self.state.into_dump();
        Analysis {
            ledger: dump.ledger,
            memory_cells: dump.memory_cells,
            text_bytes: dump.text_bytes,
            input_bytes: dump.input_bytes,
            skipped_tautologies,
            skipped_text_noise,
        }
    }
}

fn harvest_input_concrete(steps: &[Step]) -> HashMap<u64, u8> {
    let mut m = HashMap::new();
    let put = |m: &mut HashMap<u64, u8>, addr: u64, n: usize, value: u64| {
        if !is_input(addr) {
            return;
        }
        for i in 0..n {
            let b = ((value >> (8 * i)) & 0xff) as u8;
            m.insert(input_offset(addr).wrapping_add(i as u64), b);
        }
    };
    for step in steps {
        match parse_disasm(&step.disasm) {
            Op::Load { width, dst, mem } => put(
                &mut m,
                mem.effective_addr(&step.pre_regs),
                width.bytes(),
                step.post_regs[dst],
            ),
            Op::StoreReg { width, mem, src } => put(
                &mut m,
                mem.effective_addr(&step.post_regs),
                width.bytes(),
                step.post_regs[src],
            ),
            Op::StoreImm { width, mem, imm } => put(
                &mut m,
                mem.effective_addr(&step.post_regs),
                width.bytes(),
                imm as u64,
            ),
            _ => {}
        }
    }
    m
}

#[cfg(test)]
mod tests;
