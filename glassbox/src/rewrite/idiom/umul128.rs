//! `umul128_hi` / `umul128_lo` — halves of a 64×64→128 multiply.
//!
//! SBPF has no 128-bit mul. Sample against `(a as u128) * (b as u128)`.

use z3::ast::BV;

use crate::rewrite::ast::uif2;
use crate::rewrite::eval::{binary_samples, samples_match_binary};

pub(crate) fn detect(expr: &BV, a: &BV, b: &BV) -> Option<BV> {
    if expr.get_size() != 64 || a.get_size() != 64 || b.get_size() != 64 {
        return None;
    }
    let pairs = binary_samples();
    if samples_match_binary(expr, a, b, &pairs, true_hi) {
        return Some(uif2("umul128_hi", a, b, 64));
    }
    if samples_match_binary(expr, a, b, &pairs, true_lo) {
        return Some(uif2("umul128_lo", a, b, 64));
    }
    None
}

/// LLVM wrap-hi fragment, not true `(a as u128 * b as u128) >> 64`.
pub(crate) fn near_miss(expr: &BV, a: &BV, b: &BV) -> Option<&'static str> {
    if expr.get_size() != 64 || a.get_size() != 64 || b.get_size() != 64 {
        return None;
    }
    let pairs = binary_samples();
    if samples_match_binary(expr, a, b, &pairs, true_hi) {
        return None;
    }
    if samples_match_binary(expr, a, b, &pairs, wrap_hi) {
        return Some("umul128_hi");
    }
    None
}

fn true_hi(va: u64, vb: u64) -> Option<u64> {
    Some((((va as u128) * (vb as u128)) >> 64) as u64)
}

fn true_lo(va: u64, vb: u64) -> Option<u64> {
    Some(((va as u128) * (vb as u128)) as u64)
}

/// `ah×bh + (ah×bl + al×bh) ≫ 32` in wrapping 64-bit — what compiler-rt
/// leaves in the hi register when carry-out of the limb adds is dropped.
fn wrap_hi(va: u64, vb: u64) -> Option<u64> {
    let al = va & 0xffff_ffff;
    let ah = va >> 32;
    let bl = vb & 0xffff_ffff;
    let bh = vb >> 32;
    let mid = ah.wrapping_mul(bl).wrapping_add(al.wrapping_mul(bh));
    Some(ah.wrapping_mul(bh).wrapping_add(mid >> 32))
}

#[cfg(test)]
mod tests {
    use crate::rewrite::alu;
    use crate::state::LoadDef;
    use z3::ast::{Ast, BV};

    fn lo32(x: &BV) -> BV {
        x.extract(31, 0)
            .concat(&BV::from_u64(0, 32))
            .bvlshr(&BV::from_u64(32, 64))
    }

    fn hi32(x: &BV) -> BV {
        x.bvlshr(&BV::from_u64(32, 64))
    }

    /// compiler-rt 64×64→128 low half from 32-bit limbs.
    fn software_umul128_lo(a: &BV, b: &BV) -> BV {
        let al = lo32(a);
        let ah = hi32(a);
        let bl = lo32(b);
        let bh = hi32(b);
        let mid = ah.bvmul(&bl).bvadd(&al.bvmul(&bh));
        al.bvmul(&bl)
            .bvadd(&mid.extract(31, 0).concat(&BV::from_u64(0, 32)))
    }

    /// Exact 64×64→128 high half from 32-bit limbs (`mulhu`).
    fn software_umul128_hi(ah_src: &BV, al_src: &BV, bh_src: &BV, bl_src: &BV) -> BV {
        let al = lo32(al_src);
        let ah = hi32(ah_src);
        let bl = lo32(bl_src);
        let bh = hi32(bh_src);
        let p0 = al.bvmul(&bl);
        let p1 = al.bvmul(&bh);
        let p2 = ah.bvmul(&bl);
        let p3 = ah.bvmul(&bh);
        let t = hi32(&p0).bvadd(&lo32(&p1)).bvadd(&lo32(&p2));
        p3.bvadd(&hi32(&p1)).bvadd(&hi32(&p2)).bvadd(&hi32(&t))
    }

    #[test]
    fn recovers_limb_mul_of_two_atoms() {
        let a = BV::new_const("w_a", 64);
        let b = BV::new_const("w_b", 64);
        let got = alu(&software_umul128_lo(&a, &b), &[]);
        assert_eq!(got.decl().name(), "umul128_lo", "got {got}");
    }

    #[test]
    fn recovers_limb_mul_hi_of_two_atoms() {
        let a = BV::new_const("w_a", 64);
        let b = BV::new_const("w_b", 64);
        let got = alu(&software_umul128_hi(&a, &a, &b, &b), &[]);
        assert_eq!(got.decl().name(), "umul128_hi", "got {got}");
    }

    #[test]
    fn cse_named_div_then_limb_mul() {
        let src = BV::new_const("w_src", 64);
        let div = src.bvudiv(&BV::from_u64(1_000_000_000, 64));
        let b = BV::new_const("w_b", 64);
        let defs = [LoadDef {
            name: "w_a".into(),
            expr: div.clone(),
        }];
        let got = alu(&software_umul128_lo(&div, &b), &defs);
        assert_eq!(got.decl().name(), "umul128_lo", "got {got}");
        let mut args: Vec<_> = (0..2)
            .map(|i| got.nth_child(i).unwrap().as_bv().unwrap().decl().name())
            .collect();
        args.sort();
        assert_eq!(args, ["w_a", "w_b"]);
    }

    #[test]
    fn peel_root_compound_or_is_one_operand() {
        let a = BV::new_const("w_a", 64);
        let x = BV::new_const("w_x", 64);
        let y = BV::new_const("w_y", 64);
        let stitch = x
            .bvlshr(&BV::from_u64(8, 64))
            .bvor(&y.extract(7, 0).concat(&BV::from_u64(0, 56)));
        let got = alu(&software_umul128_lo(&a, &stitch), &[]);
        assert_eq!(got.decl().name(), "umul128_lo", "got {got}");
    }

    /// High limbs from the names, low limbs from the inlined defs — the
    /// leftover-register shape of `w_296`.
    #[test]
    fn cse_mixed_name_and_def_windows_then_hi() {
        use crate::rewrite::ast::uif2;
        let src = BV::new_const("w_src", 64);
        let div = src.bvudiv(&BV::from_u64(1_000_000_000, 64));
        let x = BV::new_const("w_x", 64);
        let y = BV::new_const("w_y", 64);
        let inner = uif2("umul128_lo", &x, &y, 64);
        let a = BV::new_const("w_a", 64);
        let b = BV::new_const("w_b", 64);
        let defs = [
            LoadDef {
                name: "w_a".into(),
                expr: div.clone(),
            },
            LoadDef {
                name: "w_b".into(),
                expr: inner.clone(),
            },
        ];
        let got = alu(&software_umul128_hi(&a, &div, &b, &inner), &defs);
        assert_eq!(got.decl().name(), "umul128_hi", "got {got}");
        let mut args: Vec<_> = (0..2)
            .map(|i| got.nth_child(i).unwrap().as_bv().unwrap().decl().name())
            .collect();
        args.sort();
        assert_eq!(args, ["w_a", "w_b"]);
    }

    /// compiler-rt wrap-hi with leftover `+ 0` / `0 × b` (the `w_284` shape).
    fn software_umul128_hi_wrap(a: &BV, b: &BV) -> BV {
        let al = lo32(a);
        let ah = hi32(a);
        let bl = lo32(b);
        let bh = hi32(b);
        ah.bvmul(&bh)
            .bvadd(&BV::from_u64(0, 64).bvmul(b))
            .bvadd(&BV::from_u64(0, 64))
            .bvadd(
                &ah.bvmul(&bl)
                    .bvadd(&al.bvmul(&bh))
                    .bvlshr(&BV::from_u64(32, 64)),
            )
            .bvadd(&BV::from_u64(0, 64))
    }

    #[test]
    fn labels_wrap_hi_as_near_miss() {
        let a = BV::new_const("w_a", 64);
        let b = BV::new_const("w_b", 64);
        assert_eq!(
            crate::rewrite::near_miss(&software_umul128_hi_wrap(&a, &b), &[]),
            Some("umul128_hi")
        );
        assert_eq!(
            crate::rewrite::near_miss(&software_umul128_hi(&a, &a, &b, &b), &[]),
            None,
            "exact hi is not a near-miss"
        );
        let lo = alu(&software_umul128_lo(&a, &b), &[]);
        assert_eq!(crate::rewrite::near_miss(&lo, &[]), None);
    }

    #[test]
    fn labels_wrap_hi_of_unaligned_qword() {
        let a = BV::new_const("w_a", 64);
        let b = BV::new_const("w_b", 64);
        let b8 = b.bvlshr(&BV::from_u64(8, 64));
        let bh = hi32(&b8);
        let bl = b
            .extract(39, 8)
            .concat(&BV::from_u64(0, 32))
            .bvlshr(&BV::from_u64(32, 64));
        let ah = hi32(&a);
        let al = lo32(&a);
        let expr = ah
            .bvmul(&bh)
            .bvadd(&BV::from_u64(0, 64))
            .bvadd(
                &ah.bvmul(&bl)
                    .bvadd(&al.bvmul(&bh))
                    .bvlshr(&BV::from_u64(32, 64)),
            )
            .bvadd(&BV::from_u64(0, 64));
        assert_eq!(crate::rewrite::near_miss(&expr, &[]), Some("umul128_hi"));
    }

    fn concat_bytes_be(bytes: &[BV]) -> BV {
        let mut acc = bytes[0].clone();
        for b in &bytes[1..] {
            acc = acc.concat(b);
        }
        acc
    }

    /// `w_190` shape: hi32 from the packed qword, lo32 rebuilt from its low bytes.
    #[test]
    fn labels_wrap_hi_when_lo_limb_is_byte_pack() {
        let bytes: Vec<BV> = (0..8).map(|i| BV::new_const(format!("n_{i}"), 8)).collect();
        let a = concat_bytes_be(&bytes);
        let b = BV::new_const("w_b", 64);
        let ah = hi32(&a);
        let al = concat_bytes_be(&bytes[4..])
            .concat(&BV::from_u64(0, 32))
            .bvlshr(&BV::from_u64(32, 64));
        let bh = hi32(&b);
        let bl = lo32(&b);
        let expr = ah
            .bvmul(&bh)
            .bvadd(&BV::from_u64(0, 64))
            .bvadd(
                &ah.bvmul(&bl)
                    .bvadd(&al.bvmul(&bh))
                    .bvlshr(&BV::from_u64(32, 64)),
            )
            .bvadd(&BV::from_u64(0, 64));
        assert_eq!(crate::rewrite::near_miss(&expr, &[]), Some("umul128_hi"));

        let name = BV::new_const("w_acc", 64);
        let ah = hi32(&name);
        let expr = ah
            .bvmul(&bh)
            .bvadd(&BV::from_u64(0, 64))
            .bvadd(
                &ah.bvmul(&bl)
                    .bvadd(&al.bvmul(&bh))
                    .bvlshr(&BV::from_u64(32, 64)),
            )
            .bvadd(&BV::from_u64(0, 64));
        let defs = [LoadDef {
            name: "w_acc".into(),
            expr: a,
        }];
        assert_eq!(crate::rewrite::near_miss(&expr, &defs), Some("umul128_hi"));
    }
}
