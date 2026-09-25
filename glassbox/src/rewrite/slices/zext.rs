//! `zero_ext` — high bits are zero. A window in the padding is the 0 constant;
//! a window that straddles the original bits is not a single root. [`split`]
//! of a full zext is high zeros then the source (so disjoint `|` can see them).

use std::collections::{HashMap, HashSet};

use z3::ast::{Ast, BV, Dynamic};

use super::bit::{BitSlice, merge_adjacent_slices, zero_slice};
use super::bv_slices;
use super::window::{Step, Window};

pub(super) fn step(w: &mut Window) -> Step {
    let Some(src) = w.cur.nth_child(0) else {
        return Step::Fail;
    };
    let Some(src_bits) = src.as_bv().map(|b| b.get_size()) else {
        return Step::Fail;
    };
    let Some(cur_bits) = w.cur.as_bv().map(|b| b.get_size()) else {
        return Step::Fail;
    };
    if w.lo == 0 && w.hi + 1 == cur_bits {
        let Some(hi) = src_bits.checked_sub(1) else {
            return Step::Fail;
        };
        w.hi = hi;
    } else if w.lo >= src_bits {
        let width = w.width();
        w.cur = Dynamic::from(&BV::from_u64(0, width));
        w.lo = 0;
        w.hi = width - 1;
        return Step::Done;
    } else if w.hi >= src_bits {
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
    let src = z3::ast::Dynamic::from(bv).nth_child(0)?.as_bv()?;
    let extra = bv.get_size().saturating_sub(src.get_size());
    let inner = bv_slices(&src, defs, expanding)?;
    if extra == 0 {
        return Some(inner);
    }
    let mut out = vec![zero_slice(extra)];
    out.extend(inner);
    Some(merge_adjacent_slices(out))
}

#[cfg(test)]
mod tests {
    use crate::rewrite::pack;
    use crate::rewrite::testing::stored_dword_smear;
    use z3::ast::{Ast, BV};

    #[test]
    fn peels_stored_zext_byte_to_concat_zero() {
        let n = BV::new_const("n_acc0_executable", 8);
        let got = pack(&stored_dword_smear(&n.zero_ext(56)), &[]);
        let high = got.nth_child(0).and_then(|c| c.as_bv()).unwrap();
        let low = got.nth_child(1).and_then(|c| c.as_bv()).unwrap();
        assert_eq!(high.as_u64(), Some(0));
        assert_eq!(low.decl().name(), "n_acc0_executable");
        assert!(!got.to_string().contains("bvlshr"), "got {got}");
    }
}
