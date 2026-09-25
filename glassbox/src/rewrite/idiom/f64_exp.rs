//! `f64_exp` — the 11-bit IEEE exponent field.
//!
//! Structural: extract `[62:52]`, or `(x ≫ 52) & 0x7ff`.
//! Sampled: `exp(uitofp(x))`, `exp(f64_mul(…))`, and the unbiased form
//! `exp - 0x3ff`. Predicate: `exp(mul) ≤ 0x7fe` (finite product).

use z3::ast::{Ast, Bool, Dynamic, BV};
use z3::DeclKind;

use super::uitofp::{as_float, float_bits, SAMPLES};
use crate::rewrite::ast::{extract_hi_lo, uif1, uif2};
use crate::rewrite::eval::{
    binary_samples, eval_env, f64_exp_bits, f64_exp_or_inf, samples_match_binary,
    samples_match_bool,
};

pub(crate) fn from_field(expr: &BV) -> Option<BV> {
    if expr.get_size() != 64 {
        return None;
    }
    let core = peel_zext(expr);
    let d = Dynamic::from(&core);
    if is_extract_hi_lo(&d, 62, 52) {
        let src = core.nth_child(0)?.as_bv()?;
        let floatish = matches!(src.decl().name().as_str(), "uitofp" | "sitofp" | "f64_mul")
            || (src.get_size() == 64 && src.num_children() == 0);
        if floatish {
            return Some(uif1("f64_exp", &src, expr.get_size()));
        }
    }
    // `(x ≫ 52) & 0x7ff` — compiler-rt exponent extract.
    if d.decl().kind() == DeclKind::Band && d.num_children() == 2 {
        let a = d.nth_child(0)?.as_bv()?;
        let b = d.nth_child(1)?.as_bv()?;
        let shifted = if b.as_u64() == Some(0x7ff) {
            a
        } else if a.as_u64() == Some(0x7ff) {
            b
        } else {
            return None;
        };
        let s = Dynamic::from(&shifted);
        if s.decl().kind() == DeclKind::Blshr
            && s.num_children() == 2
            && s.nth_child(1)?.as_bv()?.as_u64() == Some(52)
        {
            return Some(uif1("f64_exp", &s.nth_child(0)?.as_bv()?, expr.get_size()));
        }
    }
    None
}

pub(crate) fn of_int(expr: &BV, x: &BV) -> Option<BV> {
    if expr.get_size() != 64 || x.get_size() != 64 {
        return None;
    }
    // exp(1.0) = 1023
    if eval_env(expr, &[(x, 1)])? != 0x3ff {
        return None;
    }
    for &v in SAMPLES {
        let expect = ((v as f64).to_bits() >> 52) & 0x7ff;
        if eval_env(expr, &[(x, v)])? != expect {
            return None;
        }
    }
    Some(uif1("f64_exp", &uif1("uitofp", x, 64), 64))
}

pub(crate) fn of_mul(expr: &BV, a: &BV, b: &BV) -> Option<BV> {
    if expr.get_size() != 64 || a.get_size() != 64 || b.get_size() != 64 {
        return None;
    }
    let pairs = binary_samples();
    for unbiased in [false, true] {
        for (a_int, b_int) in [(true, false), (false, true), (false, false), (true, true)] {
            if samples_match_binary(expr, a, b, &pairs, |va, vb| {
                let e = f64_exp_bits(float_bits(va, a_int), float_bits(vb, b_int))?;
                Some(if unbiased { e.wrapping_sub(0x3ff) } else { e })
            }) {
                let mut out = uif1(
                    "f64_exp",
                    &uif2("f64_mul", &as_float(a, a_int), &as_float(b, b_int), 64),
                    64,
                );
                if unbiased {
                    out = out.bvadd(&BV::from_u64(0xfffffffffffffc01, 64));
                }
                return Some(out);
            }
        }
    }
    None
}

pub(crate) fn finite_mul(formula: &Bool, a: &BV, b: &BV) -> Option<Bool> {
    let pairs = binary_samples();
    for (a_int, b_int) in [(true, false), (false, true), (false, false)] {
        if samples_match_bool(formula, a, b, &pairs, |va, vb| {
            Some(f64_exp_or_inf(float_bits(va, a_int), float_bits(vb, b_int))? <= 0x7fe)
        }) {
            return Some(
                uif1(
                    "f64_exp",
                    &uif2("f64_mul", &as_float(a, a_int), &as_float(b, b_int), 64),
                    64,
                )
                .bvsle(&BV::from_u64(0x7fe, 64)),
            );
        }
    }
    None
}

pub(super) fn peel_zext(expr: &BV) -> BV {
    let mut cur = expr.clone();
    loop {
        match cur.decl().kind() {
            DeclKind::ZeroExt if cur.num_children() == 1 => {
                if let Some(inner) = cur.nth_child(0).and_then(|c| c.as_bv()) {
                    cur = inner;
                    continue;
                }
            }
            DeclKind::Concat if cur.num_children() == 2 => {
                if let (Some(hi), Some(lo)) = (
                    cur.nth_child(0).and_then(|c| c.as_bv()),
                    cur.nth_child(1).and_then(|c| c.as_bv()),
                ) {
                    if hi.as_u64() == Some(0) {
                        cur = lo;
                        continue;
                    }
                }
            }
            _ => {}
        }
        break;
    }
    cur
}

fn is_extract_hi_lo(ast: &Dynamic, hi: u32, lo: u32) -> bool {
    ast.decl().kind() == DeclKind::Extract && extract_hi_lo(ast) == Some((hi, lo))
}

#[cfg(test)]
mod tests {
    use crate::rewrite::ast::{uif1, uif2};
    use crate::rewrite::testing::software_uitofp;
    use z3::ast::{Ast, BV};
    use z3::DeclKind;

    #[test]
    fn recovers_f64_exp_of_uitofp() {
        let x = BV::new_const("w_47", 64);
        let exp = software_uitofp(&x).extract(62, 52).zero_ext(53);
        let got = crate::rewrite::alu(&exp, &[]);
        assert_eq!(got.decl().name(), "f64_exp");
    }

    #[test]
    fn recovers_f64_exp_from_shift_and() {
        let x = BV::new_const("w_39", 64);
        let exp = x
            .bvlshr(&BV::from_u64(52, 64))
            .bvand(&BV::from_u64(0x7ff, 64));
        let got = crate::rewrite::alu(&exp, &[]);
        assert_eq!(got.decl().name(), "f64_exp");
    }

    #[test]
    fn recovers_f64_exp_sles_finite() {
        let x = BV::new_const("w_47", 64);
        let y = BV::new_const("w_39", 64);
        let exp = uif1(
            "f64_exp",
            &uif2("f64_mul", &uif1("uitofp", &x, 64), &y, 64),
            64,
        );
        let bloated = BV::from_u64(0xfffffffffffffc01, 64)
            .bvadd(&exp)
            .bvadd(&BV::from_u64(0x3ff, 64));
        let got = crate::rewrite::branch(&bloated.bvsle(&BV::from_u64(0x7fe, 64)), &[]);
        assert_eq!(got.decl().kind(), DeclKind::Sleq);
        let lhs = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(lhs.decl().name(), "f64_exp");
    }
}
