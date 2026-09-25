//! `ctz` — count trailing zeros.
//!
//! SBPF has no CTZ. compiler-rt isolates the lowest set bit (`x & -x`)
//! then reuses CLZ. Sample `ctz(1)=0`, `ctz(0)=w`, prove against a compact
//! ite, mint `ctz(x)`.

use z3::ast::BV;
use z3::SatResult;

use crate::rewrite::ast::{bv_equiv_result, uif1};
use crate::rewrite::eval::eval_at;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    let w = x.get_size();
    if w == 0 || w > 64 || w != expr.get_size() {
        return None;
    }
    if eval_at(expr, x, 1)? != 0 {
        return None;
    }
    if eval_at(expr, x, 0)? != w as u64 {
        return None;
    }
    if eval_at(expr, x, 1u64 << (w - 1))? != (w as u64 - 1) {
        return None;
    }
    for bit in 0..w {
        if eval_at(expr, x, 1u64 << bit)? != bit as u64 {
            return None;
        }
    }
    match bv_equiv_result(expr, &ctz_ite(x)) {
        SatResult::Sat => None,
        SatResult::Unsat | SatResult::Unknown => Some(uif1("ctz", x, w)),
    }
}

pub(crate) fn ctz_ite(x: &BV) -> BV {
    let w = x.get_size();
    let mut acc = BV::from_u64(w as u64, w);
    for ctz in (0..w).rev() {
        let bit = x.extract(ctz, ctz);
        acc = bit
            .eq(&BV::from_u64(1, 1))
            .ite(&BV::from_u64(ctz as u64, w), &acc);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::ctz_ite;
    use crate::rewrite::testing::software_ctz;
    use z3::ast::{Ast, BV};

    #[test]
    fn recovers_ctz_from_ite_reference() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&ctz_ite(&x), &[]);
        assert_eq!(got.decl().name(), "ctz");
        assert_eq!(got.num_children(), 1);
    }

    #[test]
    fn recovers_ctz_from_lowest_bit_clz() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&software_ctz(&x), &[]);
        assert_eq!(got.decl().name(), "ctz");
    }
}
