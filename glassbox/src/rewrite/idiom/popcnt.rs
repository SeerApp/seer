//! `popcnt` — population count.
//!
//! SBPF has no POPCNT. compiler-rt uses the Hacker's Delight SWAR reduction
//! (same tail as `testing::swar_clz`). Sample `popcnt(0)=0`, `popcnt(1)=1`,
//! prove against a bit-sum, mint `popcnt(x)`.

use z3::ast::BV;
use z3::SatResult;

use crate::rewrite::ast::{bv_equiv_result, uif1};
use crate::rewrite::eval::eval_at;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    let w = x.get_size();
    if w == 0 || w > 64 || w != expr.get_size() {
        return None;
    }
    if eval_at(expr, x, 0)? != 0 {
        return None;
    }
    if eval_at(expr, x, 1)? != 1 {
        return None;
    }
    if eval_at(expr, x, 3)? != 2 {
        return None;
    }
    let all = if w >= 64 { u64::MAX } else { (1u64 << w) - 1 };
    if eval_at(expr, x, all)? != w as u64 {
        return None;
    }
    for bit in 0..w {
        if eval_at(expr, x, 1u64 << bit)? != 1 {
            return None;
        }
    }
    match bv_equiv_result(expr, &popcnt_sum(x)) {
        SatResult::Sat => None,
        SatResult::Unsat | SatResult::Unknown => Some(uif1("popcnt", x, w)),
    }
}

fn popcnt_sum(x: &BV) -> BV {
    let w = x.get_size();
    let mut acc = BV::from_u64(0, w);
    for i in 0..w {
        acc = acc.bvadd(&x.extract(i, i).zero_ext(w - 1));
    }
    acc
}

#[cfg(test)]
mod tests {
    use crate::rewrite::testing::swar_popcnt;
    use z3::ast::{Ast, BV};

    #[test]
    fn recovers_popcnt_from_swar() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&swar_popcnt(&x), &[]);
        assert_eq!(got.decl().name(), "popcnt");
        assert_eq!(got.num_children(), 1);
    }
}
