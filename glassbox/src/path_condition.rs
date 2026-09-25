//! Recorded symbolic branch, and whether a view should keep it.

use z3::ast::Bool;

use crate::ancestry::{
    LoadDef, data_len_child_only_account, is_num_accounts_child_only_constraint,
    is_text_without_interesting_input, provision_only_flags,
};
use crate::parse::RelOp;
use crate::sym::is_tautology;

/// Concrete sysvar bytes observed at a syscall, identified by producer PC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SysvarOrigin {
    pub syscall: &'static str,
    pub pc: u64,
}

pub(crate) fn merge_origins(into: &mut Vec<SysvarOrigin>, extra: &[SysvarOrigin]) {
    for &o in extra {
        if !into.contains(&o) {
            into.push(o);
        }
    }
}

/// A path condition recorded when a conditional jump depends on tracked symbols.
#[derive(Debug, Clone)]
pub struct PathCondition {
    pub order: u64,
    pub pc: u64,
    pub disasm: String,
    /// Whether the branch was taken in the concrete trace.
    pub taken: bool,
    /// CPU comparison at this jump (`dst` vs src/imm).
    pub rel: RelOp,
    pub lhs: u64,
    pub rhs: u64,
    pub formula: Bool,
    /// Syscalls whose concrete bytes this formula used (blame, not free vars).
    pub origins: Vec<SysvarOrigin>,
}

/// How [`PathCondition::classify`] buckets a candidate before it is recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathClass {
    Tautology,
    TextNoise,
    Keep,
}

/// Why a recorded path condition is (or is not) allocator / provision noise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseKind {
    /// Mixed with real input, or not a hideable family.
    None,
    /// Derived `w_num_accounts_{i}` pointer arithmetic only.
    NumAccountsChild,
    /// Derived `w_acc{i}_data_len_{j}` only.
    DataLenChild(u32),
    /// Only account-header provision flags.
    Provision {
        signer: bool,
        writable: bool,
        executable: bool,
    },
}

/// Constraint-side hide policy. Word stripping stays in analysis.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstraintHide {
    pub num_accounts_children: bool,
    pub data_len_all: bool,
    /// Bit `i` set ⇒ hide `acc{i}` data-len children. Ignored when `data_len_all`.
    pub data_len_mask: u128,
    pub signer: bool,
    pub writable: bool,
    pub executable: bool,
}

impl ConstraintHide {
    fn hides_data_len(self, account: u32) -> bool {
        if self.data_len_all {
            return true;
        }
        account < 128 && (self.data_len_mask & (1u128 << account)) != 0
    }

    fn hides_provision(self, signer: bool, writable: bool, executable: bool) -> bool {
        // Drop iff every flag that appears is selected for hiding.
        (!signer || self.signer) && (!writable || self.writable) && (!executable || self.executable)
    }
}

impl PathCondition {
    /// Whether this formula is a tautology, text-only noise, or worth recording.
    pub fn classify(&self, load_defs: &[LoadDef]) -> PathClass {
        if is_tautology(&self.formula) {
            PathClass::Tautology
        } else if is_text_without_interesting_input(&self.formula, load_defs) {
            PathClass::TextNoise
        } else {
            PathClass::Keep
        }
    }

    /// Allocator / provision noise family, independent of any hide mask.
    pub fn noise_kind(&self, load_defs: &[LoadDef]) -> NoiseKind {
        if is_num_accounts_child_only_constraint(&self.formula, load_defs) {
            return NoiseKind::NumAccountsChild;
        }
        if let Some(i) = data_len_child_only_account(&self.formula, load_defs) {
            return NoiseKind::DataLenChild(i);
        }
        if let Some(flags) = provision_only_flags(&self.formula, load_defs) {
            return NoiseKind::Provision {
                signer: flags.signer,
                writable: flags.writable,
                executable: flags.executable,
            };
        }
        NoiseKind::None
    }

    /// Keep this PC in the printed view?
    pub fn visible(&self, load_defs: &[LoadDef], taken_only: bool, hide: ConstraintHide) -> bool {
        if taken_only && !self.taken {
            return false;
        }
        !self.noise_kind(load_defs).hidden_by(hide)
    }
}

impl NoiseKind {
    fn hidden_by(self, hide: ConstraintHide) -> bool {
        match self {
            NoiseKind::None => false,
            NoiseKind::NumAccountsChild => hide.num_accounts_children,
            NoiseKind::DataLenChild(i) => hide.hides_data_len(i),
            NoiseKind::Provision {
                signer,
                writable,
                executable,
            } => hide.hides_provision(signer, writable, executable),
        }
    }

    #[allow(dead_code)]
    fn hide_tally(self) -> Option<HideTally> {
        match self {
            NoiseKind::None => None,
            NoiseKind::NumAccountsChild => Some(HideTally::NumAccountsChild),
            NoiseKind::DataLenChild(_) => Some(HideTally::DataLenChild),
            NoiseKind::Provision {
                signer,
                writable,
                executable,
            } => {
                let n = signer as u8 + writable as u8 + executable as u8;
                if n > 1 {
                    Some(HideTally::FlagMix)
                } else if signer {
                    Some(HideTally::Signer)
                } else if writable {
                    Some(HideTally::Writable)
                } else {
                    Some(HideTally::Executable)
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum HideTally {
    NumAccountsChild,
    DataLenChild,
    Signer,
    Writable,
    Executable,
    FlagMix,
}

/// One walk: kept PCs plus per-family hide counts (relative to `taken_only`).
#[allow(dead_code)]
pub(crate) struct View<'a> {
    pub path_conditions: Vec<&'a PathCondition>,
    pub hidden_na_children: usize,
    pub hidden_dl_children: usize,
    pub hidden_signer: usize,
    pub hidden_writable: usize,
    pub hidden_executable: usize,
    pub hidden_flag_mix: usize,
}

impl<'a> View<'a> {
    #[allow(dead_code)]
    pub(crate) fn apply(
        path_conditions: &'a [PathCondition],
        load_defs: &[LoadDef],
        taken_only: bool,
        hide: ConstraintHide,
    ) -> Self {
        let mut view = View {
            path_conditions: Vec::new(),
            hidden_na_children: 0,
            hidden_dl_children: 0,
            hidden_signer: 0,
            hidden_writable: 0,
            hidden_executable: 0,
            hidden_flag_mix: 0,
        };
        for pc in path_conditions {
            if taken_only && !pc.taken {
                continue;
            }
            if pc.visible(load_defs, false, hide) {
                view.path_conditions.push(pc);
                continue;
            }
            match pc.noise_kind(load_defs).hide_tally() {
                Some(HideTally::NumAccountsChild) => view.hidden_na_children += 1,
                Some(HideTally::DataLenChild) => view.hidden_dl_children += 1,
                Some(HideTally::Signer) => view.hidden_signer += 1,
                Some(HideTally::Writable) => view.hidden_writable += 1,
                Some(HideTally::Executable) => view.hidden_executable += 1,
                Some(HideTally::FlagMix) => view.hidden_flag_mix += 1,
                None => {}
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ancestry::LoadDef;
    use z3::ast::BV;

    fn pc(formula: Bool) -> PathCondition {
        PathCondition {
            order: 1,
            pc: 0,
            disasm: "jne r0, 0".into(),
            taken: true,
            rel: RelOp::Ne,
            lhs: 0,
            rhs: 0,
            origins: Vec::new(),
            formula,
        }
    }

    #[test]
    fn noise_kind_num_accounts_child_not_parent() {
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
        let parent = pc(BV::new_const("w_num_accounts", 64).eq(&zero).not());
        let child = pc(BV::new_const("w_num_accounts_0", 64).eq(&zero).not());
        let mixed =
            pc(BV::new_const("w_acc0_data_len", 64).bvule(&BV::new_const("w_num_accounts_0", 64)));
        assert_eq!(parent.noise_kind(&defs), NoiseKind::None);
        assert_eq!(child.noise_kind(&defs), NoiseKind::NumAccountsChild);
        assert_eq!(mixed.noise_kind(&defs), NoiseKind::None);
    }

    #[test]
    fn visible_taken_only_and_constraint_hide() {
        let defs = vec![LoadDef {
            name: "w_num_accounts_0".into(),
            expr: BV::new_const("n_num_accounts_00", 8).zero_ext(56),
        }];
        let mut not_taken = pc(BV::new_const("w_num_accounts_0", 64).eq(&BV::from_u64(0, 64)));
        not_taken.taken = false;
        let hide = ConstraintHide {
            num_accounts_children: true,
            ..ConstraintHide::default()
        };
        assert!(!not_taken.visible(&defs, true, ConstraintHide::default()));
        assert!(not_taken.visible(&defs, false, ConstraintHide::default()));
        assert!(!not_taken.visible(&defs, false, hide));
    }
}
