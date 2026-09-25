//! `concat(high, low)` — high-then-low. A window wholly in one side steps into
//! that side. High zeros on a full window peel like zext padding. A window that
//! straddles both sides is not a single root (split handles that).

use std::collections::{HashMap, HashSet};

use z3::ast::{Ast, BV};

use super::bit::{BitSlice, merge_adjacent_slices};
use super::bv_slices;
use super::window::{Step, Window};

pub(super) fn step(w: &mut Window) -> Step {
    let Some(high) = w.cur.nth_child(0) else {
        return Step::Fail;
    };
    let Some(low) = w.cur.nth_child(1) else {
        return Step::Fail;
    };
    let Some(low_bits) = low.as_bv().map(|b| b.get_size()) else {
        return Step::Fail;
    };
    let Some(cur_bits) = w.cur.as_bv().map(|b| b.get_size()) else {
        return Step::Fail;
    };
    if w.lo == 0 && w.hi + 1 == cur_bits && high.as_bv().and_then(|b| b.as_u64()) == Some(0) {
        let Some(hi) = low_bits.checked_sub(1) else {
            return Step::Fail;
        };
        w.hi = hi;
        w.cur = low;
        return Step::Next;
    }
    if w.hi < low_bits {
        w.cur = low;
        Step::Next
    } else if w.lo >= low_bits {
        w.lo -= low_bits;
        w.hi -= low_bits;
        w.cur = high;
        Step::Next
    } else {
        Step::Fail
    }
}

pub(super) fn split(
    bv: &BV,
    defs: &HashMap<String, BV>,
    expanding: &mut HashSet<String>,
) -> Option<Vec<BitSlice>> {
    let d = z3::ast::Dynamic::from(bv);
    let high = d.nth_child(0)?.as_bv()?;
    let low = d.nth_child(1)?.as_bv()?;
    let mut slices = bv_slices(&high, defs, expanding)?;
    if slices.len() > 32 {
        return None;
    }
    slices.extend(bv_slices(&low, defs, expanding)?);
    if slices.len() > 32 {
        return None;
    }
    Some(merge_adjacent_slices(slices))
}

#[cfg(test)]
mod tests {
    use crate::rewrite::pack;
    use crate::state::LoadDef;
    use z3::ast::{Ast, BV};
    use z3::{FuncDecl, Sort};

    fn pda_word(arg: u64, hi: u32, lo: u32) -> BV {
        FuncDecl::new(
            "uif_sol_create_program_address",
            &[&Sort::bitvector(64)],
            &Sort::bitvector(256),
        )
        .apply(&[&BV::from_u64(arg, 64)])
        .as_bv()
        .unwrap()
        .extract(hi, lo)
    }

    #[test]
    fn collapses_realigned_word_to_existing_temp() {
        let w171 = pda_word(0, 255, 192);
        let w172 = pda_word(0, 191, 128);
        let w177 = w171.extract(55, 0).concat(&w172.extract(63, 56));
        let w186 = w171.extract(63, 56).concat(&w177.extract(63, 8));
        let defs = [
            LoadDef {
                name: "w_171".into(),
                expr: w171.clone(),
            },
            LoadDef {
                name: "w_172".into(),
                expr: w172,
            },
            LoadDef {
                name: "w_177".into(),
                expr: w177,
            },
        ];
        let got = pack(&w186, &defs);
        assert_eq!(got.decl().name(), "w_171", "got {got}");
    }

    #[test]
    fn unaligned_window_is_not_an_aligned_word() {
        let w171 = pda_word(1, 255, 192);
        let w172 = pda_word(1, 191, 128);
        let w177 = w171.extract(55, 0).concat(&w172.extract(63, 56));
        let defs = [
            LoadDef {
                name: "w_171".into(),
                expr: w171,
            },
            LoadDef {
                name: "w_172".into(),
                expr: w172,
            },
        ];
        let got = pack(&w177, &defs);
        assert_ne!(got.decl().name(), "w_171");
        assert_ne!(got.decl().name(), "w_172");
    }
}
