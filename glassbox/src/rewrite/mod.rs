//! Compact a value at the three moments the scratchpad asks.
//!
//! [`pack`] — flatten + realign; numeral trees eval to a constant.
//! [`alu`] — idiom catalogue at the root. [`branch`] — walk + catalogue.
//! No Z3 `simplify`. Display prints whatever those three left behind.
//!
//! Recoveries live in [`idiom`]. Syscall UIFs (`uif_sol_*`) are produced in
//! `syscalls`, not here.

use std::collections::HashMap;

use z3::DeclKind;
use z3::ast::{Ast, BV, Bool, Dynamic};

use crate::astwalk::{ast_id, for_each_app};
use crate::state::LoadDef;

mod ast;
mod eval;
mod idiom;
mod pda;
mod slices;
/// Fixtures and soundness checks. Skip when reading production behaviour.
#[cfg(test)]
mod testing;

pub(crate) use pda::{PdaSyscall, match_pda_word};

use ast::{extract_hi_lo, unique_nodes};
use eval::eval_env;
use idiom::{recover, recover_bool};
use slices::{def_map, match_aligned_word, pack_load};

/// Decl name, child ids after CSE, extract `[hi:lo]` (or `0,0`).
type NodeKey = (String, Vec<usize>, u32, u32);

/// Print-only: catalogue near-miss (e.g. wrap-hi of `umul128_hi`). Not a rewrite.
pub fn near_miss(expr: &BV, defs: &[LoadDef]) -> Option<&'static str> {
    idiom::near_miss(expr, defs)
}

/// Eval a tree that contains no free symbols. Not Z3 `simplify`.
pub(crate) fn eval_numeral(bv: &BV) -> Option<u64> {
    eval_env(bv, &[])
}

fn fold_numerals(expr: &BV) -> Option<BV> {
    eval_numeral(expr).map(|v| BV::from_u64(v, expr.get_size()))
}

fn recovered_uif_app(bv: &BV) -> bool {
    bv.num_children() > 0 && crate::grammar::is_recovered_uif(&bv.decl().name())
}

fn walk_bool(formula: &Bool) -> Bool {
    rewrite_dynamic(&Dynamic::from(formula), &mut HashMap::new())
        .as_bool()
        .unwrap_or_else(|| formula.clone())
}

fn align(expr: BV, defs: &[LoadDef]) -> BV {
    if defs.is_empty() {
        return expr;
    }
    match_aligned_word(&expr, defs).unwrap_or(expr)
}

/// Name a load pack: flatten concat/extract/shift-or shuffles, snap to an
/// existing `w_*`. Numeral trees eval; no catalog, no `simplify`.
pub fn pack(expr: &BV, defs: &[LoadDef]) -> BV {
    if let Some(n) = fold_numerals(expr) {
        return align(n, defs);
    }
    pack_load(expr, defs)
}

/// Idiom catalogue at the root after a register ALU step. No child walk.
/// CSE named subtrees first so operands are `w_*` rather than inlined defs.
/// Numeral trees eval. Flatten stays in [`pack`] — SipHash ALUs hang it.
pub fn alu(expr: &BV, defs: &[LoadDef]) -> BV {
    if let Some(n) = fold_numerals(expr) {
        return n;
    }
    recover(&cse_load_defs(expr, defs))
}

/// Replace any subtree that is already a load-def with that `w_*` name.
/// Match by hash-cons id and by (decl, children, extract window), so a rebuilt
/// `umul128_lo(…)` or `w_260 ÷ 10⁹` still snaps to the named temp.
fn cse_load_defs(expr: &BV, defs: &[LoadDef]) -> BV {
    if defs.is_empty() {
        return expr.clone();
    }
    let mut by_id = HashMap::new();
    let mut by_key = HashMap::new();
    for d in defs {
        if d.expr.as_u64().is_some() {
            continue;
        }
        let dyn_e = Dynamic::from(&d.expr);
        by_id.entry(ast_id(&d.expr)).or_insert(d.name.as_str());
        if dyn_e.nth_child(0).is_some() {
            by_key.entry(node_key(&dyn_e)).or_insert(d.name.as_str());
        }
    }
    cse_dyn(&Dynamic::from(expr), &by_id, &by_key, &mut HashMap::new())
        .as_bv()
        .unwrap_or_else(|| expr.clone())
}

fn node_key(d: &Dynamic) -> NodeKey {
    let (hi, lo) = extract_hi_lo(d).unwrap_or((0, 0));
    let mut kids = Vec::new();
    let mut i = 0;
    while let Some(c) = d.nth_child(i) {
        kids.push(ast_id(&c));
        i += 1;
    }
    (d.decl().name(), kids, hi, lo)
}

fn snap_name(
    out: Dynamic,
    by_id: &HashMap<usize, &str>,
    by_key: &HashMap<NodeKey, &str>,
) -> Dynamic {
    let Some(bv) = out.as_bv() else {
        return out;
    };
    let name = by_id
        .get(&ast_id(&out))
        .copied()
        .or_else(|| by_key.get(&node_key(&out)).copied());
    match name {
        Some(name) if bv.decl().name() != name => {
            Dynamic::from(&BV::new_const(name, bv.get_size()))
        }
        _ => out,
    }
}

fn cse_dyn<'a>(
    d: &Dynamic,
    by_id: &HashMap<usize, &'a str>,
    by_key: &HashMap<NodeKey, &'a str>,
    memo: &mut HashMap<usize, Dynamic>,
) -> Dynamic {
    let id = ast_id(d);
    if let Some(hit) = memo.get(&id) {
        return hit.clone();
    }
    if let Some(&name) = by_id.get(&id) {
        if let Some(bv) = d.as_bv() {
            if bv.num_children() != 0 || bv.decl().name() != name {
                let out = Dynamic::from(&BV::new_const(name, bv.get_size()));
                memo.insert(id, out.clone());
                return out;
            }
        }
    }
    let mut kids = Vec::new();
    let mut i = 0;
    while let Some(c) = d.nth_child(i) {
        kids.push(cse_dyn(&c, by_id, by_key, memo));
        i += 1;
    }
    if kids.is_empty() {
        let out = snap_name(d.clone(), by_id, by_key);
        memo.insert(id, out.clone());
        return out;
    }
    let refs: Vec<&dyn Ast> = kids.iter().map(|k| k as &dyn Ast).collect();
    let out = snap_name(d.decl().apply(&refs), by_id, by_key);
    memo.insert(id, out.clone());
    out
}

/// Recover a path-condition Boolean. Keep `w_*` names unless inlining the
/// load-def dictionary finds a smaller float op (`f64_mul`, `fptoui`, …).
pub fn branch(formula: &Bool, defs: &[LoadDef]) -> Bool {
    let named = walk_bool(formula);
    if defs.is_empty() {
        return named;
    }
    let inlined_root =
        subst_load_defs(&Dynamic::from(formula), &def_map(defs), &mut HashMap::new());
    let inlined = rewrite_dynamic(&inlined_root, &mut HashMap::new())
        .as_bool()
        .unwrap_or_else(|| formula.clone());
    if prefer_inlined_recovery(&inlined, &named) {
        inlined
    } else {
        named
    }
}

fn prefer_inlined_recovery(inlined: &Bool, named: &Bool) -> bool {
    unique_nodes(inlined) < unique_nodes(named)
        && has_recovered_float_uif(inlined)
        && !has_recovered_float_uif(named)
}

fn has_recovered_float_uif(formula: &dyn Ast) -> bool {
    let mut hit = false;
    for_each_app(formula, |_, name| {
        if matches!(
            name,
            "f64_mul" | "fptoui" | "fptosi" | "uitofp" | "sitofp" | "f64_exp"
        ) {
            hit = true;
            return true;
        }
        false
    });
    hit
}

fn subst_load_defs(
    d: &Dynamic,
    defs: &HashMap<String, BV>,
    memo: &mut HashMap<usize, Dynamic>,
) -> Dynamic {
    let id = ast_id(d);
    if let Some(hit) = memo.get(&id) {
        return hit.clone();
    }
    if d.num_children() == 0 {
        if let Some(bv) = d.as_bv() {
            if let Some(def) = defs.get(&bv.decl().name()) {
                if ast_id(&Dynamic::from(def)) != id && recovered_uif_app(def) {
                    let out = subst_load_defs(&Dynamic::from(def), defs, memo);
                    memo.insert(id, out.clone());
                    return out;
                }
            }
        }
        memo.insert(id, d.clone());
        return d.clone();
    }
    let n = d.num_children();
    let kids: Vec<Dynamic> = (0..n)
        .filter_map(|i| d.nth_child(i).map(|c| subst_load_defs(&c, defs, memo)))
        .collect();
    let refs: Vec<&dyn Ast> = kids.iter().map(|k| k as &dyn Ast).collect();
    let out = d.decl().apply(&refs);
    memo.insert(id, out.clone());
    out
}

fn rewrite_dynamic(d: &Dynamic, memo: &mut HashMap<usize, Dynamic>) -> Dynamic {
    let id = ast_id(d);
    if let Some(hit) = memo.get(&id) {
        return hit.clone();
    }
    let n = d.num_children();
    let rebuilt = if n == 0 {
        d.clone()
    } else {
        let kids: Vec<Dynamic> = (0..n)
            .filter_map(|i| d.nth_child(i).map(|c| rewrite_dynamic(&c, memo)))
            .collect();
        let refs: Vec<&dyn Ast> = kids.iter().map(|k| k as &dyn Ast).collect();
        d.decl().apply(&refs)
    };
    let out = if let Some(bv) = rebuilt.as_bv() {
        Dynamic::from(&recover(&bv))
    } else if let Some(b) = rebuilt.as_bool() {
        Dynamic::from(&peel_double_not(recover_bool(&b).unwrap_or(b)))
    } else {
        rebuilt
    };
    memo.insert(id, out.clone());
    out
}

/// `¬¬φ` is `φ`. Children are already rewritten, so one peel per Not is enough.
fn peel_double_not(b: Bool) -> Bool {
    let d = Dynamic::from(&b);
    if d.decl().kind() != DeclKind::Not {
        return b;
    }
    let Some(inner) = d.nth_child(0).and_then(|c| c.as_bool()) else {
        return b;
    };
    let id = Dynamic::from(&inner);
    if id.decl().kind() != DeclKind::Not {
        return b;
    }
    id.nth_child(0)
        .and_then(|c| c.as_bool())
        .unwrap_or(inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rewrite::ast::{uif1, uif2};
    use crate::rewrite::testing::free_bv_consts;
    use crate::state::LoadDef;
    use z3::ast::{Ast, BV};

    #[test]
    fn keeps_load_temp_names_when_inline_does_not_shrink() {
        let x = BV::new_const("w_47", 64);
        let y = BV::new_const("w_39", 64);
        let prod = uif2("f64_mul", &uif1("uitofp", &x, 64), &y, 64);
        let w51 = BV::new_const("w_51", 64);
        let defs = [LoadDef {
            name: "w_51".into(),
            expr: prod,
        }];
        let formula = w51.bvsge(&BV::from_u64(0, 64));
        let got = branch(&formula, &defs);
        let lhs = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(lhs.decl().name(), "w_51");
    }

    #[test]
    fn keeps_num_accounts_child_name_after_rewrite() {
        let parent = BV::new_const("w_num_accounts", 64);
        let child = BV::new_const("w_num_accounts_0", 64);
        let bytes: Vec<BV> = (0..8)
            .map(|i| BV::new_const(format!("n_num_accounts_{i:02}"), 8))
            .collect();
        let mut pack = bytes[7].clone();
        for b in bytes[..7].iter().rev() {
            pack = pack.concat(b);
        }
        let defs = [
            LoadDef {
                name: "w_num_accounts".into(),
                expr: pack,
            },
            LoadDef {
                name: "w_num_accounts_0".into(),
                expr: parent.bvmul(&BV::from_u64(0x30, 64)),
            },
        ];
        let got = branch(&child.bvule(&parent), &defs);
        assert!(
            free_bv_consts(&got)
                .iter()
                .any(|v| v.decl().name() == "w_num_accounts_0"),
            "hide flag needs the child name, got {}",
            got,
        );
    }

    #[test]
    fn peels_double_not_from_untaken_ne() {
        let a = BV::new_const("w_28", 64);
        let b = BV::new_const("w_program_id_2", 64);
        let got = branch(&a.eq(&b).not().not(), &[]);
        assert_eq!(got.decl().kind(), DeclKind::Eq, "got {got}");
        assert_eq!(got.num_children(), 2);
    }
}
