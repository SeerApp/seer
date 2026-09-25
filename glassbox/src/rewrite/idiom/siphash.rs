//! SipHash — `sipround` / one-word `siphash13` / `siphash24`.
//!
//! SBPF has no rotate; compiler-rt expands SIPROUND into complementary
//! shifts plus xor/add. Four mixed `v` words are more than two sampling
//! atoms, so `ror` never fires. Sample against the paper round and mint a
//! UIF of those atoms. No SAT — a miss is one eval.

use z3::ast::BV;

use crate::rewrite::ast::{rust_sipc, rust_siphash13, rust_siphash24, rust_sipround, uif3, uif4};
use crate::rewrite::eval::eval_env;

const ROUND_NAMES: [&str; 4] = ["sipround0", "sipround1", "sipround2", "sipround3"];
const COMP_NAMES: [&str; 4] = ["sipc0", "sipc1", "sipc2", "sipc3"];
const D_NAMES: [&str; 4] = ["sipd0", "sipd1", "sipd2", "sipd3"];

const PERMS4: [[usize; 4]; 24] = [
    [0, 1, 2, 3],
    [0, 1, 3, 2],
    [0, 2, 1, 3],
    [0, 2, 3, 1],
    [0, 3, 1, 2],
    [0, 3, 2, 1],
    [1, 0, 2, 3],
    [1, 0, 3, 2],
    [1, 2, 0, 3],
    [1, 2, 3, 0],
    [1, 3, 0, 2],
    [1, 3, 2, 0],
    [2, 0, 1, 3],
    [2, 0, 3, 1],
    [2, 1, 0, 3],
    [2, 1, 3, 0],
    [2, 3, 0, 1],
    [2, 3, 1, 0],
    [3, 0, 1, 2],
    [3, 0, 2, 1],
    [3, 1, 0, 2],
    [3, 1, 2, 0],
    [3, 2, 0, 1],
    [3, 2, 1, 0],
];

const PERMS3: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

const SAMPLES4: [[u64; 4]; 5] = [
    [1, 2, 4, 8],
    [0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210, 7, 9],
    [u64::MAX, 1, 2, 3],
    [
        0x1111_1111_1111_1111,
        0x2222_2222_2222_2222,
        0x3333_3333_3333_3333,
        0x4444_4444_4444_4444,
    ],
    [0x9e37_79b9_7f4a_7c15, 0x7f4a_7c15_9e37_79b9, 13, 17],
];

const SAMPLES3: [[u64; 3]; 5] = [
    [1, 2, 4],
    [0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210, 7],
    [u64::MAX, 1, 2],
    [
        0x1111_1111_1111_1111,
        0x2222_2222_2222_2222,
        0x3333_3333_3333_3333,
    ],
    [0x9e37_79b9_7f4a_7c15, 0x7f4a_7c15_9e37_79b9, 13],
];

fn all64(vs: &[&BV]) -> bool {
    vs.iter().all(|v| v.get_size() == 64)
}

fn eval4(expr: &BV, vs: [&BV; 4], vals: [u64; 4]) -> Option<u64> {
    eval_env(
        expr,
        &[
            (vs[0], vals[0]),
            (vs[1], vals[1]),
            (vs[2], vals[2]),
            (vs[3], vals[3]),
        ],
    )
}

fn eval3(expr: &BV, vs: [&BV; 3], vals: [u64; 3]) -> Option<u64> {
    eval_env(
        expr,
        &[(vs[0], vals[0]), (vs[1], vals[1]), (vs[2], vals[2])],
    )
}

fn pick4<T: Copy>(xs: [T; 4], p: [usize; 4]) -> [T; 4] {
    [xs[p[0]], xs[p[1]], xs[p[2]], xs[p[3]]]
}

fn pick3<T: Copy>(xs: [T; 3], p: [usize; 3]) -> [T; 3] {
    [xs[p[0]], xs[p[1]], xs[p[2]]]
}

/// One SIPROUND of four 64-bit atoms. `sipd*` is the finalization round
/// (`v2 ^= 0xff` first).
pub(crate) fn detect4(expr: &BV, a: &BV, b: &BV, c: &BV, d: &BV) -> Option<BV> {
    if expr.get_size() != 64 || !all64(&[a, b, c, d]) {
        return None;
    }
    let vs = [a, b, c, d];
    let probe = SAMPLES4[0];
    let got = eval4(expr, vs, probe)?;
    for p in PERMS4 {
        let t = pick4(probe, p);
        let round = rust_sipround(t[0], t[1], t[2], t[3]);
        for i in 0..4 {
            if got == round[i]
                && confirm4(expr, vs, p, |v| rust_sipround(v[0], v[1], v[2], v[3])[i])
            {
                let args = pick4(vs, p);
                return Some(uif4(ROUND_NAMES[i], args[0], args[1], args[2], args[3], 64));
            }
        }
        let dround = rust_sipround(t[0], t[1], t[2] ^ 0xff, t[3]);
        for i in 0..4 {
            if got == dround[i]
                && confirm4(expr, vs, p, |v| {
                    rust_sipround(v[0], v[1], v[2] ^ 0xff, v[3])[i]
                })
            {
                let args = pick4(vs, p);
                return Some(uif4(D_NAMES[i], args[0], args[1], args[2], args[3], 64));
            }
        }
    }
    None
}

fn confirm4(expr: &BV, vs: [&BV; 4], p: [usize; 4], want: impl Fn([u64; 4]) -> u64) -> bool {
    SAMPLES4
        .iter()
        .all(|&s| eval4(expr, vs, s) == Some(want(pick4(s, p))))
}

/// Init IV xor + first compression SIPROUND of `(k0, k1, m)`, or the
/// finished one-word SipHash-1-3 / 2-4 value.
pub(crate) fn detect3(expr: &BV, a: &BV, b: &BV, c: &BV) -> Option<BV> {
    if expr.get_size() != 64 || !all64(&[a, b, c]) {
        return None;
    }
    let vs = [a, b, c];
    let probe = SAMPLES3[0];
    let got = eval3(expr, vs, probe)?;
    for p in PERMS3 {
        let t = pick3(probe, p);
        let comp = rust_sipc(t[0], t[1], t[2]);
        for i in 0..4 {
            if got == comp[i] && confirm3(expr, vs, p, |v| rust_sipc(v[0], v[1], v[2])[i]) {
                let args = pick3(vs, p);
                return Some(uif3(COMP_NAMES[i], args[0], args[1], args[2], 64));
            }
        }
        if got == rust_siphash13(t[0], t[1], t[2])
            && confirm3(expr, vs, p, |v| rust_siphash13(v[0], v[1], v[2]))
        {
            let args = pick3(vs, p);
            return Some(uif3("siphash13", args[0], args[1], args[2], 64));
        }
        if got == rust_siphash24(t[0], t[1], t[2])
            && confirm3(expr, vs, p, |v| rust_siphash24(v[0], v[1], v[2]))
        {
            let args = pick3(vs, p);
            return Some(uif3("siphash24", args[0], args[1], args[2], 64));
        }
    }
    None
}

fn confirm3(expr: &BV, vs: [&BV; 3], p: [usize; 3], want: impl Fn([u64; 3]) -> u64) -> bool {
    SAMPLES3
        .iter()
        .all(|&s| eval3(expr, vs, s) == Some(want(pick3(s, p))))
}

#[cfg(test)]
mod tests {
    use super::detect4;
    use crate::rewrite::ast::{rust_sipc, rust_siphash13, rust_sipround, uif3};
    use crate::rewrite::eval::eval_env;
    use crate::rewrite::testing::software_sipround;
    use z3::ast::{Ast, BV};

    #[test]
    fn eval_minted_siphash13() {
        let k0 = BV::new_const("w_k0", 64);
        let k1 = BV::new_const("w_k1", 64);
        let m = BV::new_const("w_m", 64);
        let u = uif3("siphash13", &k0, &k1, &m, 64);
        assert_eq!(
            eval_env(&u, &[(&k0, 1), (&k1, 2), (&m, 4)]),
            Some(rust_siphash13(1, 2, 4))
        );
    }

    #[test]
    fn recovers_sipround_v0_from_compiler_shifts() {
        let v0 = BV::new_const("w_v0", 64);
        let v1 = BV::new_const("w_v1", 64);
        let v2 = BV::new_const("w_v2", 64);
        let v3 = BV::new_const("w_v3", 64);
        let blob = software_sipround(&v0, &v1, &v2, &v3)[0].clone();
        let got = crate::rewrite::alu(&blob, &[]);
        assert_eq!(got.decl().name(), "sipround0");
        assert_eq!(got.num_children(), 4);
        let binds = [(&v0, 1u64), (&v1, 2), (&v2, 4), (&v3, 8)];
        assert_eq!(eval_env(&got, &binds), Some(rust_sipround(1, 2, 4, 8)[0]));
    }

    #[test]
    fn detect4_picks_v3_and_perm() {
        let vs: [BV; 4] = std::array::from_fn(|i| BV::new_const(format!("w_{i}"), 64));
        let blob = software_sipround(&vs[2], &vs[0], &vs[3], &vs[1])[3].clone();
        let got = detect4(&blob, &vs[0], &vs[1], &vs[2], &vs[3]).unwrap();
        assert_eq!(got.decl().name(), "sipround3");
    }

    #[test]
    fn detect3_recovers_compression_round() {
        let k0 = BV::new_const("w_k0", 64);
        let k1 = BV::new_const("w_k1", 64);
        let m = BV::new_const("w_m", 64);
        let c0 = BV::from_u64(0x736f_6d65_7073_6575, 64);
        let c1 = BV::from_u64(0x646f_7261_6e64_6f6d, 64);
        let c2 = BV::from_u64(0x6c79_6765_6e65_7261, 64);
        let c3 = BV::from_u64(0x7465_6462_7974_6573, 64);
        let blob = software_sipround(
            &k0.bvxor(&c0),
            &k1.bvxor(&c1),
            &k0.bvxor(&c2),
            &k1.bvxor(&c3).bvxor(&m),
        )[1]
        .clone();
        let got = crate::rewrite::alu(&blob, &[]);
        assert_eq!(got.decl().name(), "sipc1");
        assert_eq!(
            eval_env(&got, &[(&k0, 1), (&k1, 2), (&m, 4)]),
            Some(rust_sipc(1, 2, 4)[1])
        );
    }

    #[test]
    fn recovers_sipd_after_v2_xor_ff() {
        let v0 = BV::new_const("w_v0", 64);
        let v1 = BV::new_const("w_v1", 64);
        let v2 = BV::new_const("w_v2", 64);
        let v3 = BV::new_const("w_v3", 64);
        let blob = software_sipround(&v0, &v1, &v2.bvxor(&BV::from_u64(0xff, 64)), &v3)[0].clone();
        let got = crate::rewrite::alu(&blob, &[]);
        assert_eq!(got.decl().name(), "sipd0");
    }

    #[test]
    fn recovers_nested_sipround_of_uifs() {
        let v0 = BV::new_const("w_v0", 64);
        let v1 = BV::new_const("w_v1", 64);
        let v2 = BV::new_const("w_v2", 64);
        let v3 = BV::new_const("w_v3", 64);
        let r1 = software_sipround(&v0, &v1, &v2, &v3);
        let uifs: [BV; 4] = std::array::from_fn(|i| crate::rewrite::alu(&r1[i], &[]));
        assert_eq!(uifs[0].decl().name(), "sipround0");
        let blob = software_sipround(&uifs[0], &uifs[1], &uifs[2], &uifs[3])[0].clone();
        let got = crate::rewrite::alu(&blob, &[]);
        assert_eq!(got.decl().name(), "sipround0");
        assert_eq!(got.num_children(), 4);
        for i in 0..4 {
            let n = got.nth_child(i).unwrap().as_bv().unwrap().decl().name();
            assert!(
                n.starts_with("sipround") || n == "ror",
                "child {i} is {n}"
            );
        }
        // v0/v1 are interchangeable in sipround0; check the value, not arg order.
        let binds = [(&v0, 1u64), (&v1, 2), (&v2, 4), (&v3, 8)];
        let inner = rust_sipround(1, 2, 4, 8);
        assert_eq!(
            eval_env(&got, &binds),
            Some(rust_sipround(inner[0], inner[1], inner[2], inner[3])[0])
        );
    }

    #[test]
    fn four_way_xor_is_not_sipround() {
        let vs: [BV; 4] = std::array::from_fn(|i| BV::new_const(format!("w_{i}"), 64));
        let blob = vs[0].bvxor(&vs[1]).bvxor(&vs[2]).bvxor(&vs[3]);
        assert!(detect4(&blob, &vs[0], &vs[1], &vs[2], &vs[3]).is_none());
    }
}
