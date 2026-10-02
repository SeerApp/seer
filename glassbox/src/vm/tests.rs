
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
    logger::init_seer_logger(logger::SeerLogger::from_env());
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
