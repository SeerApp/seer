//! `fptosi` — IEEE-754 f64 to signed integer (trunc toward zero).

use z3::ast::BV;

use crate::rewrite::ast::uif1;
use crate::rewrite::eval::eval_env;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    if expr.get_size() != 64 || x.get_size() != 64 {
        return None;
    }
    if eval_env(expr, &[(x, 1.0f64.to_bits())])? != 1 {
        return None;
    }
    if eval_env(expr, &[(x, (-2.5f64).to_bits())])? != (-2i64) as u64 {
        return None;
    }
    for v in [0.0f64, 1.0, -1.0, 2.5, -2.5, 297.0, -3480.0] {
        if eval_env(expr, &[(x, v.to_bits())])? != (v as i64) as u64 {
            return None;
        }
    }
    Some(uif1("fptosi", x, 64))
}
