use super::*;
use crate::analysis::options::{HideDataLenChildren, HideMode};
use crate::path_condition::View;
use crate::state::{Ledger, LoadDef};
use z3::ast::BV;

fn shown_pcs<'a>(
    ledger: &'a Ledger,
    taken_only: bool,
    hide: HideFilters,
) -> Vec<&'a PathCondition> {
    View::apply(
        &ledger.path_conditions,
        &ledger.load_defs,
        taken_only,
        hide.constraint_hide(),
    )
    .path_conditions
}

#[test]
fn load_defs_filtered_to_path_condition_references() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_0".into(),
        expr: BV::from_u64(0, 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_74".into(),
        expr: BV::from_u64(1, 64),
    });
    let w0 = BV::new_const("w_0", 64);
    let w99 = BV::new_const("w_99", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jeq r3, r2".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: w0.eq(&w99),
    });

    let shown = load_defs_to_show(
        state.path_conditions.iter(),
        &state.load_defs,
        false,
        HideFilters::default(),
    );
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].name, "w_0");

    let all = load_defs_to_show(
        state.path_conditions.iter(),
        &state.load_defs,
        true,
        HideFilters::default(),
    );
    assert_eq!(all.len(), 2);
}

#[test]
fn load_defs_filter_matches_abi_temp_names() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_num_accounts".into(),
        expr: BV::from_u64(0, 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_data_len".into(),
        expr: BV::new_const("w_num_accounts", 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_99".into(),
        expr: BV::from_u64(1, 64),
    });
    let data_len = BV::new_const("w_acc0_data_len", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jgt r1, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: data_len.eq(&BV::from_u64(0, 64)),
    });

    let shown = load_defs_to_show(
        state.path_conditions.iter(),
        &state.load_defs,
        false,
        HideFilters::default(),
    );
    let names: Vec<&str> = shown.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["w_num_accounts", "w_acc0_data_len"]);
}

#[test]
fn taken_only_filters_path_conditions_and_load_defs() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_0".into(),
        expr: BV::from_u64(0, 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_1".into(),
        expr: BV::from_u64(1, 64),
    });
    let w0 = BV::new_const("w_0", 64);
    let w1 = BV::new_const("w_1", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jeq r3, r2".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: w0.eq(&BV::from_u64(1, 64)),
    });
    state.assert_path(PathCondition {
        order: 2,
        pc: 8,
        disasm: "jne r3, r2".into(),
        taken: false,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: w1.eq(&BV::from_u64(2, 64)),
    });

    let shown = shown_pcs(&state, true, HideFilters::default());
    assert_eq!(shown.len(), 1);
    assert!(shown[0].taken);

    let defs = load_defs_to_show(
        shown.iter().copied(),
        &state.load_defs,
        false,
        HideFilters::default(),
    );
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "w_0");
}

#[test]
fn load_defs_include_transitive_dependencies() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_44".into(),
        expr: BV::from_u64(0, 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_75".into(),
        expr: BV::new_const("w_44", 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_99".into(),
        expr: BV::from_u64(1, 64),
    });
    let w75 = BV::new_const("w_75", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jeq r3, r2".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: w75.eq(&BV::from_u64(1, 64)),
    });

    let shown = load_defs_to_show(
        state.path_conditions.iter(),
        &state.load_defs,
        false,
        HideFilters::default(),
    );
    let names: Vec<&str> = shown.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["w_44", "w_75"]);
}

#[test]
fn hide_num_account_children_unless_needed_by_other_words() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_num_accounts".into(),
        expr: BV::from_u64(2, 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_num_accounts_0".into(),
        expr: BV::new_const("w_num_accounts", 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_num_accounts_1".into(),
        expr: BV::new_const("w_num_accounts_0", 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_data_len".into(),
        expr: BV::new_const("w_num_accounts_0", 64),
    });

    let child = BV::new_const("w_num_accounts_1", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jeq r1, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: child.eq(&BV::from_u64(0, 64)),
    });
    let data_len = BV::new_const("w_acc0_data_len", 64);
    state.assert_path(PathCondition {
        order: 2,
        pc: 8,
        disasm: "jgt r2, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: data_len.eq(&BV::from_u64(0, 64)),
    });

    let shown = load_defs_to_show(
        state.path_conditions.iter(),
        &state.load_defs,
        false,
        HideFilters {
            num_account_children: HideMode::Words,
            ..HideFilters::default()
        },
    );
    let names: Vec<&str> = shown.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["w_num_accounts", "w_num_accounts_0", "w_acc0_data_len"]
    );
    assert!(!names.contains(&"w_num_accounts_1"));
}

#[test]
fn hide_num_account_children_constraints_keeps_parent_and_mixed() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_num_accounts".into(),
        expr: BV::new_const("n_num_accounts_00", 8).zero_ext(56),
    });
    state.load_defs.push(LoadDef {
        name: "w_num_accounts_0".into(),
        expr: BV::new_const("w_num_accounts", 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_data_len".into(),
        expr: BV::new_const("n_acc0_data_len_0", 8).zero_ext(56),
    });

    let parent = BV::new_const("w_num_accounts", 64);
    let child = BV::new_const("w_num_accounts_0", 64);
    let data_len = BV::new_const("w_acc0_data_len", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jne r0, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: parent.eq(&BV::from_u64(0, 64)).not(),
    });
    state.assert_path(PathCondition {
        order: 2,
        pc: 8,
        disasm: "jne r1, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: child.eq(&BV::from_u64(0, 64)).not(),
    });
    state.assert_path(PathCondition {
        order: 3,
        pc: 16,
        disasm: "jle r2, r3".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: data_len.bvule(&child),
    });

    let shown = shown_pcs(
        &state,
        false,
        HideFilters {
            num_account_children: HideMode::Constraints,
            ..HideFilters::default()
        },
    );
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[0].order, 1);
    assert_eq!(shown[1].order, 3);

    let full = shown_pcs(
        &state,
        false,
        HideFilters {
            num_account_children: HideMode::Full,
            ..HideFilters::default()
        },
    );
    assert_eq!(full.len(), 2);
    let defs = load_defs_to_show(
        full.iter().copied(),
        &state.load_defs,
        false,
        HideFilters {
            num_account_children: HideMode::Full,
            ..HideFilters::default()
        },
    );
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    // Formula still mentions the child, but its def is omitted unless another
    // shown word's definition depends on it.
    assert!(!names.contains(&"w_num_accounts_0"));
    assert!(names.contains(&"w_acc0_data_len"));
    assert!(names.contains(&"w_num_accounts"));
}

#[test]
fn hide_data_len_children_keeps_parent_and_mixed() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_acc0_data_len".into(),
        expr: BV::new_const("n_acc0_data_len_0", 8).zero_ext(56),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_data_len_0".into(),
        expr: BV::new_const("w_acc0_data_len", 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc1_data_len".into(),
        expr: BV::new_const("n_acc1_data_len_0", 8).zero_ext(56),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc1_data_len_0".into(),
        expr: BV::new_const("w_acc1_data_len", 64),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_pubkey_0".into(),
        expr: BV::new_const("n_acc0_pubkey_00", 8).zero_ext(56),
    });

    let parent = BV::new_const("w_acc0_data_len", 64);
    let child0 = BV::new_const("w_acc0_data_len_0", 64);
    let child1 = BV::new_const("w_acc1_data_len_0", 64);
    let pubkey = BV::new_const("w_acc0_pubkey_0", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jgt r1, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: parent.eq(&BV::from_u64(0, 64)).not(),
    });
    state.assert_path(PathCondition {
        order: 2,
        pc: 8,
        disasm: "jne r2, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: child0.eq(&BV::from_u64(0, 64)).not(),
    });
    state.assert_path(PathCondition {
        order: 3,
        pc: 16,
        disasm: "jle r2, r3".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: child0.bvule(&pubkey),
    });
    state.assert_path(PathCondition {
        order: 4,
        pc: 24,
        disasm: "jne r4, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: child1.eq(&BV::from_u64(0, 64)).not(),
    });

    let hide0 = HideDataLenChildren::parse("0").unwrap();
    let shown = shown_pcs(
        &state,
        false,
        HideFilters {
            data_len_children: hide0,
            ..HideFilters::default()
        },
    );
    assert_eq!(
        shown.iter().map(|pc| pc.order).collect::<Vec<_>>(),
        vec![1, 3, 4]
    );

    let defs = load_defs_to_show(
        shown.iter().copied(),
        &state.load_defs,
        false,
        HideFilters {
            data_len_children: hide0,
            ..HideFilters::default()
        },
    );
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"w_acc0_data_len"));
    // Mixed constraint still names the acc0 child, so its def stays.
    assert!(names.contains(&"w_acc0_data_len_0"));
    assert!(names.contains(&"w_acc1_data_len_0"));

    let hide_all = HideDataLenChildren::All;
    let all_shown = shown_pcs(
        &state,
        false,
        HideFilters {
            data_len_children: hide_all,
            ..HideFilters::default()
        },
    );
    assert_eq!(
        all_shown.iter().map(|pc| pc.order).collect::<Vec<_>>(),
        vec![1, 3]
    );
    let all_defs = load_defs_to_show(
        all_shown.iter().copied(),
        &state.load_defs,
        false,
        HideFilters {
            data_len_children: hide_all,
            ..HideFilters::default()
        },
    );
    let all_names: Vec<&str> = all_defs.iter().map(|d| d.name.as_str()).collect();
    assert!(all_names.contains(&"w_acc0_data_len_0"));
    assert!(!all_names.contains(&"w_acc1_data_len_0"));
}

#[test]
fn hide_signer_writable_executable_keeps_mixed_and_other_fields() {
    let mut state = Ledger::new();
    state.load_defs.push(LoadDef {
        name: "w_acc0_writable".into(),
        expr: BV::new_const("n_acc0_writable", 8).zero_ext(56),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_signer".into(),
        expr: BV::new_const("n_acc0_signer", 8).zero_ext(56),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_executable".into(),
        expr: BV::new_const("n_acc0_executable", 8).zero_ext(56),
    });
    state.load_defs.push(LoadDef {
        name: "w_acc0_pubkey_0".into(),
        expr: BV::new_const("n_acc0_pubkey_00", 8).zero_ext(56),
    });
    state.load_defs.push(LoadDef {
        name: "w_mix".into(),
        expr: BV::new_const("n_acc0_writable", 8)
            .concat(&BV::new_const("n_acc0_signer", 8))
            .zero_ext(48),
    });

    let writable = BV::new_const("w_acc0_writable", 64);
    let signer = BV::new_const("w_acc0_signer", 64);
    let exec = BV::new_const("w_acc0_executable", 64);
    let pubkey = BV::new_const("w_acc0_pubkey_0", 64);
    let mix = BV::new_const("w_mix", 64);
    state.assert_path(PathCondition {
        order: 1,
        pc: 0,
        disasm: "jne r1, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: writable.eq(&BV::from_u64(0, 64)).not(),
    });
    state.assert_path(PathCondition {
        order: 2,
        pc: 8,
        disasm: "jne r2, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: signer.eq(&BV::from_u64(0, 64)).not(),
    });
    state.assert_path(PathCondition {
        order: 3,
        pc: 16,
        disasm: "jeq r3, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: exec.eq(&BV::from_u64(0, 64)),
    });
    state.assert_path(PathCondition {
        order: 4,
        pc: 24,
        disasm: "jne r4, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: pubkey.eq(&BV::from_u64(0, 64)).not(),
    });
    state.assert_path(PathCondition {
        order: 5,
        pc: 32,
        disasm: "jne r5, 0".into(),
        taken: true,
        origins: Vec::new(),
        rel: crate::parse::RelOp::Eq,
        lhs: 0,
        rhs: 0,
        formula: mix.eq(&BV::from_u64(0, 64)).not(),
    });

    let hide_w = HideFilters {
        writable: HideMode::Full,
        ..HideFilters::default()
    };
    let shown = shown_pcs(&state, false, hide_w);
    assert_eq!(
        shown.iter().map(|pc| pc.order).collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );

    let hide_both = HideFilters {
        signer: HideMode::Full,
        writable: HideMode::Full,
        ..HideFilters::default()
    };
    let both = shown_pcs(&state, false, hide_both);
    assert_eq!(
        both.iter().map(|pc| pc.order).collect::<Vec<_>>(),
        vec![3, 4]
    );

    let defs = load_defs_to_show(both.iter().copied(), &state.load_defs, false, hide_both);
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"w_acc0_executable"));
    assert!(names.contains(&"w_acc0_pubkey_0"));
    assert!(!names.contains(&"w_acc0_writable"));
    assert!(!names.contains(&"w_acc0_signer"));
    assert!(!names.contains(&"w_mix"));
}
