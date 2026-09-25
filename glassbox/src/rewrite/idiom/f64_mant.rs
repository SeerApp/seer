//! `f64_mant` — the 52-bit IEEE mantissa field of `(x as f64)`.
//!
//! After `x ≪ clz(x)` and RNE, bits `[51:0]`. Minted as
//! `extract(uitofp(x), 51, 0)`, zero-extended if the dest is wider.

use z3::ast::BV;

use super::uitofp::SAMPLES;
use crate::rewrite::ast::uif1;
use crate::rewrite::eval::eval_env;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    let w = expr.get_size();
    if w < 52 || w > 64 || x.get_size() != 64 {
        return None;
    }
    let mant = |v: u64| (v as f64).to_bits() & ((1u64 << 52) - 1);
    // 1.0 → mant 0; 1.5 → hidden-bit-cleared 1<<51.
    if eval_env(expr, &[(x, 1)])? != 0 {
        return None;
    }
    if eval_env(expr, &[(x, 3)])? != mant(3) {
        return None;
    }
    for &v in SAMPLES {
        if eval_env(expr, &[(x, v)])? != mant(v) {
            return None;
        }
    }
    let bits = uif1("uitofp", x, 64).extract(51, 0);
    Some(if w == 52 { bits } else { bits.zero_ext(w - 52) })
}

#[cfg(test)]
mod tests {
    use crate::rewrite::testing::software_uitofp;
    use z3::ast::{Ast, BV};
    use z3::DeclKind;

    #[test]
    fn recovers_uitofp_mantissa_extract() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&software_uitofp(&x).extract(51, 0), &[]);
        assert_eq!(got.decl().kind(), DeclKind::Extract);
        let src = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(src.decl().name(), "uitofp");
    }
}
