use z3::ast::{Bool, BV};

use super::{eval_bool_env, eval_env};

pub(crate) fn binary_samples() -> Vec<(u64, u64)> {
    let ints = [0u64, 1, 2, 297, 3480, 3480 * 297, 1u64 << 52, 1u64 << 53];
    let floats = [
        0.0f64.to_bits(),
        1.0f64.to_bits(),
        2.0f64.to_bits(),
        2.5f64.to_bits(),
        (-1.0f64).to_bits(),
        1.5f64.to_bits(),
        3480.0f64.to_bits(),
        f64::MAX.to_bits(),
        (f64::MAX / 2.0).to_bits(),
        1e300f64.to_bits(),
    ];
    let mut pairs = Vec::new();
    for &i in &ints {
        for &f in &floats {
            pairs.push((i, f));
            pairs.push((f, i));
        }
    }
    for &a in &floats {
        for &b in &floats {
            pairs.push((a, b));
        }
    }
    pairs
}

pub(crate) fn samples_match_binary(
    expr: &BV,
    a: &BV,
    b: &BV,
    pairs: &[(u64, u64)],
    expect: impl Fn(u64, u64) -> Option<u64>,
) -> bool {
    let mut matched = 0u32;
    for &(va, vb) in pairs {
        let Some(want) = expect(va, vb) else {
            continue;
        };
        let Some(got) = eval_env(expr, &[(a, va), (b, vb)]) else {
            continue;
        };
        if got != want {
            return false;
        }
        matched += 1;
    }
    matched >= 8
}

pub(crate) fn samples_match_bool(
    formula: &Bool,
    a: &BV,
    b: &BV,
    pairs: &[(u64, u64)],
    expect: impl Fn(u64, u64) -> Option<bool>,
) -> bool {
    let mut matched = 0u32;
    let mut saw_true = false;
    let mut saw_false = false;
    for &(va, vb) in pairs {
        let Some(want) = expect(va, vb) else {
            continue;
        };
        let Some(got) = eval_bool_env(formula, &[(a, va), (b, vb)]) else {
            continue;
        };
        if got != want {
            return false;
        }
        if got {
            saw_true = true;
        } else {
            saw_false = true;
        }
        matched += 1;
    }
    matched >= 8 && saw_true && saw_false
}

pub(crate) fn f64_mul_bits(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    let fa = f64::from_bits(a?);
    let fb = f64::from_bits(b?);
    // compiler-rt's unpacked-mantissa path is wrong at 0 (hidden bit still set).
    if !fa.is_normal() || !fb.is_normal() {
        return None;
    }
    let p = fa * fb;
    if !p.is_finite() {
        return None;
    }
    Some(p.to_bits())
}

pub(crate) fn f64_exp_bits(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    Some((f64_mul_bits(a, b)? >> 52) & 0x7ff)
}

pub(crate) fn f64_exp_or_inf(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    let fa = f64::from_bits(a?);
    let fb = f64::from_bits(b?);
    if !fa.is_finite() || !fb.is_finite() {
        return None;
    }
    Some(((fa * fb).to_bits() >> 52) & 0x7ff)
}

pub(crate) fn fptoui_mul_bits(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    let p = f64_mul_bits(a, b)?;
    let f = f64::from_bits(p);
    if !f.is_finite() || f < 0.0 {
        return None;
    }
    Some(f as u64)
}

pub(crate) fn ternary_samples() -> Vec<(u64, u64, u64)> {
    let ints = [1u64, 2, 297, 3480];
    let floats = [1.0f64.to_bits(), 2.0f64.to_bits(), 2.5f64.to_bits()];
    let bounds = [0u64, 1, 2, 1000, 1_000_000, u64::MAX / 2];
    let mut out = Vec::new();
    for &i in &ints {
        for &f in &floats {
            for &b in &bounds {
                out.push((i, f, b));
                out.push((f, i, b));
            }
        }
    }
    out
}

pub(crate) fn samples_match_bool3(
    formula: &Bool,
    a: &BV,
    b: &BV,
    c: &BV,
    triples: &[(u64, u64, u64)],
    expect: impl Fn(u64, u64, u64) -> Option<bool>,
) -> bool {
    let mut matched = 0u32;
    let mut saw_true = false;
    let mut saw_false = false;
    for &(va, vb, vc) in triples {
        let Some(want) = expect(va, vb, vc) else {
            continue;
        };
        let Some(got) = eval_bool_env(formula, &[(a, va), (b, vb), (c, vc)]) else {
            continue;
        };
        if got != want {
            return false;
        }
        if got {
            saw_true = true;
        } else {
            saw_false = true;
        }
        matched += 1;
    }
    matched >= 8 && saw_true && saw_false
}
