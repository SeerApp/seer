//! Canonical JSON document stored as the glassbox blob.
//!
//! Unfiltered: every recorded path condition (taken and not), full logic
//! formulas, load defs referenced by those conditions. CLI hide / taken-only
//! flags do not apply — filtering is a later read-side concern.

use serde::{Deserialize, Serialize};
use z3::ast::Dynamic;

use crate::parse::RelOp;
use crate::path_condition::NoiseKind;
use crate::state::{LoadDef, PathCondition, SysvarOrigin};

use super::filter::load_defs_to_show;
use super::formula::{fmt_ast, fmt_u64_const};
use super::options::{DisplayOptions, HideFilters};

const VERSION: u32 = 1;

const CANONICAL_OPTS: DisplayOptions = DisplayOptions {
    full: true,
    smt: false,
    all_load_defs: false,
    taken_only: false,
    color: false,
    hide_num_account_children: super::options::HideMode::Off,
    hide_data_len_children: super::options::HideDataLenChildren::Off,
    hide_signer: super::options::HideMode::Off,
    hide_writable: super::options::HideMode::Off,
    hide_executable: super::options::HideMode::Off,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub version: u32,
    pub run: i64,
    pub ix: i64,
    pub steps_applied: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub missing_disasm: usize,
    pub minted: Minted,
    pub skipped: Skipped,
    pub path_conditions: Vec<PathConditionJson>,
    pub load_defs: Vec<LoadDefJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Minted {
    pub text: usize,
    pub input: usize,
    pub load_temps: usize,
    pub memory_cells: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub tautologies: u64,
    pub text_noise: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathConditionJson {
    pub order: u64,
    pub pc: u64,
    pub disasm: String,
    pub taken: bool,
    pub rel: RelJson,
    pub lhs: String,
    pub rhs: String,
    pub formula: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vacuous: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub text_noise: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise: Option<NoiseJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<OriginJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RelJson {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Sgt,
    Sge,
    Slt,
    Sle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NoiseJson {
    NumAccountsChild,
    DataLenChild { account: u32 },
    Provision {
        signer: bool,
        writable: bool,
        executable: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginJson {
    pub syscall: String,
    pub pc: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadDefJson {
    pub name: String,
    pub expr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_miss: Option<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn rel_json(rel: RelOp) -> RelJson {
    match rel {
        RelOp::Eq => RelJson::Eq,
        RelOp::Ne => RelJson::Ne,
        RelOp::Gt => RelJson::Gt,
        RelOp::Ge => RelJson::Ge,
        RelOp::Lt => RelJson::Lt,
        RelOp::Le => RelJson::Le,
        RelOp::Sgt => RelJson::Sgt,
        RelOp::Sge => RelJson::Sge,
        RelOp::Slt => RelJson::Slt,
        RelOp::Sle => RelJson::Sle,
    }
}

fn noise_json(kind: NoiseKind) -> Option<NoiseJson> {
    match kind {
        NoiseKind::None => None,
        NoiseKind::NumAccountsChild => Some(NoiseJson::NumAccountsChild),
        NoiseKind::DataLenChild(account) => Some(NoiseJson::DataLenChild { account }),
        NoiseKind::Provision {
            signer,
            writable,
            executable,
        } => Some(NoiseJson::Provision {
            signer,
            writable,
            executable,
        }),
    }
}

fn origin_json(o: SysvarOrigin) -> OriginJson {
    OriginJson {
        syscall: o.syscall.into(),
        pc: o.pc,
    }
}

fn path_condition_json(pc: &PathCondition, load_defs: &[LoadDef]) -> PathConditionJson {
    PathConditionJson {
        order: pc.order,
        pc: pc.pc,
        disasm: pc.disasm.clone(),
        taken: pc.taken,
        rel: rel_json(pc.rel),
        lhs: fmt_u64_const(pc.lhs, 64),
        rhs: fmt_u64_const(pc.rhs, 64),
        formula: fmt_ast(&Dynamic::from(&pc.formula), CANONICAL_OPTS),
        vacuous: crate::sym::is_tautology(&pc.formula),
        text_noise: crate::ancestry::is_text_without_interesting_input(
            &pc.formula,
            load_defs,
        ),
        noise: noise_json(pc.noise_kind(load_defs)),
        origins: pc.origins.iter().copied().map(origin_json).collect(),
    }
}

pub(crate) fn build(
    run: i64,
    ix: i64,
    steps_applied: usize,
    missing_disasm: usize,
    text_bytes: usize,
    input_bytes: usize,
    memory_cells: usize,
    _skipped_tautologies: u64,
    _skipped_text_noise: u64,
    path_conditions: &[PathCondition],
    load_defs: &[LoadDef],
) -> Report {
    let pcs: Vec<_> = path_conditions
        .iter()
        .map(|pc| path_condition_json(pc, load_defs))
        .collect();

    let shown_defs = load_defs_to_show(
        path_conditions.iter(),
        load_defs,
        false,
        HideFilters::default(),
    );
    let defs = shown_defs
        .into_iter()
        .map(|def| LoadDefJson {
            name: def.name.clone(),
            expr: fmt_ast(&Dynamic::from(&def.expr), CANONICAL_OPTS),
            near_miss: crate::rewrite::near_miss(&def.expr, load_defs).map(str::to_string),
        })
        .collect();

    Report {
        version: VERSION,
        run,
        ix,
        steps_applied,
        missing_disasm,
        minted: Minted {
            text: text_bytes,
            input: input_bytes,
            load_temps: load_defs.len(),
            memory_cells,
        },
        skipped: Skipped {
            tautologies: pcs.iter().filter(|p| p.vacuous).count() as u64,
            text_noise: pcs
                .iter()
                .filter(|p| p.text_noise && !p.vacuous)
                .count() as u64,
        },
        path_conditions: pcs,
        load_defs: defs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SysvarOrigin;
    use z3::ast::BV;

    #[test]
    fn path_condition_json_has_concrete_hex_and_origin() {
        let w = BV::new_const("w_acc3_lamports", 64);
        let pc = PathCondition {
            order: 10,
            pc: 20,
            disasm: "jle r0, r9".into(),
            taken: true,
            rel: RelOp::Le,
            lhs: 3480,
            rhs: 9,
            origins: vec![SysvarOrigin {
                syscall: "sol_get_rent_sysvar",
                pc: 0x100,
            }],
            formula: BV::from_u64(3480, 64).bvule(&w),
        };
        let json = path_condition_json(&pc, &[]);
        let s = serde_json::to_string(&json).unwrap();
        assert!(s.contains("\"rel\":\"le\""), "{s}");
        assert!(s.contains("\"lhs\":\"0xd98\""), "{s}");
        assert!(s.contains("\"rhs\":\"0x9\""), "{s}");
        assert!(s.contains("w_acc3_lamports"), "{s}");
        assert!(s.contains("sol_get_rent_sysvar"), "{s}");
        assert!(s.contains("\"pc\":256"), "{s}");
    }
}
