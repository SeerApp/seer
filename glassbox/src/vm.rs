//! Concolic replay of a trace: owns the scratchpad and steps SBPF ops.

use std::collections::HashMap;
use std::io::{self, Write};
use std::time::Instant;

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

fn debug_line(msg: impl std::fmt::Display) {
    eprintln!("{msg}");
    let _ = io::stderr().flush();
}

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
                        Operand::Reg(r) => self.state.registers.copy(dst, r),
                        Operand::Imm(_) => self.state.registers.clear(dst),
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
                debug_line(format_args!(
                    "debug {} pc=0x{:x} {}",
                    step.order, step.pc, step.disasm
                ));
            }
            let t0 = self.debug.then(Instant::now);
            if let Some(skip) = self.apply_step(step, i) {
                skips.push(skip);
            }
            if let Some(t0) = t0 {
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                if ms >= SLOW_STEP_MS {
                    debug_line(format_args!("debug {} took {ms:.1}ms", step.order));
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
mod tests {
    use super::*;
    use crate::coverage::SkipReason;
    use crate::regions::INPUT_BASE;
    use crate::state::{SymVal, SysvarOrigin};
    use z3::ast::BV;

    #[test]
    fn load_mints_input_byte() {
        let mut vm = Vm::new();
        let mut pre = [0u64; 11];
        pre[1] = INPUT_BASE;
        let step = Step {
            order: 0,
            pc: 0,
            next_pc: Some(8),
            disasm: "ldxb r0, [r1+0]".into(),
            pre_regs: pre,
            post_regs: pre,
        };
        vm.step(&step);
        let r0 = vm.state.registers[0].as_ref().unwrap();
        assert!(r0.environmental);
        assert!(r0.bv.to_string().contains("n_num_accounts_00"));
        assert!(vm.state.ledger.load_defs.is_empty());
    }

    #[test]
    fn load_mints_load_temp_for_dword() {
        let mut vm = Vm::new();
        let mut pre = [0u64; 11];
        pre[1] = INPUT_BASE;
        let step = Step {
            order: 0,
            pc: 0,
            next_pc: Some(8),
            disasm: "ldxdw r0, [r1+0]".into(),
            pre_regs: pre,
            post_regs: pre,
        };
        vm.step(&step);
        assert_eq!(vm.state.ledger.load_defs.len(), 1);
        assert_eq!(vm.state.ledger.load_defs[0].name, "w_num_accounts");
        assert!(vm.state.registers[0]
            .as_ref()
            .is_some_and(|s| s.bv.to_string() == "w_num_accounts"));
    }

    #[test]
    fn load_reuses_temp_for_identical_dword() {
        let mut vm = Vm::new();
        let mut pre = [0u64; 11];
        pre[1] = INPUT_BASE;
        let step = Step {
            order: 0,
            pc: 0,
            next_pc: Some(8),
            disasm: "ldxdw r0, [r1+0]".into(),
            pre_regs: pre,
            post_regs: pre,
        };
        vm.step(&step);
        vm.step(&step);
        assert_eq!(vm.state.ledger.load_defs.len(), 1);
        assert_eq!(vm.state.ledger.load_defs[0].name, "w_num_accounts");
    }

    #[test]
    fn load_names_later_account_fields_from_concrete_data_len() {
        let mut vm = Vm::new();
        let mut pre = [0u64; 11];
        pre[1] = INPUT_BASE;
        let mut post = pre;
        post[0] = 0;
        vm.step(&Step {
            order: 0,
            pc: 0,
            next_pc: Some(8),
            disasm: "ldxdw r0, [r1+88]".into(),
            pre_regs: pre,
            post_regs: post,
        });
        assert_eq!(vm.state.ledger.load_defs[0].name, "w_acc0_data_len");

        let mut dup_post = pre;
        dup_post[0] = 0xff;
        vm.step(&Step {
            order: 1,
            pc: 8,
            next_pc: Some(16),
            disasm: "ldxb r0, [r1+10344]".into(),
            pre_regs: pre,
            post_regs: dup_post,
        });
        assert!(vm.state.registers[0]
            .as_ref()
            .unwrap()
            .bv
            .to_string()
            .contains("n_acc1_dup"));

        post[0] = 96;
        vm.step(&Step {
            order: 2,
            pc: 16,
            next_pc: Some(24),
            disasm: "ldxdw r0, [r1+10424]".into(),
            pre_regs: pre,
            post_regs: post,
        });
        assert_eq!(
            vm.state.ledger.load_defs.last().unwrap().name,
            "w_acc1_data_len"
        );

        vm.step(&Step {
            order: 3,
            pc: 24,
            next_pc: Some(32),
            disasm: "ldxdw r0, [r1+20856]".into(),
            pre_regs: pre,
            post_regs: pre,
        });
        assert_eq!(
            vm.state.ledger.load_defs.last().unwrap().name,
            "w_acc2_data_len"
        );
    }

    fn rent_syscall_and_load(vm: &mut Vm, bits: u64) {
        let buf = 0x2000_0000u64;
        let mut pre = [0u64; 11];
        pre[1] = buf;
        vm.step(&Step {
            order: 0,
            pc: 0x100,
            next_pc: Some(0x108),
            disasm: "syscall sol_get_rent_sysvar".into(),
            pre_regs: pre,
            post_regs: pre,
        });
        let mut post = pre;
        post[0] = bits;
        vm.step(&Step {
            order: 1,
            pc: 0x108,
            next_pc: Some(0x110),
            disasm: "ldxdw r0, [r1+0]".into(),
            pre_regs: pre,
            post_regs: post,
        });
    }

    #[test]
    fn sysvar_load_is_concrete_with_provenance() {
        let mut vm = Vm::new();
        rent_syscall_and_load(&mut vm, 3480);
        let v = vm.state.registers[0].as_ref().unwrap();
        assert!(v.environmental);
        assert_eq!(v.bv.as_u64(), Some(3480));
        assert_eq!(
            v.origins,
            vec![SysvarOrigin {
                syscall: "sol_get_rent_sysvar",
                pc: 0x100
            }]
        );
        assert!(vm.state.ledger.load_defs.is_empty());
    }

    #[test]
    fn get_sysvar_marks_dest_and_stamps_r0() {
        let mut vm = Vm::new();
        vm.state.registers[0] = Some(SymVal::named("n_acc0_lamports"));
        let buf = 0x2000_0000u64;
        let mut pre = [0u64; 11];
        pre[2] = buf;
        pre[4] = 8;
        let mut post = pre;
        post[0] = 0;
        assert!(vm
            .step(&Step {
                order: 0,
                pc: 0x100,
                next_pc: Some(0x108),
                disasm: "syscall sol_get_sysvar".into(),
                pre_regs: pre,
                post_regs: post,
            })
            .is_none());
        let r0 = vm.state.registers[0].as_ref().unwrap();
        assert_eq!(r0.bv.as_u64(), Some(0));
        assert!(!r0.environmental);

        let mut load_post = post;
        load_post[0] = 0x1111;
        vm.step(&Step {
            order: 1,
            pc: 0x108,
            next_pc: Some(0x110),
            disasm: "ldxdw r0, [r2+0]".into(),
            pre_regs: post,
            post_regs: load_post,
        });
        let v = vm.state.registers[0].as_ref().unwrap();
        assert!(v.environmental);
        assert_eq!(v.bv.as_u64(), Some(0x1111));
        assert_eq!(
            v.origins,
            vec![SysvarOrigin {
                syscall: "sol_get_sysvar",
                pc: 0x100
            }]
        );
    }

    #[test]
    fn sysvar_alu_folds_but_keeps_origin() {
        let mut vm = Vm::new();
        rent_syscall_and_load(&mut vm, 3480);
        let mut regs = [0u64; 11];
        regs[0] = 3480;
        vm.step(&Step {
            order: 2,
            pc: 0x110,
            next_pc: Some(0x118),
            disasm: "mul64 r0, 0x129".into(),
            pre_regs: regs,
            post_regs: regs,
        });
        let v = vm.state.registers[0].as_ref().unwrap();
        assert_eq!(v.bv.as_u64(), Some(3480 * 0x129));
        assert_eq!(v.origins[0].syscall, "sol_get_rent_sysvar");
    }

    #[test]
    fn sysvar_only_compare_is_tautology() {
        let mut vm = Vm::new();
        rent_syscall_and_load(&mut vm, 3480);
        let mut regs = [0u64; 11];
        regs[0] = 3480;
        vm.step(&Step {
            order: 2,
            pc: 0x110,
            next_pc: Some(0x200),
            disasm: "jgt r0, 1, 0x200".into(),
            pre_regs: regs,
            post_regs: regs,
        });
        let analysis = vm.into_analysis();
        assert_eq!(analysis.ledger.path_conditions.len(), 1);
        assert_eq!(analysis.skipped_tautologies, 0);
    }

    #[test]
    fn sysvar_vs_input_keeps_constant_and_origin() {
        let mut vm = Vm::new();
        rent_syscall_and_load(&mut vm, 3480);
        vm.state.registers[9] = Some(SymVal::env(BV::new_const("w_acc3_lamports", 64)));
        let mut regs = [0u64; 11];
        regs[0] = 3480;
        vm.step(&Step {
            order: 2,
            pc: 0x110,
            next_pc: Some(0x200),
            disasm: "jle r0, r9, 0x200".into(),
            pre_regs: regs,
            post_regs: regs,
        });
        let analysis = vm.into_analysis();
        assert_eq!(analysis.ledger.path_conditions.len(), 1);
        let pc = &analysis.ledger.path_conditions[0];
        assert_eq!(
            pc.origins,
            vec![SysvarOrigin {
                syscall: "sol_get_rent_sysvar",
                pc: 0x100
            }]
        );
        let blob = pc.formula.to_string();
        assert!(blob.contains("w_acc3_lamports"), "{blob}");
        assert!(
            blob.contains("3480") || blob.contains("#x0000000000000d98"),
            "{blob}"
        );
    }

    fn uif_arg_bits(bv: &BV) -> Option<u32> {
        use z3::ast::{Ast, Dynamic};
        use z3::DeclKind;
        let mut cur = Dynamic::from(bv);
        loop {
            match cur.decl().kind() {
                DeclKind::Extract | DeclKind::Blshr => cur = cur.nth_child(0)?,
                _ if crate::grammar::is_syscall_uif(&cur.decl().name()) => {
                    return cur.nth_child(0)?.as_bv().map(|a| a.get_size());
                }
                _ => return None,
            }
        }
    }

    #[test]
    fn stack_store_makes_seed_table_visible() {
        let mut vm = Vm::new();
        const STACK: u64 = 0x2_0000_0000;
        const OUT: u64 = 0x3_0000_0000;
        let prog = INPUT_BASE;
        let seed = INPUT_BASE + 32;
        for off in 0..32 {
            let _ = vm.state.memory.resolve_byte(prog + off);
            let _ = vm.state.memory.resolve_byte(seed + off);
        }
        let mut pre = [0u64; 11];
        pre[1] = STACK;
        pre[2] = seed;
        vm.step(&Step {
            order: 0,
            pc: 0,
            next_pc: Some(8),
            disasm: "stxdw [r1+0], r2".into(),
            pre_regs: pre,
            post_regs: pre,
        });
        pre[2] = 32;
        vm.step(&Step {
            order: 1,
            pc: 8,
            next_pc: Some(16),
            disasm: "stxdw [r1+8], r2".into(),
            pre_regs: pre,
            post_regs: pre,
        });
        pre[2] = 1;
        pre[3] = prog;
        pre[4] = OUT;
        vm.step(&Step {
            order: 2,
            pc: 16,
            next_pc: Some(24),
            disasm: "syscall sol_create_program_address".into(),
            pre_regs: pre,
            post_regs: pre,
        });
        let out = vm.state.memory.resolve_byte(OUT).expect("pda byte");
        assert_eq!(uif_arg_bits(&out.bv), Some(512));
    }

    fn bare_step(disasm: &str) -> Step {
        Step {
            order: 1,
            pc: 8,
            next_pc: None,
            disasm: disasm.into(),
            pre_regs: [0; 11],
            post_regs: [0; 11],
        }
    }

    #[test]
    fn callx_saves_callee_regs_like_call() {
        let mut vm = Vm::new();
        vm.state.registers[7] = Some(SymVal::env(BV::new_const("w_exp", 64)));
        assert!(vm.step(&bare_step("callx r4")).is_none());
        vm.state.registers[7] = Some(SymVal::env(BV::new_const("w_clobbered", 64)));
        assert!(vm.step(&bare_step("exit")).is_none());
        assert_eq!(
            vm.state.registers[7].as_ref().unwrap().bv.to_string(),
            "w_exp"
        );
    }

    #[test]
    fn unknown_op_is_a_coverage_skip() {
        let mut vm = Vm::new();
        let skip = vm.step(&bare_step("hor64 r1, r2"));
        assert_eq!(skip.as_ref().map(|s| s.reason), Some(SkipReason::UnknownOp));
        assert_eq!(skip.unwrap().detail, "hor64 r1, r2");
    }

    #[test]
    fn unhandled_syscall_is_a_coverage_skip() {
        let mut vm = Vm::new();
        let skip = vm.step(&bare_step("syscall sol_sha256"));
        assert_eq!(
            skip.as_ref().map(|s| s.reason),
            Some(SkipReason::UnhandledSyscall)
        );
        assert_eq!(skip.unwrap().detail, "sol_sha256");
    }

    #[test]
    fn modelled_syscall_is_not_a_skip() {
        let mut vm = Vm::new();
        assert!(vm.step(&bare_step("syscall sol_log_")).is_none());
    }

    #[test]
    fn helper_span_skips_jumps_inside_recovered_clz() {
        let mut vm = Vm::new();
        let mut regs = [0u64; 11];
        regs[1] = INPUT_BASE;
        regs[3] = u64::MAX;
        regs[4] = 0x5555_5555_5555_5555;
        regs[5] = 0x3333_3333_3333_3333;
        regs[6] = 0x0f0f_0f0f_0f0f_0f0f;
        regs[7] = 0x0101_0101_0101_0101;

        let mut order = 0u64;
        let mut pc = 0u64;
        let mut steps = Vec::new();
        let mut push = |disasm: &str, next: Option<u64>| {
            steps.push(Step {
                order,
                pc,
                next_pc: next.or(Some(pc + 8)),
                disasm: disasm.into(),
                pre_regs: regs,
                post_regs: regs,
            });
            order += 1;
            pc += 8;
        };

        push("ldxdw r0, [r1+0]", None);
        push("jne r0, 0, 0x1000", Some(0x1000));
        push("call", None);
        for sh in [1, 2, 4, 8, 16, 32] {
            push("mov64 r2, r0", None);
            push(&format!("rsh64 r2, {sh}"), None);
            push("or64 r0, r2", None);
        }
        push("jne r0, 0, 0x2000", Some(0x2000));
        push("xor64 r0, r3", None);
        push("mov64 r2, r0", None);
        push("rsh64 r2, 1", None);
        push("and64 r2, r4", None);
        push("sub64 r0, r2", None);
        push("mov64 r2, r0", None);
        push("and64 r0, r5", None);
        push("rsh64 r2, 2", None);
        push("and64 r2, r5", None);
        push("add64 r0, r2", None);
        push("mov64 r2, r0", None);
        push("rsh64 r2, 4", None);
        push("add64 r0, r2", None);
        push("and64 r0, r6", None);
        push("mul64 r0, r7", None);
        push("rsh64 r0, 56", None);
        push("exit", None);
        push("jgt r0, 1, 0x3000", Some(0x3000));

        assert!(vm.run(steps.iter()).is_empty());
        let analysis = vm.into_analysis();
        let disasms: Vec<_> = analysis
            .ledger
            .path_conditions
            .iter()
            .map(|pc| pc.disasm.as_str())
            .collect();
        assert!(
            disasms.contains(&"jne r0, 0, 0x1000"),
            "outer jump dropped: {disasms:?}"
        );
        assert!(
            disasms.contains(&"jgt r0, 1, 0x3000"),
            "post-helper jump dropped: {disasms:?}"
        );
        assert!(
            !disasms.contains(&"jne r0, 0, 0x2000"),
            "helper jump recorded: {disasms:?}"
        );
        let post = analysis
            .ledger
            .path_conditions
            .iter()
            .find(|pc| pc.disasm == "jgt r0, 1, 0x3000")
            .unwrap();
        let blob = post.formula.to_string();
        assert!(blob.contains("clz"), "{blob}");
    }

    #[test]
    fn debug_run_applies_steps() {
        let mut vm = Vm::new().debug(true);
        let step = bare_step("mov64 r0, 1");
        assert!(vm.run(std::iter::once(&step)).is_empty());
    }

    #[test]
    fn cpi_parks_caller_registers() {
        let mut vm = Vm::new();
        vm.state.registers[7] = Some(SymVal::env(BV::new_const("w_caller", 64)));
        let callee = [bare_step("mov64 r0, 1")];
        vm.enter_cpi([1u8; 32], &callee);
        assert!(vm.state.registers[7].is_none());
        assert!(vm.run_stretch(callee.iter()).is_empty());
        vm.return_cpi();
        assert_eq!(
            vm.state.registers[7].as_ref().unwrap().bv.to_string(),
            "w_caller"
        );
    }
}
