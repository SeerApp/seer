//! `x*k + b` — split multiply / add recovered as one affine form.
//!
//! Not an ISA op. Must not seed helper taint: the dest is a real ALU result,
//! not compiler-rt smear.

use z3::ast::BV;
use z3::SatResult;

use crate::rewrite::ast::bv_equiv_result;
use crate::rewrite::eval::eval_at;

pub(crate) fn detect(expr: &BV, x: &BV) -> Option<BV> {
    let w = x.get_size();
    if w != expr.get_size() {
        return None;
    }
    let b = eval_at(expr, x, 0)?;
    let f1 = eval_at(expr, x, 1)?;
    let k = f1.wrapping_sub(b);
    for v in [
        2,
        3,
        0xff,
        0x100,
        0x1_0000,
        0xffff_ffff,
        0x1_0000_0000,
        0xdead_beef,
        0x8000_0000_0000_0000,
        u64::MAX,
    ] {
        let got = eval_at(expr, x, v)?;
        if got != v.wrapping_mul(k).wrapping_add(b) {
            return None;
        }
    }
    let guessed = if k == 0 {
        BV::from_u64(b, w)
    } else if k == 1 && b == 0 {
        x.clone()
    } else if k == 1 {
        x.bvadd(&BV::from_u64(b, w))
    } else if b == 0 {
        x.bvmul(&BV::from_u64(k, w))
    } else {
        x.bvmul(&BV::from_u64(k, w)).bvadd(&BV::from_u64(b, w))
    };
    match bv_equiv_result(expr, &guessed) {
        SatResult::Sat => None,
        SatResult::Unsat | SatResult::Unknown => Some(guessed),
    }
}

#[cfg(test)]
mod tests {
    use crate::rewrite::ast::unique_nodes;
    use crate::rewrite::testing::{bv_equiv, free_bv_consts, split_mul_0x129};
    use z3::ast::{Ast, BV};
    use z3::DeclKind;

    #[test]
    fn recovers_split_mul_as_times_constant() {
        let x = BV::new_const("w_40", 64);
        let bloated = split_mul_0x129(&x);
        let expect = x.bvmul(&BV::from_u64(0x129, 64));
        let got = crate::rewrite::alu(&bloated, &[]);
        assert_eq!(got.decl().kind(), DeclKind::Bmul);
        assert!(
            bv_equiv(&got, &expect),
            "rewrite did not recover w_40 × 0x129"
        );
        assert!(unique_nodes(&got) < unique_nodes(&bloated));
        assert_eq!(free_bv_consts(&got).len(), 1);
    }

    #[test]
    fn leaves_simple_mul_alone() {
        let x = BV::new_const("w_1", 64);
        let e = x.bvmul(&BV::from_u64(3, 64));
        let got = crate::rewrite::alu(&e, &[]);
        assert!(bv_equiv(&got, &e));
    }
}
