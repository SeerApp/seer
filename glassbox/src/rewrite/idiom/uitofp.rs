//! `uitofp` — unsigned integer to IEEE-754 f64.
//!
//! compiler-rt packs sign/exp/mant with round-to-nearest-even
//! (`testing::software_uitofp`). Sample against `(v as f64).to_bits()`.
//! The software pack is wrong at 0; we still recover if every other sample
//! matches.

use z3::ast::BV;

use crate::rewrite::ast::uif1;
use crate::rewrite::eval::eval_env;

pub(crate) const SAMPLES: &[u64] = &[
    0,
    1,
    2,
    3,
    0xff,
    0x1_0000,
    (1u64 << 52) - 1,
    1u64 << 52,
    (1u64 << 52) + 1,
    (1u64 << 53) - 1,
    1u64 << 53,
    (1u64 << 53) + 1,
    297,
    3480,
    3480 * 297,
    1u64 << 63,
    (1u64 << 63) + 1,
    u64::MAX,
];

/// Bits of `v` as an f64 payload. If `from_int`, `v` is a raw integer that
/// went through compiler-rt `uitofp` (skip 0 — that pack is wrong).
pub(crate) fn float_bits(v: u64, from_int: bool) -> Option<u64> {
    if from_int {
        if v == 0 {
            None
        } else {
            Some((v as f64).to_bits())
        }
    } else {
        Some(v)
    }
}

pub(crate) fn as_float(x: &BV, from_int: bool) -> BV {
    if from_int {
        uif1("uitofp", x, 64)
    } else {
        x.clone()
    }
}

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    if expr.get_size() != 64 || x.get_size() != 64 {
        return None;
    }
    // 1.0 in IEEE-754 f64 — almost nothing else yields this at x=1.
    if eval_env(expr, &[(x, 1)])? != 1.0f64.to_bits() {
        return None;
    }
    for &v in SAMPLES {
        let got = eval_env(expr, &[(x, v)])?;
        if got != (v as f64).to_bits() && v != 0 {
            return None;
        }
    }
    Some(uif1("uitofp", x, 64))
}

#[cfg(test)]
mod tests {
    use super::SAMPLES;
    use crate::rewrite::eval::eval_at;
    use crate::rewrite::testing::software_uitofp;
    use z3::ast::{Ast, BV};

    #[test]
    fn software_uitofp_matches_ieee_samples() {
        let x = BV::new_const("w_47", 64);
        let expr = software_uitofp(&x);
        for &v in SAMPLES {
            assert_eq!(
                eval_at(&expr, &x, v),
                Some((v as f64).to_bits()),
                "uitofp construction failed at {v}"
            );
        }
    }

    #[test]
    fn recovers_software_uitofp() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&software_uitofp(&x), &[]);
        assert_eq!(got.decl().name(), "uitofp");
        assert_eq!(got.num_children(), 1);
    }

    #[test]
    fn recovers_uitofp_pack_wrong_at_zero() {
        let x = BV::new_const("w_47", 64);
        let packed = software_uitofp(&x);
        let expr = x
            .eq(&BV::from_u64(0, 64))
            .ite(&BV::from_u64(0x43d0000000000000, 64), &packed);
        assert_ne!(eval_at(&expr, &x, 0), Some(0));
        let got = crate::rewrite::alu(&expr, &[]);
        assert_eq!(got.decl().name(), "uitofp");
    }
}
