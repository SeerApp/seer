//! `ror` / `rol` — rotate right / left.
//!
//! SBPF has no rotate. compiler-rt emits complementary shifts
//! `(x ≫ n) | (x ≪ (w-n))` (sometimes XOR). Variable `n` is the binary
//! case. A constant amount is unary: mint `ror(x, k)`. Const `rol(x, k)`
//! is the same bits as `ror(x, w-k)` — recovered as `ror`.

use z3::ast::{Ast, BV};
use z3::{DeclKind, SatResult};

use crate::astwalk::ast_id;
use crate::rewrite::ast::{bv_equiv_result, rust_rol, rust_ror, uif2};
use crate::rewrite::eval::{eval_at, samples_match_binary};

/// Complementary shifts of the same `x`: `(x ≪ n) | (x ≫ (w-n))`.
///
/// Any arity — SipHash rotates a mixed `v` word, so the unary `ror_const`
/// sampler never fires. Flatten would treat the disjoint OR as a pack and
/// explode. Mint `ror(x, w-n)` (same bits as `rol(x, n)`).
pub(crate) fn from_shifts(expr: &BV) -> Option<BV> {
    let w = expr.get_size();
    if w < 2 || w > 64 {
        return None;
    }
    let d = z3::ast::Dynamic::from(expr);
    if !matches!(d.decl().kind(), DeclKind::Bor | DeclKind::Bxor) || d.num_children() != 2 {
        return None;
    }
    let a = d.nth_child(0)?.as_bv()?;
    let b = d.nth_child(1)?.as_bv()?;
    let (x1, dir1, n1) = shift_const(&a)?;
    let (x2, dir2, n2) = shift_const(&b)?;
    if ast_id(&x1) != ast_id(&x2) || n1 == 0 || n2 == 0 || n1 + n2 != w || dir1 == dir2 {
        return None;
    }
    let ror_n = if dir1 == ShiftDir::Lshr { n1 } else { n2 };
    Some(uif2("ror", &x1, &BV::from_u64(ror_n as u64, w), w))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShiftDir {
    Shl,
    Lshr,
}

fn shift_const(bv: &BV) -> Option<(BV, ShiftDir, u32)> {
    let d = z3::ast::Dynamic::from(bv);
    let dir = match d.decl().kind() {
        DeclKind::Bshl => ShiftDir::Shl,
        DeclKind::Blshr => ShiftDir::Lshr,
        _ => return None,
    };
    if d.num_children() != 2 {
        return None;
    }
    let x = d.nth_child(0)?.as_bv()?;
    let n = d.nth_child(1)?.as_bv()?.as_u64()?;
    if n == 0 || n >= bv.get_size() as u64 {
        return None;
    }
    Some((x, dir, n as u32))
}

pub(crate) fn ror_const(expr: &BV, x: &BV) -> Option<BV> {
    let w = x.get_size();
    if w < 2 || w > 64 || w != expr.get_size() {
        return None;
    }
    if eval_at(expr, x, 0)? != 0 {
        return None;
    }
    let y = eval_at(expr, x, 1)?;
    if y.count_ones() != 1 {
        return None;
    }
    let k = (w - y.trailing_zeros()) % w;
    if k == 0 {
        return None;
    }
    for v in [
        2,
        0xff,
        0x0102,
        0x8000_0000_0000_0000,
        0x0123_4567_89ab_cdef,
        u64::MAX,
    ] {
        if eval_at(expr, x, v)? != rust_ror(v, k as u64, w) {
            return None;
        }
    }
    let amt = BV::from_u64(k as u64, w);
    match bv_equiv_result(expr, &x.bvrotr(&amt)) {
        SatResult::Sat => None,
        SatResult::Unsat | SatResult::Unknown => Some(uif2("ror", x, &amt, w)),
    }
}

pub(crate) fn ror_var(expr: &BV, a: &BV, b: &BV) -> Option<BV> {
    rotate_var(expr, a, b, rust_ror, |x, n| x.bvrotr(n), "ror")
}

pub(crate) fn rol_var(expr: &BV, a: &BV, b: &BV) -> Option<BV> {
    rotate_var(expr, a, b, rust_rol, |x, n| x.bvrotl(n), "rol")
}

fn rotate_var(
    expr: &BV,
    a: &BV,
    b: &BV,
    expect: fn(u64, u64, u32) -> u64,
    reference: fn(&BV, &BV) -> BV,
    name: &str,
) -> Option<BV> {
    let w = expr.get_size();
    if w < 2 || w > 64 || a.get_size() != w || b.get_size() != w {
        return None;
    }
    let pairs = rotate_pairs(w);
    for (val, amt) in [(a, b), (b, a)] {
        if samples_match_binary(expr, val, amt, &pairs, |vx, vn| Some(expect(vx, vn, w))) {
            match bv_equiv_result(expr, &reference(val, amt)) {
                SatResult::Sat => return None,
                SatResult::Unsat | SatResult::Unknown => {
                    return Some(uif2(name, val, amt, w));
                }
            }
        }
    }
    None
}

fn rotate_pairs(w: u32) -> Vec<(u64, u64)> {
    let xs = [
        0u64,
        1,
        2,
        0xff,
        1u64 << (w.min(64) - 1),
        0x0123_4567_89ab_cdef,
        0xdead_beef,
        u64::MAX,
    ];
    let mut out = Vec::new();
    for &x in &xs {
        for n in 0..w {
            out.push((x, n as u64));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::ror_const;
    use crate::rewrite::eval::eval_env;
    use crate::rewrite::testing::{software_rol, software_ror};
    use z3::ast::{Ast, BV};

    #[test]
    fn from_shifts_recovers_rotl_of_mixed_word() {
        let vs: [BV; 4] = std::array::from_fn(|i| BV::new_const(format!("w_{i}"), 64));
        let x = vs[0].bvxor(&vs[1]).bvadd(&vs[2]).bvxor(&vs[3]);
        let blob = x
            .bvshl(&BV::from_u64(13, 64))
            .bvor(&x.bvlshr(&BV::from_u64(51, 64)));
        let got = crate::rewrite::alu(&blob, &[]);
        assert_eq!(got.decl().name(), "ror");
        assert_eq!(got.nth_child(1).unwrap().as_bv().unwrap().as_u64(), Some(51));
        assert_eq!(got.num_children(), 2);
    }

    #[test]
    fn from_shifts_survives_prior_shift_alus() {
        let vs: [BV; 4] = std::array::from_fn(|i| BV::new_const(format!("w_{i}"), 64));
        let x = vs[0].bvxor(&vs[1]).bvadd(&vs[2]).bvxor(&vs[3]);
        let shl = crate::rewrite::alu(&x.bvshl(&BV::from_u64(13, 64)), &[]);
        let lshr = crate::rewrite::alu(&x.bvlshr(&BV::from_u64(51, 64)), &[]);
        let got = crate::rewrite::alu(&shl.bvor(&lshr), &[]);
        assert_eq!(got.decl().name(), "ror", "got {got}");
    }

    #[test]
    fn recovers_const_ror_from_complementary_shift() {
        let x = BV::new_const("w_47", 64);
        let k = BV::from_u64(8, 64);
        let blob = x.bvlshr(&k).bvor(&x.bvshl(&BV::from_u64(56, 64)));
        let got = crate::rewrite::alu(&blob, &[]);
        assert_eq!(got.decl().name(), "ror");
        assert_eq!(got.nth_child(1).unwrap().as_bv().unwrap().as_u64(), Some(8));
    }

    #[test]
    fn recovers_var_ror_from_xor_shifts() {
        let x = BV::new_const("w_47", 64);
        let n = BV::new_const("w_3", 64);
        let got = crate::rewrite::alu(&software_ror(&x, &n), &[]);
        assert_eq!(got.decl().name(), "ror");
        assert_eq!(got.num_children(), 2);
    }

    #[test]
    fn recovers_var_rol() {
        let x = BV::new_const("w_47", 64);
        let n = BV::new_const("w_3", 64);
        let got = crate::rewrite::alu(&software_rol(&x, &n), &[]);
        assert_eq!(got.decl().name(), "rol");
    }

    #[test]
    fn const_ror_is_not_bswap() {
        let x = BV::new_const("w_47", 64);
        // ror 8 at x=1 looks like bswap64; 0x0102 distinguishes.
        let blob = x
            .bvlshr(&BV::from_u64(8, 64))
            .bvor(&x.bvshl(&BV::from_u64(56, 64)));
        assert!(ror_const(&blob, &x).is_some());
        let got = crate::rewrite::alu(&blob, &[]);
        assert_eq!(got.decl().name(), "ror");
        assert_eq!(eval_env(&got, &[(&x, 0x0102)]), Some(0x0200000000000001));
    }
}
