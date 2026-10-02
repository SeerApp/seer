use std::collections::{HashMap, HashSet};

use z3::ast::{Ast, Dynamic, BV};
use z3::DeclKind;

use crate::astwalk::{ast_id, for_each_app};
use crate::grammar::is_recovered_uif;

use super::super::ast::extract_hi_lo;
use super::mask_width;

/// 64-bit inputs we can sample after inlining: named temps, `uitofp`/`sitofp`
/// results, 64-bit extracts of opaque UIFs (sysvars), and packed byte concats.
pub(crate) fn is_sampling_atom(bv: &BV) -> bool {
    if bv.get_size() != 64 || bv.as_u64().is_some() {
        return false;
    }
    let d = Dynamic::from(bv);
    match d.decl().kind() {
        DeclKind::Bnum => false,
        _ if d.num_children() == 0 => true,
        DeclKind::Extract => d.nth_child(0).is_some_and(|src| {
            src.decl().kind() == DeclKind::Uninterpreted && !is_recovered_uif(&src.decl().name())
        }),
        DeclKind::Uninterpreted => {
            is_recovered_uif(&d.decl().name())
                || (matches!(d.decl().name().as_str(), "uitofp" | "sitofp")
                    && d.nth_child(0).is_some_and(|c| c.num_children() != 0))
        }
        DeclKind::Concat => is_pure_byte_pack(bv),
        _ => false,
    }
}

/// Packed `n_acc*_lamports_*` (and similar) — concat/zext of sub-64 consts only.
pub(crate) fn is_pure_byte_pack(bv: &BV) -> bool {
    if bv.get_size() != 64 {
        return false;
    }
    let mut pure = true;
    for_each_app(bv, |node, _| {
        if node.as_bv().and_then(|b| b.as_u64()).is_some() {
            return false;
        }
        match node.decl().kind() {
            DeclKind::Concat | DeclKind::ZeroExt => false,
            _ if node.num_children() == 0 && node.as_bv().is_some_and(|c| c.get_size() < 64) => {
                false
            }
            _ => {
                pure = false;
                true
            }
        }
    });
    pure
}

pub(crate) fn sampling_vars(expr: &dyn Ast) -> Vec<BV> {
    let mut cands = Vec::new();
    let mut seen = HashSet::new();
    for_each_app(expr, |node, _| {
        if let Some(bv) = node.as_bv() {
            if is_sampling_atom(&bv) && seen.insert(ast_id(node)) {
                cands.push(bv);
            }
        }
        false
    });
    drop_nested(cands)
}

fn drop_nested(cands: Vec<BV>) -> Vec<BV> {
    let mut nested = HashSet::new();
    for c in &cands {
        for_each_app(c, |node, _| {
            let id = ast_id(node);
            if id != ast_id(c) {
                nested.insert(id);
            }
            false
        });
    }
    cands
        .into_iter()
        .filter(|c| !nested.contains(&ast_id(c)))
        .collect()
}

/// 64-bit sources of `×` after unwrapping compiler-rt `lo32`/`hi32` limbs.
pub(crate) fn mul_operand_roots(expr: &dyn Ast) -> Vec<BV> {
    let mut cands = Vec::new();
    let mut seen = HashSet::new();
    for_each_app(expr, |node, _| {
        if node.decl().kind() == DeclKind::Bmul && node.num_children() == 2 {
            let a = node.nth_child(0).and_then(|c| c.as_bv());
            let b = node.nth_child(1).and_then(|c| c.as_bv());
            if a.as_ref().and_then(BV::as_u64) == Some(0)
                || b.as_ref().and_then(BV::as_u64) == Some(0)
            {
                return false;
            }
            for ch in [a, b].into_iter().flatten() {
                let src = unwrap_limb_operand(&ch);
                if src.get_size() == 64 && src.as_u64().is_none() && seen.insert(ast_id(&src)) {
                    cands.push(src);
                }
            }
        }
        false
    });
    drop_nested(cands)
}

/// Drop a limb rebuilt from a suffix of another pack (`lo32` from the low
/// four bytes of an 8-byte qword). Near-miss only — not used by recover.
pub(crate) fn drop_pack_suffixes(cands: Vec<BV>) -> Vec<BV> {
    let leaves: Vec<Option<Vec<usize>>> = cands.iter().map(pack_named_bytes).collect();
    let mut drop = HashSet::new();
    for i in 0..cands.len() {
        let Some(a) = leaves[i].as_ref().filter(|a| !a.is_empty()) else {
            continue;
        };
        for j in 0..cands.len() {
            if i == j {
                continue;
            }
            let Some(b) = leaves[j].as_ref() else {
                continue;
            };
            if a == b {
                if i > j {
                    drop.insert(i);
                }
            } else if b.ends_with(a) {
                drop.insert(i);
            }
        }
    }
    cands
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(_, c)| c)
        .collect()
}

/// Named sub-64 concat leaves, high-to-low. Numerals skipped.
fn pack_named_bytes(bv: &BV) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    if !collect_named_bytes(&Dynamic::from(bv), &mut out) {
        return None;
    }
    Some(out)
}

fn collect_named_bytes(d: &Dynamic, out: &mut Vec<usize>) -> bool {
    let Some(bv) = d.as_bv() else {
        return false;
    };
    if bv.as_u64().is_some() {
        return true;
    }
    match d.decl().kind() {
        DeclKind::Concat => {
            for i in 0..d.num_children() {
                if !d.nth_child(i).is_some_and(|c| collect_named_bytes(&c, out)) {
                    return false;
                }
            }
            true
        }
        DeclKind::ZeroExt => d.nth_child(0).is_some_and(|c| collect_named_bytes(&c, out)),
        DeclKind::Extract => d
            .nth_child(0)
            .is_some_and(|src| src.as_bv().and_then(|b| b.as_u64()).is_some()),
        _ if d.num_children() == 0 && bv.get_size() < 64 => {
            out.push(ast_id(d));
            true
        }
        _ => false,
    }
}

pub(crate) fn bind_pack_bytes(d: &Dynamic, val: u64, by_id: &mut HashMap<usize, u64>) {
    let mut bit = 0u32;
    bind_pack_bytes_from_low(d, val, &mut bit, by_id);
}

fn bind_pack_bytes_from_low(d: &Dynamic, val: u64, bit: &mut u32, by_id: &mut HashMap<usize, u64>) {
    let Some(bv) = d.as_bv() else {
        return;
    };
    if bv.as_u64().is_some() {
        *bit += bv.get_size();
        return;
    }
    match d.decl().kind() {
        DeclKind::Concat => {
            for i in (0..d.num_children()).rev() {
                if let Some(c) = d.nth_child(i) {
                    bind_pack_bytes_from_low(&c, val, bit, by_id);
                }
            }
        }
        DeclKind::ZeroExt => {
            if let Some(c) = d.nth_child(0) {
                bind_pack_bytes_from_low(&c, val, bit, by_id);
            }
        }
        DeclKind::Extract => {
            *bit += bv.get_size();
        }
        _ if d.num_children() == 0 && bv.get_size() < 64 => {
            let w = bv.get_size();
            by_id.insert(ast_id(d), mask_width(val >> *bit, w));
            *bit += w;
        }
        _ => {}
    }
}

fn unwrap_limb_operand(ch: &BV) -> BV {
    unwrap_shift32(ch)
        .or_else(|| unwrap_zext32(ch))
        .unwrap_or_else(|| ch.clone())
}

/// `(src ≫ 32)` or `(concat(src[31:0], 0) ≫ 32)`.
fn unwrap_shift32(ch: &BV) -> Option<BV> {
    let d = Dynamic::from(ch);
    if d.decl().kind() != DeclKind::Blshr {
        return None;
    }
    let src = d.nth_child(0)?.as_bv()?;
    let k = d.nth_child(1)?.as_bv()?.as_u64()?;
    if src.get_size() != 64 || k != 32 {
        return None;
    }
    Some(unwrap_concat_lo32(&src).unwrap_or(src))
}

fn unwrap_concat_lo32(src: &BV) -> Option<BV> {
    let d = Dynamic::from(src);
    if d.decl().kind() != DeclKind::Concat {
        return None;
    }
    let high = d.nth_child(0)?.as_bv()?;
    let low = d.nth_child(1)?.as_bv()?;
    if low.get_size() != 32 || low.as_u64() != Some(0) || high.get_size() != 32 {
        return None;
    }
    let hd = Dynamic::from(&high);
    let (hi, lo) = extract_hi_lo(&hd)?;
    if hi - lo + 1 != 32 {
        return None;
    }
    let word = hd.nth_child(0)?.as_bv()?;
    if word.get_size() != 64 {
        return None;
    }
    if lo == 0 {
        Some(word)
    } else {
        Some(word.bvlshr(&BV::from_u64(lo as u64, 64)))
    }
}

fn unwrap_zext32(ch: &BV) -> Option<BV> {
    let d = Dynamic::from(ch);
    if d.decl().kind() != DeclKind::ZeroExt {
        return None;
    }
    let src = d.nth_child(0)?.as_bv()?;
    if src.get_size() != 32 || ch.get_size() != 64 {
        return None;
    }
    let sd = Dynamic::from(&src);
    let (hi, lo) = extract_hi_lo(&sd)?;
    if hi - lo + 1 != 32 {
        return None;
    }
    let word = sd.nth_child(0)?.as_bv()?;
    if word.get_size() != 64 {
        return None;
    }
    if lo == 0 {
        Some(word)
    } else {
        Some(word.bvlshr(&BV::from_u64(lo as u64, 64)))
    }
}
