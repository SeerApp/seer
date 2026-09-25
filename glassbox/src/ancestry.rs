//! Classify load temps and formulas by Solana input-byte ancestry.
//!
//! Minting uses purity to name `w_num_accounts_{i}`, `w_acc{i}_data_len_{j}`,
//! and flag words. [`crate::path_condition::PathCondition`] uses the same
//! predicates to decide which recorded branches are allocator / provision noise.

use std::collections::HashSet;

use z3::ast::{Ast, BV};

use crate::astwalk::for_each_app;
use crate::grammar::{
    acc_flag_input, data_len_input_account, is_input, is_num_accounts_byte, is_text, is_word,
};

pub use crate::grammar::{AccFlag, data_len_child_account, is_num_accounts_child_name};

/// Definition of a load temporary: `name` stands for `expr` (for display / inspection).
#[derive(Clone)]
pub struct LoadDef {
    pub name: String,
    pub expr: BV,
}

fn absorb_named(
    root: &dyn Ast,
    load_defs: &[LoadDef],
    into: &mut HashSet<String>,
    keep: fn(&str) -> bool,
    seen_w: &mut HashSet<String>,
) {
    let mut newly = Vec::new();
    for_each_app(root, |_, name| {
        if keep(name) {
            into.insert(name.to_string());
        } else if is_word(name) && seen_w.insert(name.to_string()) {
            newly.push(name.to_string());
        }
        false
    });
    for w in newly {
        if let Some(def) = load_defs.iter().find(|d| d.name == w) {
            absorb_named(&def.expr, load_defs, into, keep, seen_w);
        }
    }
}

fn names(root: &dyn Ast, load_defs: &[LoadDef], keep: fn(&str) -> bool) -> HashSet<String> {
    let mut into = HashSet::new();
    let mut seen_w = HashSet::new();
    absorb_named(root, load_defs, &mut into, keep, &mut seen_w);
    into
}

/// `n_*` leaves reachable from `root` through load-def deps.
fn input_names(root: &dyn Ast, load_defs: &[LoadDef]) -> HashSet<String> {
    names(root, load_defs, is_input)
}

/// `t_*` leaves reachable from `root` through load-def deps.
fn text_names(root: &dyn Ast, load_defs: &[LoadDef]) -> HashSet<String> {
    names(root, load_defs, is_text)
}

/// True when `root` transitively mentions text bytes, but every reachable input
/// byte is from `num_accounts` (or there is no input at all). Such constraints
/// are rodata / allocator noise, not input–text interactions.
pub fn is_text_without_interesting_input(root: &dyn Ast, load_defs: &[LoadDef]) -> bool {
    if text_names(root, load_defs).is_empty() {
        return false;
    }
    !input_names(root, load_defs)
        .iter()
        .any(|n| !is_num_accounts_byte(n))
}

/// True when every reachable input byte is from `num_accounts` and the
/// expression mentions at least one `w_num_accounts_{i}` child. Used to hide
/// allocator/pointer-arithmetic path conditions while keeping constraints on
/// `w_num_accounts` itself.
pub fn is_num_accounts_child_only_constraint(root: &dyn Ast, load_defs: &[LoadDef]) -> bool {
    let input = input_names(root, load_defs);
    if input.iter().any(|n| !is_num_accounts_byte(n)) {
        return false;
    }
    let mut seen = HashSet::new();
    absorb_w_from_ast(root, load_defs, &mut seen);
    seen.iter().any(|w| is_num_accounts_child_name(w))
}

pub fn collect_w_names_ast(root: &dyn Ast) -> HashSet<String> {
    let mut names = HashSet::new();
    for_each_app(root, |_, name| {
        if is_word(name) {
            names.insert(name.to_string());
        }
        false
    });
    names
}

fn absorb_w_from_ast(root: &dyn Ast, load_defs: &[LoadDef], seen: &mut HashSet<String>) {
    let mut newly = Vec::new();
    for_each_app(root, |_, name| {
        if is_word(name) && seen.insert(name.to_string()) {
            newly.push(name.to_string());
        }
        false
    });
    for w in newly {
        if let Some(def) = load_defs.iter().find(|d| d.name == w) {
            absorb_w_from_ast(&def.expr, load_defs, seen);
        }
    }
}

pub(crate) fn pack_pure_acc_flag(pack_expr: &BV, load_defs: &[LoadDef]) -> Option<(u32, AccFlag)> {
    pure_acc_flag_from_input(&input_names(pack_expr, load_defs))
}

fn pure_acc_flag_from_input(input: &HashSet<String>) -> Option<(u32, AccFlag)> {
    if input.is_empty() {
        return None;
    }
    let mut found = None;
    for n in input {
        let (i, flag) = acc_flag_input(n)?;
        match found {
            None => found = Some((i, flag)),
            Some((j, g)) if j == i && g == flag => {}
            Some(_) => return None,
        }
    }
    found
}

/// Every reachable input byte is one of the selected account-header flags.
pub fn is_acc_flag_only_constraint_ast(
    root: &dyn Ast,
    load_defs: &[LoadDef],
    hide: impl Fn(AccFlag) -> bool,
) -> bool {
    let input = input_names(root, load_defs);
    if input.is_empty() {
        return false;
    }
    input
        .iter()
        .all(|n| acc_flag_input(n).is_some_and(|(_, flag)| hide(flag)))
}

/// Load-temp whose only input ancestry is selected account-header flags.
pub fn is_acc_flag_only_word(
    expr: &BV,
    load_defs: &[LoadDef],
    hide: impl Fn(AccFlag) -> bool,
) -> bool {
    let input = input_names(expr, load_defs);
    if input.is_empty() {
        return false;
    }
    input
        .iter()
        .all(|n| acc_flag_input(n).is_some_and(|(_, flag)| hide(flag)))
}

pub(crate) fn pack_is_num_accounts_child(pack_expr: &BV, load_defs: &[LoadDef]) -> bool {
    let input = input_names(pack_expr, load_defs);
    !input.is_empty() && input.iter().all(|n| is_num_accounts_byte(n))
}

/// If every reachable input byte is `acc{i}_data_len` for a single `i`, return `i`.
pub(crate) fn pack_pure_data_len_account(pack_expr: &BV, load_defs: &[LoadDef]) -> Option<u32> {
    pure_data_len_from_input(&input_names(pack_expr, load_defs))
}

fn pure_data_len_from_input(input: &HashSet<String>) -> Option<u32> {
    if input.is_empty() {
        return None;
    }
    let mut acc = None;
    for n in input {
        let i = data_len_input_account(n)?;
        match acc {
            None => acc = Some(i),
            Some(j) if j == i => {}
            Some(_) => return None,
        }
    }
    acc
}

/// If the formula is a data-len-child-only constraint, the account index.
pub fn data_len_child_only_account(root: &dyn Ast, load_defs: &[LoadDef]) -> Option<u32> {
    let i = pure_data_len_from_input(&input_names(root, load_defs))?;
    let mut seen = HashSet::new();
    absorb_w_from_ast(root, load_defs, &mut seen);
    seen.iter()
        .any(|w| data_len_child_account(w) == Some(i))
        .then_some(i)
}

/// If every reachable input byte is a provision flag, which flags appear.
pub fn provision_only_flags(root: &dyn Ast, load_defs: &[LoadDef]) -> Option<ProvisionFlags> {
    let input = input_names(root, load_defs);
    if input.is_empty() {
        return None;
    }
    let mut flags = ProvisionFlags::default();
    for n in &input {
        let (_, flag) = acc_flag_input(n)?;
        match flag {
            AccFlag::Signer => flags.signer = true,
            AccFlag::Writable => flags.writable = true,
            AccFlag::Executable => flags.executable = true,
        }
    }
    Some(flags)
}

/// Set of account-header flags that appear in a provision-only formula.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProvisionFlags {
    pub signer: bool,
    pub writable: bool,
    pub executable: bool,
}

impl ProvisionFlags {
    pub fn count(self) -> u8 {
        self.signer as u8 + self.writable as u8 + self.executable as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_names_do_not_match_n_inside_data_len_name() {
        let empty = [];
        let w = BV::new_const("w_acc0_data_len_0", 64);
        let eq = w.eq(&BV::from_u64(1, 64));
        assert!(input_names(&eq, &empty).is_empty());
        assert!(text_names(&eq, &empty).is_empty());

        let n = BV::new_const("n_acc0_data_len_0", 8).zero_ext(56);
        let input = input_names(&w.bvadd(&n), &empty);
        assert_eq!(input.len(), 1);
        assert!(input.contains("n_acc0_data_len_0"));
    }

    #[test]
    fn text_without_interesting_input_detects_rodata_noise() {
        let defs = vec![
            LoadDef {
                name: "w_text".into(),
                expr: BV::new_const("t_0000000001", 8).zero_ext(56),
            },
            LoadDef {
                name: "w_num_accounts".into(),
                expr: BV::new_const("n_num_accounts_00", 8).zero_ext(56),
            },
            LoadDef {
                name: "w_acc".into(),
                expr: BV::new_const("n_acc0_dup", 8).zero_ext(56),
            },
        ];
        let zero = BV::from_u64(0, 64);
        let w_text = BV::new_const("w_text", 64);
        let w_num = BV::new_const("w_num_accounts", 64);
        let w_acc = BV::new_const("w_acc", 64);
        assert!(is_text_without_interesting_input(&w_text.eq(&zero), &defs));
        assert!(is_text_without_interesting_input(
            &w_text.bvadd(&w_num).eq(&zero),
            &defs
        ));
        assert!(!is_text_without_interesting_input(&w_acc.eq(&zero), &defs));
        assert!(!is_text_without_interesting_input(
            &w_text.eq(&w_acc),
            &defs
        ));
    }

    #[test]
    fn num_accounts_child_only_constraint_keeps_parent_count() {
        let defs = vec![
            LoadDef {
                name: "w_num_accounts".into(),
                expr: BV::new_const("n_num_accounts_00", 8).zero_ext(56),
            },
            LoadDef {
                name: "w_num_accounts_0".into(),
                expr: BV::new_const("w_num_accounts", 64),
            },
            LoadDef {
                name: "w_acc0_data_len".into(),
                expr: BV::new_const("n_acc0_data_len_0", 8).zero_ext(56),
            },
        ];
        let zero = BV::from_u64(0, 64);
        let w_num = BV::new_const("w_num_accounts", 64);
        let w_child = BV::new_const("w_num_accounts_0", 64);
        let w_dl = BV::new_const("w_acc0_data_len", 64);
        assert!(!is_num_accounts_child_only_constraint(
            &w_num.eq(&zero).not(),
            &defs
        ));
        assert!(is_num_accounts_child_only_constraint(
            &w_child.eq(&zero).not(),
            &defs
        ));
        assert!(!is_num_accounts_child_only_constraint(
            &w_dl.bvule(&w_child),
            &defs
        ));
    }

    #[test]
    fn data_len_child_only_account_keeps_parent_and_mixed() {
        let defs = vec![
            LoadDef {
                name: "w_acc0_data_len".into(),
                expr: BV::new_const("n_acc0_data_len_0", 8).zero_ext(56),
            },
            LoadDef {
                name: "w_acc0_data_len_0".into(),
                expr: BV::new_const("w_acc0_data_len", 64),
            },
            LoadDef {
                name: "w_acc1_data_len".into(),
                expr: BV::new_const("n_acc1_data_len_0", 8).zero_ext(56),
            },
            LoadDef {
                name: "w_acc0_pubkey_0".into(),
                expr: BV::new_const("n_acc0_pubkey_00", 8).zero_ext(56),
            },
        ];
        let zero = BV::from_u64(0, 64);
        let parent = BV::new_const("w_acc0_data_len", 64);
        let child = BV::new_const("w_acc0_data_len_0", 64);
        let peer = BV::new_const("w_acc1_data_len", 64);
        let pubkey = BV::new_const("w_acc0_pubkey_0", 64);
        assert_eq!(
            data_len_child_only_account(&parent.bvule(&BV::from_u64(0x2800, 64)), &defs),
            None
        );
        assert_eq!(
            data_len_child_only_account(&child.eq(&zero).not(), &defs),
            Some(0)
        );
        assert_eq!(
            data_len_child_only_account(&child.bvule(&pubkey), &defs),
            None
        );
        assert_eq!(
            data_len_child_only_account(&child.bvadd(&peer).eq(&zero), &defs),
            None
        );
    }

    #[test]
    fn pack_pure_names_a_single_flag_or_data_len_family() {
        let empty = [];
        let signer = BV::new_const("n_acc0_signer", 8).zero_ext(56);
        assert_eq!(
            pack_pure_acc_flag(&signer, &empty),
            Some((0, AccFlag::Signer))
        );
        let mixed = signer.bvadd(&BV::new_const("n_acc0_writable", 8).zero_ext(56));
        assert_eq!(pack_pure_acc_flag(&mixed, &empty), None);

        let dl = BV::new_const("n_acc3_data_len_0", 8).zero_ext(56);
        assert_eq!(pack_pure_data_len_account(&dl, &empty), Some(3));
        assert_eq!(
            pack_pure_data_len_account(
                &dl.bvadd(&BV::new_const("n_acc0_pubkey_00", 8).zero_ext(56)),
                &empty
            ),
            None
        );
    }

    #[test]
    fn pack_num_accounts_child_is_any_pack_of_only_count_bytes() {
        let empty = [];
        let count = BV::new_const("n_num_accounts_00", 8).zero_ext(56);
        assert!(pack_is_num_accounts_child(&count, &empty));
        // Mixed with a real account byte is not allocator-only.
        assert!(!pack_is_num_accounts_child(
            &count.bvadd(&BV::new_const("n_acc0_dup", 8).zero_ext(56)),
            &empty
        ));
    }

    #[test]
    fn input_names_chase_load_defs_collect_w_names_do_not() {
        let n = BV::new_const("n_acc0_signer", 8).zero_ext(56);
        let defs = [LoadDef {
            name: "w_acc0_signer".into(),
            expr: n.clone(),
        }];
        let w = BV::new_const("w_acc0_signer", 64);
        assert!(input_names(&w, &defs).contains("n_acc0_signer"));
        let names = collect_w_names_ast(&w);
        assert_eq!(names.len(), 1);
        assert!(names.contains("w_acc0_signer"));
        assert!(is_acc_flag_only_word(&n, &defs, |f| f == AccFlag::Signer));
        assert!(!is_acc_flag_only_word(&n, &defs, |f| f == AccFlag::Writable));
    }
}
