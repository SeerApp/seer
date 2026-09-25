//! `f64_sign` — the IEEE sign bit.
//!
//! Structural: extract `[63:63]` of a recovered float. Sampled: the high bit
//! of `f64_mul(uitofp(a), b)` (or swapped), minted as `f64_sign(mul) ≪ 63`.

use z3::ast::{Ast, Dynamic, BV};
use z3::DeclKind;

use super::f64_exp::peel_zext;
use super::uitofp::{as_float, float_bits};
use crate::rewrite::ast::{extract_hi_lo, uif1, uif2};
use crate::rewrite::eval::{binary_samples, f64_mul_bits, samples_match_binary};

pub(crate) fn from_field(expr: &BV) -> Option<BV> {
    let core = peel_zext(expr);
    let d = Dynamic::from(&core);
    if d.decl().kind() != DeclKind::Extract || extract_hi_lo(&d) != Some((63, 63)) {
        return None;
    }
    let src = core.nth_child(0)?.as_bv()?;
    if src.get_size() != 64 {
        return None;
    }
    if !matches!(src.decl().name().as_str(), "uitofp" | "sitofp" | "f64_mul") {
        return None;
    }
    let sign = uif1("f64_sign", &src, 64);
    Some(if expr.get_size() == 64 {
        sign
    } else {
        sign.extract(0, 0)
            .zero_ext(expr.get_size().saturating_sub(1))
    })
}

pub(crate) fn of_mul(expr: &BV, a: &BV, b: &BV) -> Option<BV> {
    if expr.get_size() != 64 || a.get_size() != 64 || b.get_size() != 64 {
        return None;
    }
    let pairs = binary_samples();
    for (a_int, b_int) in [(true, false), (false, true)] {
        if samples_match_binary(expr, a, b, &pairs, |va, vb| {
            f64_mul_bits(float_bits(va, a_int), float_bits(vb, b_int)).map(|p| p & (1u64 << 63))
        }) {
            return Some(
                uif1(
                    "f64_sign",
                    &uif2("f64_mul", &as_float(a, a_int), &as_float(b, b_int), 64),
                    64,
                )
                .bvshl(&BV::from_u64(63, 64)),
            );
        }
    }
    None
}
