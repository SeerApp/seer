//! `fptoui` — IEEE-754 f64 to unsigned integer (trunc toward zero).
//!
//! Also recovers `fptoui(f64_mul(…))` and the predicates `> 1`, `≤ 1`,
//! `≤ bound` that compiler-rt emits around a converted product.

use z3::ast::{Bool, BV};

use super::uitofp::{as_float, float_bits};
use crate::rewrite::ast::{uif1, uif2};
use crate::rewrite::eval::{
    binary_samples, eval_env, fptoui_mul_bits, samples_match_binary, samples_match_bool,
    samples_match_bool3, ternary_samples,
};

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    if expr.get_size() != 64 || x.get_size() != 64 {
        return None;
    }
    if eval_env(expr, &[(x, 1.0f64.to_bits())])? != 1 {
        return None;
    }
    if eval_env(expr, &[(x, 2.5f64.to_bits())])? != 2 {
        return None;
    }
    for v in [
        0.0f64,
        1.0,
        2.0,
        2.5,
        3.0,
        297.0,
        3480.0,
        1.5,
        (1u64 << 52) as f64,
    ] {
        if !v.is_finite() {
            continue;
        }
        if eval_env(expr, &[(x, v.to_bits())])? != v as u64 {
            return None;
        }
    }
    Some(uif1("fptoui", x, 64))
}

/// `fptoui(f64_mul(a, b))`, with either operand optionally a raw integer.
pub(crate) fn of_mul(expr: &BV, a: &BV, b: &BV) -> Option<BV> {
    if expr.get_size() != 64 || a.get_size() != 64 || b.get_size() != 64 {
        return None;
    }
    let pairs = binary_samples();
    for (a_int, b_int) in [(true, false), (false, true), (false, false)] {
        if samples_match_binary(expr, a, b, &pairs, |va, vb| {
            fptoui_mul_bits(float_bits(va, a_int), float_bits(vb, b_int))
        }) {
            return Some(uif1(
                "fptoui",
                &uif2("f64_mul", &as_float(a, a_int), &as_float(b, b_int), 64),
                64,
            ));
        }
    }
    None
}

pub(crate) fn gt_one(formula: &Bool, a: &BV, b: &BV) -> Option<Bool> {
    cmp_mul(formula, a, b, |n| n > 1, |n| n.bvugt(&BV::from_u64(1, 64)))
}

pub(crate) fn ule_one(formula: &Bool, a: &BV, b: &BV) -> Option<Bool> {
    cmp_mul(formula, a, b, |n| n <= 1, |n| n.bvule(&BV::from_u64(1, 64)))
}

fn cmp_mul(
    formula: &Bool,
    a: &BV,
    b: &BV,
    pred: fn(u64) -> bool,
    build: fn(&BV) -> Bool,
) -> Option<Bool> {
    let pairs = binary_samples();
    for (a_int, b_int) in [(true, false), (false, true), (false, false)] {
        if samples_match_bool(formula, a, b, &pairs, |va, vb| {
            fptoui_mul_bits(float_bits(va, a_int), float_bits(vb, b_int)).map(pred)
        }) {
            return Some(build(&uif1(
                "fptoui",
                &uif2("f64_mul", &as_float(a, a_int), &as_float(b, b_int), 64),
                64,
            )));
        }
    }
    None
}

/// `fptoui(f64_mul(x, y)) ≤ z` — try every role for the bound.
pub(crate) fn ule_bound(formula: &Bool, a: &BV, b: &BV, c: &BV) -> Option<Bool> {
    let triples = ternary_samples();
    let roles = [
        (a, b, c),
        (b, a, c),
        (a, c, b),
        (c, a, b),
        (b, c, a),
        (c, b, a),
    ];
    for (x, y, z) in roles {
        for (x_int, y_int) in [(true, false), (false, true), (false, false)] {
            if samples_match_bool3(formula, x, y, z, &triples, |vx, vy, vz| {
                fptoui_mul_bits(float_bits(vx, x_int), float_bits(vy, y_int)).map(|n| n <= vz)
            }) {
                return Some(
                    uif1(
                        "fptoui",
                        &uif2("f64_mul", &as_float(x, x_int), &as_float(y, y_int), 64),
                        64,
                    )
                    .bvule(z),
                );
            }
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
    fn recovers_fptoui_mul_gt_one() {
        let x = BV::new_const("w_47", 64);
        let y = BV::new_const("w_39", 64);
        let n = uif1(
            "fptoui",
            &uif2("f64_mul", &uif1("uitofp", &x, 64), &y, 64),
            64,
        );
        let bloated = n.bvadd(&BV::from_u64(0, 64)).bvor(&BV::from_u64(0, 64));
        let got = crate::rewrite::branch(&bloated.bvugt(&BV::from_u64(1, 64)), &[]);
        assert!(
            matches!(got.decl().kind(), DeclKind::Ugt),
            "kind {:?}",
            got.decl().kind()
        );
    }

    #[test]
    fn recovers_fptoui_mul_gt_one_from_sysvar_extracts() {
        let (lo, hi) = rent_sysvar_halves();
        let n = uif1(
            "fptoui",
            &uif2("f64_mul", &uif1("uitofp", &lo, 64), &hi, 64),
            64,
        );
        let got = crate::rewrite::branch(&n.bvugt(&BV::from_u64(1, 64)), &[]);
        assert!(matches!(got.decl().kind(), DeclKind::Ugt));
        let lhs = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(lhs.decl().name(), "fptoui");
    }

    #[test]
    fn recovers_fptoui_mul_ule_packed_lamports() {
        let (lo, hi) = rent_sysvar_halves();
        let mut lamports = BV::new_const("n_acc3_lamports_0", 8);
        for i in 1..8u32 {
            lamports = BV::new_const(format!("n_acc3_lamports_{i}"), 8).concat(&lamports);
        }
        let n = uif1(
            "fptoui",
            &uif2("f64_mul", &uif1("uitofp", &lo, 64), &hi, 64),
            64,
        );
        let got = crate::rewrite::branch(&n.bvule(&lamports), &[]);
        assert!(matches!(got.decl().kind(), DeclKind::Uleq));
        let lhs = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(lhs.decl().name(), "fptoui");
    }
}
