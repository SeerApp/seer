//! `sitofp` — signed integer to IEEE-754 f64.

use z3::ast::BV;

use super::uitofp::SAMPLES;
use crate::rewrite::ast::uif1;
use crate::rewrite::eval::eval_env;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    if expr.get_size() != 64 || x.get_size() != 64 {
        return None;
    }
    if eval_env(expr, &[(x, 1)])? != 1.0f64.to_bits() {
        return None;
    }
    let min = i64::MIN as u64;
    if eval_env(expr, &[(x, min)])? != (i64::MIN as f64).to_bits() {
        return None;
    }
    for &v in SAMPLES {
        if eval_env(expr, &[(x, v)])? != (v as i64 as f64).to_bits() {
            return None;
        }
    }
    Some(uif1("sitofp", x, 64))
}
