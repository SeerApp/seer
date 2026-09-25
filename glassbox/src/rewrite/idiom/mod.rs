//! Catalogue of SBPF software idioms.
//!
//! LLVM/compiler-rt lowers ops the ISA does not have into bit-twiddling.
//! [`recover`] tries each documented detector; first hit wins. No Z3
//! `simplify`. Pack/peel, eval, PDA, and syscall UIFs are not idioms.

use z3::ast::{Ast, Bool, Dynamic, BV};

use std::collections::HashMap;

use crate::astwalk::ast_id;
use crate::grammar::is_recovered_uif;
use crate::state::LoadDef;

use super::ast::{unique_nodes, unique_nodes_at_most, MIN_NODES};
use super::eval::{drop_pack_suffixes, is_pure_byte_pack, mul_operand_roots, sampling_vars};

mod affine;
mod bswap;
mod clz;
mod ctz;
mod f64_exp;
mod f64_mant;
mod f64_mul;
mod f64_sign;
mod fptosi;
mod fptoui;
mod popcnt;
mod rotate;
mod siphash;
mod sitofp;
mod uitofp;
mod umul128;

#[cfg(test)]
pub(crate) use clz::{clz_ite, clz_uif};

/// Recover a known software op, or return `expr` unchanged.
pub(crate) fn recover(expr: &BV) -> BV {
    if expr.as_u64().is_some() || expr.num_children() == 0 {
        return expr.clone();
    }
    // IEEE field extracts are structural and fire at any arity.
    if let Some(v) = f64_exp::from_field(expr) {
        return v;
    }
    if let Some(v) = f64_sign::from_field(expr) {
        return v;
    }
    if let Some(v) = rotate::from_shifts(expr) {
        return v;
    }
    // Sampling walks the whole DAG. Skip the catalogue on compiler-rt blobs.
    // clz_ite / software_uitofp sit under this; SipHash grows past it.
    if unique_nodes_at_most(expr, 512) >= 512 {
        return expr.clone();
    }
    match sampling_operands(expr).as_slice() {
        [x] => {
            if let Some(v) = affine::detect(expr, x) {
                return v;
            }
            if let Some(v) = uitofp::detect(expr, x) {
                return v;
            }
            if let Some(v) = sitofp::detect(expr, x) {
                return v;
            }
            if let Some(v) = fptoui::detect(expr, x) {
                return v;
            }
            if let Some(v) = fptosi::detect(expr, x) {
                return v;
            }
            if let Some(v) = f64_exp::of_int(expr, x) {
                return v;
            }
            if let Some(v) = clz::detect(expr, x) {
                return v;
            }
            if let Some(v) = ctz::detect(expr, x) {
                return v;
            }
            if let Some(v) = popcnt::detect(expr, x) {
                return v;
            }
            if let Some(v) = bswap::detect(expr, x) {
                return v;
            }
            if let Some(v) = rotate::ror_const(expr, x) {
                return v;
            }
            if let Some(v) = f64_mant::detect(expr, x) {
                return v;
            }
            expr.clone()
        }
        [a, b] if unique_nodes(expr) >= MIN_NODES => {
            if let Some(v) = f64_exp::of_mul(expr, a, b) {
                return v;
            }
            if let Some(v) = f64_mul::detect(expr, a, b) {
                return v;
            }
            if let Some(v) = fptoui::of_mul(expr, a, b) {
                return v;
            }
            if let Some(v) = f64_sign::of_mul(expr, a, b) {
                return v;
            }
            if let Some(v) = umul128::detect(expr, a, b) {
                return v;
            }
            if let Some(v) = rotate::ror_var(expr, a, b) {
                return v;
            }
            if let Some(v) = rotate::rol_var(expr, a, b) {
                return v;
            }
            expr.clone()
        }
        [a, b, c] if unique_nodes(expr) >= MIN_NODES => {
            if let Some(v) = siphash::detect3(expr, a, b, c) {
                return v;
            }
            expr.clone()
        }
        [a, b, c, d] if unique_nodes(expr) >= MIN_NODES => {
            if let Some(v) = siphash::detect4(expr, a, b, c, d) {
                return v;
            }
            expr.clone()
        }
        _ => expr.clone(),
    }
}

/// Print-time tag when `expr` samples as a known fragment that is not the
/// recovered UIF (wrap-hi ≠ `umul128_hi`). Never rewrites.
pub(crate) fn near_miss(expr: &BV, defs: &[LoadDef]) -> Option<&'static str> {
    if expr.as_u64().is_some() || expr.num_children() == 0 {
        return None;
    }
    if is_recovered_uif(&expr.decl().name()) {
        return None;
    }
    let expr = inline_byte_packs(expr, defs);
    let muls = drop_pack_suffixes(mul_operand_roots(&expr));
    let ops = if muls.len() == 2 {
        muls
    } else {
        sampling_operands(&expr)
    };
    match ops.as_slice() {
        [a, b] if unique_nodes(&expr) >= MIN_NODES => umul128::near_miss(&expr, a, b),
        _ => None,
    }
}

fn inline_byte_packs(expr: &BV, defs: &[LoadDef]) -> BV {
    if defs.is_empty() {
        return expr.clone();
    }
    let packs: HashMap<&str, &BV> = defs
        .iter()
        .filter(|d| is_pure_byte_pack(&d.expr))
        .map(|d| (d.name.as_str(), &d.expr))
        .collect();
    if packs.is_empty() {
        return expr.clone();
    }
    subst_packs(&Dynamic::from(expr), &packs, &mut HashMap::new())
        .as_bv()
        .unwrap_or_else(|| expr.clone())
}

fn subst_packs(
    d: &Dynamic,
    packs: &HashMap<&str, &BV>,
    memo: &mut HashMap<usize, Dynamic>,
) -> Dynamic {
    let id = ast_id(d);
    if let Some(hit) = memo.get(&id) {
        return hit.clone();
    }
    if d.num_children() == 0 {
        if let Some(bv) = d.as_bv() {
            if let Some(def) = packs.get(bv.decl().name().as_str()) {
                let out = Dynamic::from(*def);
                memo.insert(id, out.clone());
                return out;
            }
        }
        memo.insert(id, d.clone());
        return d.clone();
    }
    let kids: Vec<Dynamic> = (0..d.num_children())
        .filter_map(|i| d.nth_child(i).map(|c| subst_packs(&c, packs, memo)))
        .collect();
    let refs: Vec<&dyn Ast> = kids.iter().map(|k| k as &dyn Ast).collect();
    let out = d.decl().apply(&refs);
    memo.insert(id, out.clone());
    out
}

fn sampling_operands(expr: &BV) -> Vec<BV> {
    let vars = sampling_vars(expr);
    if vars.len() == 1 || vars.len() == 2 {
        return vars;
    }
    let muls = mul_operand_roots(expr);
    if muls.len() == 1 || muls.len() == 2 {
        muls
    } else {
        vars
    }
}

/// Recover a known software predicate, or `None`.
pub(crate) fn recover_bool(formula: &Bool) -> Option<Bool> {
    if formula.as_bool().is_some() || unique_nodes(formula) < MIN_NODES {
        return None;
    }
    match sampling_vars(formula).as_slice() {
        [a, b] if a.get_size() == 64 && b.get_size() == 64 => {
            if let Some(v) = f64_exp::finite_mul(formula, a, b) {
                return Some(v);
            }
            if let Some(v) = f64_mul::nonneg(formula, a, b) {
                return Some(v);
            }
            if let Some(v) = fptoui::gt_one(formula, a, b) {
                return Some(v);
            }
            fptoui::ule_one(formula, a, b)
        }
        [a, b, c] if a.get_size() == 64 && b.get_size() == 64 && c.get_size() == 64 => {
            fptoui::ule_bound(formula, a, b, c)
        }
        _ => None,
    }
}
