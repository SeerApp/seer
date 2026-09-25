//! `f64_mul` — software IEEE-754 multiply.
//!
//! compiler-rt unpacks mantissa/exp, multiplies, re-packs. Sample against
//! `fa * fb` and mint `f64_mul`, wrapping an operand in `uitofp` when that
//! operand is a raw integer. Predicate: product `≥ 0` (sign bit clear).

use z3::ast::{Bool, BV};

use super::uitofp::{as_float, float_bits};
use crate::rewrite::ast::uif2;
use crate::rewrite::eval::{
    binary_samples, f64_mul_bits, samples_match_binary, samples_match_bool,
};

pub(crate) fn detect(expr: &BV, a: &BV, b: &BV) -> Option<BV> {
    if expr.get_size() != 64 || a.get_size() != 64 || b.get_size() != 64 {
        return None;
    }
    let pairs = binary_samples();
    for (a_int, b_int) in [(true, false), (false, true), (false, false), (true, true)] {
        if samples_match_binary(expr, a, b, &pairs, |va, vb| {
            f64_mul_bits(float_bits(va, a_int), float_bits(vb, b_int))
        }) {
            return Some(uif2(
                "f64_mul",
                &as_float(a, a_int),
                &as_float(b, b_int),
                64,
            ));
        }
    }
    None
}

pub(crate) fn nonneg(formula: &Bool, a: &BV, b: &BV) -> Option<Bool> {
    let pairs = binary_samples();
    for (a_int, b_int) in [(true, false), (false, true), (false, false)] {
        if samples_match_bool(formula, a, b, &pairs, |va, vb| {
            f64_mul_bits(float_bits(va, a_int), float_bits(vb, b_int)).map(|p| (p as i64) >= 0)
        }) {
            return Some(
                uif2("f64_mul", &as_float(a, a_int), &as_float(b, b_int), 64)
                    .bvsge(&BV::from_u64(0, 64)),
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::rewrite::ast::{uif1, uif2};
    use crate::rewrite::testing::rent_sysvar_halves;
    use z3::ast::{Ast, BV};
    use z3::DeclKind;

    #[test]
    fn recovers_f64_mul_ge_zero() {
        let x = BV::new_const("w_47", 64);
        let y = BV::new_const("w_39", 64);
        let prod = uif2("f64_mul", &uif1("uitofp", &x, 64), &y, 64);
        let got = crate::rewrite::branch(&prod.bvsge(&BV::from_u64(0, 64)), &[]);
        assert_eq!(got.decl().kind(), DeclKind::Sgeq);
        let lhs = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(lhs.decl().name(), "f64_mul");
    }

    #[test]
    fn recovers_f64_mul_ge_zero_from_sysvar_extracts() {
        let (lo, hi) = rent_sysvar_halves();
        let prod = uif2("f64_mul", &uif1("uitofp", &lo, 64), &hi, 64);
        let got = crate::rewrite::branch(&prod.bvsge(&BV::from_u64(0, 64)), &[]);
        let lhs = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(lhs.decl().name(), "f64_mul");
    }
}
