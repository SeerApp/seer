//! Test-only rewrite fixtures and the soundness gate.
//!
//! Not compiled into the library. Skip this file (and every `#[cfg(test)]`
//! block) when reading production behaviour.

use std::collections::HashSet;

use z3::ast::{Ast, Bool, Dynamic, BV};
use z3::{DeclKind, FuncDecl, Params, SatResult, Solver, Sort};

use super::ast::{bv_equiv_result, uif1, uif2, PROOF_TIMEOUT_MS};
use super::eval::eval_at;
use super::idiom::clz_ite;
use super::{alu, branch};
use crate::astwalk::for_each_app;
use crate::grammar::is_recovered_uif;

/// Compiler-style 64-bit mul by 0x129 assembled from 32-bit pieces + low byte.
pub(crate) fn split_mul_0x129(x: &BV) -> BV {
    let c129 = BV::from_u64(0x129, 64);
    let lo = BV::from_u64(0, 32).concat(&x.extract(31, 0));
    let hi = x.extract(63, 32).zero_ext(32);
    let acc = c129
        .bvmul(&lo)
        .bvadd(&c129.bvmul(&hi).bvshl(&BV::from_u64(32, 64)));
    let lo8 = BV::from_u64(0x29, 8).bvmul(&x.extract(7, 0));
    acc.extract(63, 8).concat(&lo8)
}

/// compiler-rt SWAR `clz`: smear, invert, then Hacker's Delight popcount.
pub(crate) fn swar_clz(x: &BV) -> BV {
    let w = x.get_size();
    let sh = |s: u64| BV::from_u64(s, w);
    let mut s = x.clone();
    for shift in [1u64, 2, 4, 8, 16, 32] {
        if shift < w as u64 {
            s = s.bvor(&s.bvlshr(&sh(shift)));
        }
    }
    let mut c = s.bvnot();
    let m55 = BV::from_u64(0x5555_5555_5555_5555, w);
    let m33 = BV::from_u64(0x3333_3333_3333_3333, w);
    let m0f = BV::from_u64(0x0f0f_0f0f_0f0f_0f0f, w);
    let m01 = BV::from_u64(0x0101_0101_0101_0101, w);
    c = c.bvsub(&c.bvlshr(&sh(1)).bvand(&m55));
    c = c.bvand(&m33).bvadd(&c.bvlshr(&sh(2)).bvand(&m33));
    c = c.bvadd(&c.bvlshr(&sh(4))).bvand(&m0f);
    c.bvmul(&m01).bvlshr(&sh(56))
}

/// Hacker's Delight SWAR popcount (the tail of `swar_clz` without the smear).
pub(crate) fn swar_popcnt(x: &BV) -> BV {
    let w = x.get_size();
    let sh = |s: u64| BV::from_u64(s, w);
    let mut c = x.clone();
    let m55 = BV::from_u64(0x5555_5555_5555_5555, w);
    let m33 = BV::from_u64(0x3333_3333_3333_3333, w);
    let m0f = BV::from_u64(0x0f0f_0f0f_0f0f_0f0f, w);
    let m01 = BV::from_u64(0x0101_0101_0101_0101, w);
    c = c.bvsub(&c.bvlshr(&sh(1)).bvand(&m55));
    c = c.bvand(&m33).bvadd(&c.bvlshr(&sh(2)).bvand(&m33));
    c = c.bvadd(&c.bvlshr(&sh(4))).bvand(&m0f);
    c.bvmul(&m01).bvlshr(&sh(56))
}

/// `ctz` via isolated lowest bit then CLZ: `x==0 ? w : w-1-clz(x & -x)`.
pub(crate) fn software_ctz(x: &BV) -> BV {
    let w = x.get_size();
    let lowest = x.bvand(&x.bvneg());
    let c = clz_ite(&lowest);
    x.eq(&BV::from_u64(0, w)).ite(
        &BV::from_u64(w as u64, w),
        &BV::from_u64(w as u64 - 1, w).bvsub(&c),
    )
}

/// compiler-rt-style byte swap: shift each byte into its mirrored slot.
pub(crate) fn software_bswap(x: &BV) -> BV {
    let w = x.get_size();
    let bytes = w / 8;
    let mut acc = BV::from_u64(0, w);
    for i in 0..bytes {
        let byte = x
            .bvlshr(&BV::from_u64(8 * i as u64, w))
            .bvand(&BV::from_u64(0xff, w));
        acc = acc.bvor(&byte.bvshl(&BV::from_u64(8 * (bytes - 1 - i) as u64, w)));
    }
    acc
}

/// Complementary shifts, XOR variant: `(x ≫ n) ⊕ (x ≪ (w-n))` with `n` masked.
pub(crate) fn software_ror(x: &BV, n: &BV) -> BV {
    let w = x.get_size();
    let nmask = n.bvand(&BV::from_u64((w - 1) as u64, w));
    let right = x.bvlshr(&nmask);
    let left = x.bvshl(&BV::from_u64(w as u64, w).bvsub(&nmask));
    right.bvxor(&left)
}

/// Complementary shifts, OR variant: `(x ≪ n) | (x ≫ (w-n))` with `n` masked.
pub(crate) fn software_rol(x: &BV, n: &BV) -> BV {
    let w = x.get_size();
    let nmask = n.bvand(&BV::from_u64((w - 1) as u64, w));
    let left = x.bvshl(&nmask);
    let right = x.bvlshr(&BV::from_u64(w as u64, w).bvsub(&nmask));
    left.bvor(&right)
}

fn rotl_const(x: &BV, n: u32) -> BV {
    let w = x.get_size();
    x.bvshl(&BV::from_u64(n as u64, w))
        .bvor(&x.bvlshr(&BV::from_u64((w - n) as u64, w)))
}

/// compiler-rt SIPROUND: complementary-shift rotates (13/16/17/21/32) plus xor/add.
pub(crate) fn software_sipround(v0: &BV, v1: &BV, v2: &BV, v3: &BV) -> [BV; 4] {
    let mut v0 = v0.bvadd(v1);
    let mut v1 = rotl_const(v1, 13);
    v1 = v1.bvxor(&v0);
    v0 = rotl_const(&v0, 32);
    let mut v2 = v2.bvadd(v3);
    let mut v3 = rotl_const(v3, 16);
    v3 = v3.bvxor(&v2);
    v0 = v0.bvadd(&v3);
    v3 = rotl_const(&v3, 21);
    v3 = v3.bvxor(&v0);
    v2 = v2.bvadd(&v1);
    v1 = rotl_const(&v1, 17);
    v1 = v1.bvxor(&v2);
    v2 = rotl_const(&v2, 32);
    [v0, v1, v2, v3]
}

/// compiler-rt-style store smear: each byte of `v` extracted after `≫ 8*i`.
pub(crate) fn stored_dword_smear(v: &BV) -> BV {
    let mut word = v.bvlshr(&BV::from_u64(0, 64)).extract(7, 0);
    for i in 1..8 {
        let b = v.bvlshr(&BV::from_u64((i * 8) as u64, 64)).extract(7, 0);
        word = b.concat(&word);
    }
    word
}
pub(crate) fn rent_sysvar_halves() -> (BV, BV) {
    let uif = FuncDecl::new(
        "uif_sol_get_rent_sysvar",
        &[&Sort::bitvector(64)],
        &Sort::bitvector(128),
    );
    let wide = uif
        .apply(&[&BV::from_u64(0, 64)])
        .as_bv()
        .expect("rent sysvar");
    (wide.extract(63, 0), wide.extract(127, 64))
}

/// LLVM-style u64→f64 pack with round-to-nearest-even (no software mul).
pub(crate) fn software_uitofp(x: &BV) -> BV {
    let clz = clz_ite(x);
    let norm = x.bvshl(&clz);
    let man52 = norm.extract(62, 11);
    let guard = norm.extract(10, 10);
    let low = norm.extract(9, 0);
    let lsb = man52.extract(0, 0);
    let one1 = BV::from_u64(1, 1);
    let sticky_or_round = low.eq(&BV::from_u64(0, 10)).not();
    let inc = Bool::and(&[
        &guard.eq(&one1),
        &Bool::or(&[&lsb.eq(&one1), &sticky_or_round]),
    ]);
    let man_ext = man52.zero_ext(1);
    let man_inc = inc.ite(&man_ext.bvadd(&BV::from_u64(1, 53)), &man_ext);
    let overflow = man_inc.extract(52, 52);
    let man_out = man_inc.extract(51, 0);
    let exp = BV::from_u64(1086, 64).bvsub(&clz);
    let exp_adj = overflow
        .eq(&one1)
        .ite(&exp.bvadd(&BV::from_u64(1, 64)), &exp);
    let packed = BV::from_u64(0, 1)
        .concat(&exp_adj.extract(10, 0))
        .concat(&man_out);
    x.eq(&BV::from_u64(0, 64))
        .ite(&BV::from_u64(0, 64), &packed)
}

pub(crate) fn bv_equiv(a: &BV, b: &BV) -> bool {
    matches!(bv_equiv_result(a, b), SatResult::Unsat | SatResult::Unknown)
}

pub(crate) fn free_bv_consts(expr: &dyn Ast) -> Vec<BV> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for_each_app(expr, |node, name| {
        if node.num_children() == 0 {
            if let Some(bv) = node.as_bv() {
                if !matches!(bv.decl().kind(), DeclKind::Bnum) && seen.insert(name.to_string()) {
                    out.push(bv);
                }
            }
        }
        false
    });
    out
}

fn bool_equiv(a: &Bool, b: &Bool) -> bool {
    let eq = a.simplify().eq(&b.simplify());
    if eq.simplify().as_bool() == Some(true) {
        return true;
    }
    let solver = Solver::new();
    let mut params = Params::new();
    params.set_u32("timeout", PROOF_TIMEOUT_MS);
    solver.set_params(&params);
    solver.assert(&eq.not());
    matches!(solver.check(), SatResult::Unsat | SatResult::Unknown)
}

fn ast_equiv(a: &dyn Ast, b: &dyn Ast) -> bool {
    let da = Dynamic::from_ast(a);
    let db = Dynamic::from_ast(b);
    if let (Some(x), Some(y)) = (da.as_bv(), db.as_bv()) {
        return bv_equiv(&x, &y);
    }
    if let (Some(x), Some(y)) = (da.as_bool(), db.as_bool()) {
        return bool_equiv(&x, &y);
    }
    false
}

fn uninterpreted_apply_names(expr: &dyn Ast) -> HashSet<String> {
    let mut names = HashSet::new();
    for_each_app(expr, |node, name| {
        if node.decl().kind() == DeclKind::Uninterpreted && node.num_children() > 0 {
            names.insert(name.to_string());
        }
        false
    });
    names
}

/// Accept gate for a rewrite: either Z3 cannot refute `got == src`, or `got`
/// introduced only catalog UIFs (`uitofp`, `clz`, …). Extract/zext wrappers
/// around those UIFs are allowed. Opaque UIFs that were already in `src`
/// (sysvars) may remain. A timeout is treated as equivalent, matching
/// [`bv_equiv`].
pub(crate) fn rewrite_result_is_legal(src: &dyn Ast, got: &dyn Ast) -> bool {
    if ast_equiv(src, got) {
        return true;
    }
    let src_uifs = uninterpreted_apply_names(src);
    let mut saw_recovered_uif = false;
    let mut illegal_new = false;
    for_each_app(got, |node, name| {
        if node.decl().kind() != DeclKind::Uninterpreted || node.num_children() == 0 {
            return false;
        }
        if is_recovered_uif(name) {
            saw_recovered_uif = true;
            return false;
        }
        if !src_uifs.contains(name) {
            illegal_new = true;
            return true;
        }
        false
    });
    saw_recovered_uif && !illegal_new
}

fn assert_bv_sound(src: BV) {
    let got = alu(&src, &[]);
    assert!(
        rewrite_result_is_legal(&src, &got),
        "rewrite of {src} produced {got}"
    );
}

fn assert_bool_sound(src: Bool) {
    let got = branch(&src, &[]);
    assert!(
        rewrite_result_is_legal(&src, &got),
        "rewrite of {src} produced {got}"
    );
}

#[test]
fn swar_clz_samples_like_leading_zeros() {
    let x = BV::new_const("w", 64);
    let e = swar_clz(&x);
    for v in [0u64, 1, 2, 70, 1 << 63, u64::MAX] {
        assert_eq!(eval_at(&e, &x, v), Some(v.leading_zeros() as u64), "v={v}");
    }
}

#[test]
fn legal_when_bitvector_equivalent() {
    let x = BV::new_const("w", 64);
    let e = x.bvmul(&BV::from_u64(3, 64));
    assert!(rewrite_result_is_legal(&e, &e));
    assert!(rewrite_result_is_legal(
        &e,
        &x.bvmul(&BV::from_u64(1, 64)).bvmul(&BV::from_u64(3, 64))
    ));
}

#[test]
fn legal_when_got_is_named_rewrite_uif() {
    let x = BV::new_const("w", 64);
    let src = x.bvshl(&BV::from_u64(1, 64));
    let u = uif1("uitofp", &x, 64);
    assert!(rewrite_result_is_legal(&src, &u));
    assert!(rewrite_result_is_legal(&src, &u.extract(51, 0)));
}

#[test]
fn illegal_when_neither_equivalent_nor_catalog_uif() {
    let x = BV::new_const("w", 64);
    let y = BV::new_const("w2", 64);
    // Distinct consts: Z3 refutes equality immediately. (A 100ms timeout
    // counts as equivalent, so we do not use hard-to-disprove ALU pairs.)
    assert!(!rewrite_result_is_legal(&x, &y));
    assert!(!rewrite_result_is_legal(
        &x.bvmul(&y),
        &uif1("mystery", &x, 64)
    ));
}

#[test]
fn opaque_src_uifs_may_remain_under_a_catalog_uif() {
    let arg = BV::from_u64(0, 64);
    let sys = FuncDecl::new(
        "uif_sol_get_rent_sysvar",
        &[&Sort::bitvector(64)],
        &Sort::bitvector(64),
    )
    .apply(&[&arg])
    .as_bv()
    .unwrap();
    let got = uif1("uitofp", &sys, 64);
    assert!(rewrite_result_is_legal(&sys.bvshl(&arg), &got));
}

#[test]
fn rewrite_results_are_equiv_or_named_uifs() {
    let x = BV::new_const("w_40", 64);
    let y = BV::new_const("w_39", 64);

    assert_bv_sound(x.clone());
    assert_bv_sound(x.bvmul(&BV::from_u64(3, 64)));
    assert_bv_sound(x.bvand(&y));
    assert_bv_sound(split_mul_0x129(&x));
    assert_bv_sound(clz_ite(&x));
    assert_bv_sound(swar_clz(&x));
    assert_bv_sound(software_ctz(&x));
    assert_bv_sound(swar_popcnt(&x));
    assert_bv_sound(software_bswap(&x));
    assert_bv_sound(software_ror(&x, &y));
    assert_bv_sound(software_rol(&x, &y));
    let z = BV::new_const("w_38", 64);
    let w = BV::new_const("w_37", 64);
    assert_bv_sound(software_sipround(&x, &y, &z, &w)[0].clone());
    assert_bv_sound(software_uitofp(&x));
    assert_bv_sound(software_uitofp(&x).extract(62, 52).zero_ext(53));
    assert_bv_sound(software_uitofp(&x).extract(51, 0));
    assert_bv_sound(
        x.bvlshr(&BV::from_u64(52, 64))
            .bvand(&BV::from_u64(0x7ff, 64)),
    );
    assert_bv_sound(uif1("uitofp", &x, 64));

    let prod = uif2("f64_mul", &uif1("uitofp", &x, 64), &y, 64);
    assert_bool_sound(prod.bvsge(&BV::from_u64(0, 64)));
    assert_bool_sound(x.bvule(&y));
    let n = uif1("fptoui", &prod, 64);
    assert_bool_sound(n.bvugt(&BV::from_u64(1, 64)));
}
