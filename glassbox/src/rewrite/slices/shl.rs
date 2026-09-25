//! `≪ k` / `× 2^k` — low bits are zero, source slides up. A window in the
//! padding is the 0 constant; a window wholly in the source maps down by `k`.
//! A full-width shift straddles zeros and data, so [`split`] emits both.

use std::collections::{HashMap, HashSet};

use z3::DeclKind;
use z3::ast::{Ast, BV, Dynamic};

use super::bit::{BitSlice, merge_adjacent_slices, slice_window, slices_width, zero_slice};
use super::bv_slices;
use super::window::{Step, Window};

/// `(src, k)` if this node is `src ≪ k` or `src × 2^k`.
fn shift_src(d: &Dynamic) -> Option<(BV, u32)> {
    match d.decl().kind() {
        DeclKind::Bshl if d.num_children() == 2 => {
            let src = d.nth_child(0)?.as_bv()?;
            let k = d.nth_child(1)?.as_bv()?.as_u64()? as u32;
            Some((src, k))
        }
        DeclKind::Bmul if d.num_children() == 2 => {
            let a = d.nth_child(0)?.as_bv()?;
            let b = d.nth_child(1)?.as_bv()?;
            if let Some(k) = a.as_u64().and_then(power_of_two_shift) {
                return Some((b, k));
            }
            if let Some(k) = b.as_u64().and_then(power_of_two_shift) {
                return Some((a, k));
            }
            None
        }
        _ => None,
    }
}

fn power_of_two_shift(c: u64) -> Option<u32> {
    if c == 0 || (c & (c - 1)) != 0 {
        return None;
    }
    Some(c.trailing_zeros())
}

pub(super) fn step(w: &mut Window) -> Step {
    let Some((src, k)) = shift_src(&w.cur) else {
        return Step::Stop;
    };
    if k == 0 {
        w.cur = Dynamic::from(&src);
        return Step::Next;
    }
    let src_bits = src.get_size();
    if w.hi < k {
        let width = w.width();
        w.cur = Dynamic::from(&BV::from_u64(0, width));
        w.lo = 0;
        w.hi = width - 1;
        return Step::Done;
    }
    if w.lo < k {
        // Straddles the zero pad and the source — not one root.
        return Step::Stop;
    }
    w.lo -= k;
    w.hi -= k;
    if w.hi >= src_bits {
        return Step::Fail;
    }
    w.cur = Dynamic::from(&src);
    Step::Next
}

pub(super) fn split(
    bv: &BV,
    defs: &HashMap<String, BV>,
    expanding: &mut HashSet<String>,
) -> Option<Vec<BitSlice>> {
    let d = Dynamic::from(bv);
    let (src, k) = shift_src(&d)?;
    if k == 0 {
        return bv_slices(&src, defs, expanding);
    }
    let w = bv.get_size();
    if k >= w {
        return Some(vec![zero_slice(w)]);
    }
    let inner = bv_slices(&src, defs, expanding)?;
    // A single full-width opaque (xor/add/UIF/`w_*`) must stay `≪ k`.
    // Flattening it to concat(extract, 0) breaks complementary-shift
    // rotate matching (`from_shifts` needs the same `x` on both sides).
    if inner.len() == 1 && inner[0].is_full() && inner[0].root.get_size() == w {
        return None;
    }
    let src_w = slices_width(&inner);
    // Bits [w-1:k] of the result are src[w-1-k:0]; [k-1:0] are zero.
    let kept = w - k;
    if kept > src_w {
        return None;
    }
    let taken = slice_window(&inner, kept - 1, 0)?;
    let mut out = taken;
    out.push(zero_slice(k));
    Some(merge_adjacent_slices(out))
}

#[cfg(test)]
mod tests {
    use crate::rewrite::pack;
    use z3::ast::{Ast, BV};

    #[test]
    fn peels_shl_to_concat_zero() {
        let n = BV::new_const("n_acc0_data_0", 8);
        let expr = n.zero_ext(56).bvshl(&BV::from_u64(16, 64));
        let got = pack(&expr, &[]);
        assert!(!got.to_string().contains("bvshl"), "got {got}");
        assert_eq!(got.decl().kind(), z3::DeclKind::Concat);
    }

    #[test]
    fn peels_mul_power_of_two_like_shl() {
        let n = BV::new_const("n_acc0_data_0", 8);
        let expr = n.zero_ext(56).bvmul(&BV::from_u64(0x1000000, 64));
        let got = pack(&expr, &[]);
        assert!(!got.to_string().contains("bvmul"), "got {got}");
    }
}
