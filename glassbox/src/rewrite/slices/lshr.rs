//! `bvlshr` by a constant — slide the window toward the high bits of the source.
//! Shift past the end is the 0 constant.

use z3::ast::{Ast, BV, Dynamic};

use super::window::{Step, Window};

pub(super) fn step(w: &mut Window) -> Step {
    let Some(src) = w.cur.nth_child(0) else {
        return Step::Fail;
    };
    let Some(src_bits) = src.as_bv().map(|b| b.get_size()) else {
        return Step::Fail;
    };
    let Some(sh) = w
        .cur
        .nth_child(1)
        .and_then(|c| c.as_bv())
        .and_then(|b| b.as_u64())
        .map(|s| s as u32)
    else {
        return Step::Fail;
    };
    let width = w.width();
    let new_lo = w.lo.saturating_add(sh);
    if new_lo >= src_bits {
        w.cur = Dynamic::from(&BV::from_u64(0, width));
        w.lo = 0;
        w.hi = width - 1;
        return Step::Done;
    }
    w.lo = new_lo;
    w.hi = (w.hi + sh).min(src_bits - 1);
    w.cur = src;
    Step::Next
}

#[cfg(test)]
mod tests {
    use crate::rewrite::pack;
    use crate::rewrite::testing::stored_dword_smear;
    use crate::state::LoadDef;
    use z3::ast::{Ast, BV};

    #[test]
    fn peels_stored_word_back_to_existing_temp() {
        let w = BV::new_const("w_num_accounts_4", 64);
        let defs = [LoadDef {
            name: "w_num_accounts_4".into(),
            expr: w.clone(),
        }];
        let got = pack(&stored_dword_smear(&w), &defs);
        assert_eq!(got.decl().name(), "w_num_accounts_4", "got {got}");
    }
}
