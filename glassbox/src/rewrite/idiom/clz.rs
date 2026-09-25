//! `clz` — count leading zeros.
//!
//! SBPF has no CLZ. compiler-rt smears bits then popcounts the inverse
//! (`testing::swar_clz`). Sample `clz(1)=w-1`, `clz(msb)=0`, `clz(0)=w`,
//! prove against a compact ite, mint `clz(x)`.

use z3::ast::BV;
use z3::SatResult;

use crate::rewrite::ast::{bv_equiv_result, uif1};
use crate::rewrite::eval::eval_at;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    let w = x.get_size();
    if w == 0 || w > 64 || w != expr.get_size() {
        return None;
    }
    if eval_at(expr, x, 1)? != (w as u64 - 1) {
        return None;
    }
    if eval_at(expr, x, 1u64 << (w - 1))? != 0 {
        return None;
    }
    if eval_at(expr, x, 0)? != w as u64 {
        return None;
    }
    for bit in 0..w {
        if eval_at(expr, x, 1u64 << bit)? != (w as u64 - 1 - bit as u64) {
            return None;
        }
    }
    // UIF is not BV-equivalent; prove against the ite reference.
    match bv_equiv_result(expr, &clz_ite(x)) {
        SatResult::Sat => None,
        SatResult::Unsat | SatResult::Unknown => Some(clz_uif(x)),
    }
}

pub(crate) fn clz_ite(x: &BV) -> BV {
    let w = x.get_size();
    let mut acc = BV::from_u64(w as u64, w);
    for clz in (0..w).rev() {
        let bitpos = w - 1 - clz;
        let bit = x.extract(bitpos, bitpos);
        acc = bit
            .eq(&BV::from_u64(1, 1))
            .ite(&BV::from_u64(clz as u64, w), &acc);
    }
    acc
}

pub(crate) fn clz_uif(x: &BV) -> BV {
    uif1("clz", x, x.get_size())
}

#[cfg(test)]
mod tests {
    use super::{clz_ite, detect};
    use crate::rewrite::testing::swar_clz;
    use z3::ast::{Ast, BV};

    #[test]
    fn recovers_clz_from_ite_reference() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&clz_ite(&x), &[]);
        assert_eq!(got.decl().name(), "clz");
        assert_eq!(got.num_children(), 1);
    }

    #[test]
    fn recovers_clz_from_swar() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&swar_clz(&x), &[]);
        assert_eq!(got.decl().name(), "clz");
        assert_eq!(got.num_children(), 1);
    }

    #[test]
    fn detect_rejects_unrelated_unary() {
        let x = BV::new_const("w_1", 64);
        let e = x.bvadd(&BV::from_u64(1, 64));
        assert!(detect(&e, &x).is_none());
    }
}
