//! `bswap` — byte swap (16/32/64).
//!
//! SBPF has no BSWAP. compiler-rt shifts and masks each byte into place.
//! `bswap(1)` is a single high bit, same as `ror(x, 8)` at that one sample;
//! later samples distinguish. Prove against concat-of-extracts, mint `bswap(x)`.

use z3::ast::BV;
use z3::SatResult;

use crate::rewrite::ast::{bv_equiv_result, rust_bswap, uif1};
use crate::rewrite::eval::eval_at;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    let w = x.get_size();
    if w != expr.get_size() || !matches!(w, 16 | 32 | 64) {
        return None;
    }
    if eval_at(expr, x, 0)? != 0 {
        return None;
    }
    if eval_at(expr, x, 1)? != rust_bswap(1, w) {
        return None;
    }
    for v in [
        2,
        0xff,
        0x0102,
        0x0123_4567,
        0x0123_4567_89ab_cdef,
        u64::MAX,
    ] {
        if eval_at(expr, x, v)? != rust_bswap(v, w) {
            return None;
        }
    }
    match bv_equiv_result(expr, &bswap_concat(x)?) {
        SatResult::Sat => None,
        SatResult::Unsat | SatResult::Unknown => Some(uif1("bswap", x, w)),
    }
}

fn bswap_concat(x: &BV) -> Option<BV> {
    let w = x.get_size();
    if w % 8 != 0 || w == 0 {
        return None;
    }
    let bytes = w / 8;
    let mut acc = x.extract(7, 0);
    for i in 1..bytes {
        acc = acc.concat(&x.extract(8 * i + 7, 8 * i));
    }
    Some(acc)
}

#[cfg(test)]
mod tests {
    use crate::rewrite::testing::software_bswap;
    use z3::ast::{Ast, BV};

    #[test]
    fn recovers_bswap64_from_byte_concat() {
        let x = BV::new_const("w_47", 64);
        let got = crate::rewrite::alu(&software_bswap(&x), &[]);
        assert_eq!(got.decl().name(), "bswap");
        assert_eq!(got.num_children(), 1);
    }
}
