//! `extract[hi:lo]` — a sub-window of the source.

use std::collections::{HashMap, HashSet};

use z3::ast::{Ast, BV};

use crate::rewrite::ast::extract_hi_lo;

use super::bit::{slice_window, BitSlice};
use super::bv_slices;
use super::window::{Step, Window};

pub(super) fn step(w: &mut Window) -> Step {
    let Some((ehi, elo)) = extract_hi_lo(&w.cur) else {
        return Step::Fail;
    };
    let Some(src) = w.cur.nth_child(0) else {
        return Step::Fail;
    };
    w.lo = w.lo.saturating_add(elo);
    w.hi = w.hi.saturating_add(elo);
    if w.hi > ehi {
        return Step::Fail;
    }
    w.cur = src;
    Step::Next
}

pub(super) fn split(
    bv: &BV,
    defs: &HashMap<String, BV>,
    expanding: &mut HashSet<String>,
) -> Option<Vec<BitSlice>> {
    let d = z3::ast::Dynamic::from(bv);
    let (hi, lo) = extract_hi_lo(&d)?;
    let src = d.nth_child(0)?.as_bv()?;
    let inner = bv_slices(&src, defs, expanding)?;
    slice_window(&inner, hi, lo)
}
