//! Pack peel: a BV is a list of bit-windows of opaque roots (high-to-low).
//!
//! [`peel_bv_window`] walks unary wrappers; [`bv_slices`] splits concat,
//! disjoint `|`, and `≪` / `× 2^k`. Flatten rebuilds concat; [`match_aligned_word`]
//! snaps to an existing `w_*`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use z3::DeclKind;
use z3::ast::{Ast, BV, Dynamic};

use crate::astwalk::ast_id;
use crate::rewrite::ast::unique_nodes_at_most;
use crate::state::LoadDef;

mod atom;
mod bit;
mod concat;
mod extract;
mod lshr;
mod or;
mod shl;
mod window;
mod zext;

pub(crate) use bit::BitSlice;

use bit::{rebuild_slices, slice_window, slices_eq};
use window::{Step, Window};

fn is_pack_shape(d: &Dynamic) -> bool {
    matches!(
        d.decl().kind(),
        DeclKind::Concat
            | DeclKind::ZeroExt
            | DeclKind::Extract
            | DeclKind::Bshl
            | DeclKind::Bmul
            | DeclKind::Bor
    )
}

thread_local! {
    static SLICE_MEMO: RefCell<HashMap<usize, Option<Vec<BitSlice>>>> = RefCell::new(HashMap::new());
}

fn clear_slice_memo() {
    SLICE_MEMO.with(|m| m.borrow_mut().clear());
}

pub(crate) fn def_map(defs: &[LoadDef]) -> HashMap<String, BV> {
    defs.iter()
        .map(|d| (d.name.clone(), d.expr.clone()))
        .collect()
}

/// Peel extract / zext / shifts / concat-padding to a window of the inner root.
pub(crate) fn peel_bv_window(d: &Dynamic) -> Option<(Dynamic, u32, u32)> {
    let mut w = Window::of(d)?;
    loop {
        let step = match w.cur.decl().kind() {
            DeclKind::Extract => extract::step(&mut w),
            DeclKind::ZeroExt => zext::step(&mut w),
            DeclKind::Blshr => lshr::step(&mut w),
            DeclKind::Bshl | DeclKind::Bmul => shl::step(&mut w),
            DeclKind::Concat => concat::step(&mut w),
            _ => Step::Stop,
        };
        match step {
            Step::Next => continue,
            Step::Done | Step::Stop => break,
            Step::Fail => return None,
        }
    }
    Some((w.cur, w.lo, w.hi))
}

pub(crate) fn bv_slices(
    bv: &BV,
    defs: &HashMap<String, BV>,
    expanding: &mut HashSet<String>,
) -> Option<Vec<BitSlice>> {
    let id = ast_id(bv);
    if let Some(cached) = SLICE_MEMO.with(|m| m.borrow().get(&id).cloned()) {
        return cached;
    }
    let out = bv_slices_uncached(bv, defs, expanding);
    SLICE_MEMO.with(|m| m.borrow_mut().insert(id, out.clone()));
    out
}

fn bv_slices_uncached(
    bv: &BV,
    defs: &HashMap<String, BV>,
    expanding: &mut HashSet<String>,
) -> Option<Vec<BitSlice>> {
    let d = Dynamic::from(bv);
    if let Some((root, lo, hi)) = peel_bv_window(&d) {
        if ast_id(&root) != ast_id(&d) && hi - lo + 1 == bv.get_size() {
            if let Some(rbv) = root.as_bv() {
                let inner = bv_slices(&rbv, defs, expanding)?;
                return slice_window(&inner, hi, lo);
            }
        }
    }
    match d.decl().kind() {
        DeclKind::Extract => extract::split(bv, defs, expanding),
        DeclKind::Concat => concat::split(bv, defs, expanding),
        DeclKind::ZeroExt => zext::split(bv, defs, expanding),
        DeclKind::Bshl | DeclKind::Bmul => {
            shl::split(bv, defs, expanding).or_else(|| Some(vec![BitSlice::all(bv)]))
        }
        DeclKind::Bor => or::split(bv, defs, expanding).or_else(|| Some(vec![BitSlice::all(bv)])),
        _ if d.num_children() == 0 => atom::peel(bv, defs, expanding),
        _ => Some(vec![BitSlice::all(bv)]),
    }
}

/// Flatten concat/extract/shift-or shuffles to a concat of opaque roots.
/// Opaque ops (add, …) flatten their children so `(pack | …) + w` still peels.
pub(crate) fn flatten_concat_extract(expr: &BV, defs: &HashMap<String, BV>) -> Option<BV> {
    clear_slice_memo();
    flatten_rec(expr, defs, &mut HashMap::new())
}

fn flatten_rec(
    expr: &BV,
    defs: &HashMap<String, BV>,
    memo: &mut HashMap<usize, Option<BV>>,
) -> Option<BV> {
    let id = ast_id(expr);
    if let Some(cached) = memo.get(&id) {
        return cached.clone();
    }
    let out = flatten_uncached(expr, defs, memo);
    memo.insert(id, out.clone());
    out
}

fn flatten_uncached(
    expr: &BV,
    defs: &HashMap<String, BV>,
    memo: &mut HashMap<usize, Option<BV>>,
) -> Option<BV> {
    // Keep named loads. Windows of a name stay windows of that name — do not
    // rebuild them from `def.expr` (that inlines CSE).
    if expr.num_children() == 0 && defs.contains_key(&expr.decl().name()) {
        return None;
    }
    let slices = bv_slices(expr, defs, &mut HashSet::new())?;
    let opaque_self =
        slices.len() == 1 && slices[0].is_full() && ast_id(&slices[0].root) == ast_id(expr);
    if !opaque_self {
        let flat = rebuild_slices(&slices)?;
        if flat.get_size() == expr.get_size() {
            return Some(flat);
        }
    }
    flatten_children(expr, defs, memo)
}

fn flatten_children(
    expr: &BV,
    defs: &HashMap<String, BV>,
    memo: &mut HashMap<usize, Option<BV>>,
) -> Option<BV> {
    let d = Dynamic::from(expr);
    let n = d.num_children();
    if n == 0 {
        return None;
    }
    let mut changed = false;
    let mut kids = Vec::with_capacity(n);
    for i in 0..n {
        let c = d.nth_child(i)?;
        let next = if let Some(bv) = c.as_bv() {
            if !is_pack_shape(&c) {
                c
            } else {
                match flatten_rec(&bv, defs, memo) {
                    Some(f) if ast_id(&f) != ast_id(&bv) => {
                        changed = true;
                        Dynamic::from(&f)
                    }
                    _ => c,
                }
            }
        } else {
            c
        };
        kids.push(next);
    }
    if !changed {
        return None;
    }
    let refs: Vec<&dyn Ast> = kids.iter().map(|k| k as &dyn Ast).collect();
    d.decl().apply(&refs).as_bv()
}

/// Snap `expr` to an existing load-def with the same slice list.
pub(crate) fn match_aligned_word(expr: &BV, defs: &[LoadDef]) -> Option<BV> {
    clear_slice_memo();
    let map = def_map(defs);
    let slices = bv_slices(expr, &map, &mut HashSet::new())?;
    if slices.is_empty() {
        return None;
    }
    for def in defs {
        if def.expr.get_size() != expr.get_size() {
            continue;
        }
        if unique_nodes_at_most(&def.expr, 64) >= 64 {
            continue;
        }
        let Some(ds) = bv_slices(&def.expr, &map, &mut HashSet::new()) else {
            continue;
        };
        if slices_eq(&slices, &ds) {
            return Some(BV::new_const(def.name.as_str(), expr.get_size()));
        }
    }
    if slices.len() == 1 {
        let s = &slices[0];
        if s.is_full() {
            let name = s.root.decl().name();
            if map.contains_key(&name) {
                return Some(s.root.clone());
            }
        }
    }
    let flat = rebuild_slices(&slices)?;
    for def in defs {
        if ast_id(&def.expr) == ast_id(&flat) {
            return Some(BV::new_const(def.name.as_str(), expr.get_size()));
        }
    }
    None
}
