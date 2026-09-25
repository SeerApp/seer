//! Shared Z3 helpers: UIFs, extract windows, node counts, and BV equivalence.

use z3::ast::{Ast, Dynamic, BV};
use z3::{FuncDecl, SatResult, Sort};

use crate::astwalk::{ast_id, for_each_app};

pub(crate) const MIN_NODES: usize = 6;
pub(crate) const PROOF_TIMEOUT_MS: u32 = 100;

pub(crate) fn uif1(name: &str, x: &BV, ret_w: u32) -> BV {
    FuncDecl::new(
        name,
        &[&Sort::bitvector(x.get_size())],
        &Sort::bitvector(ret_w),
    )
    .apply(&[x])
    .as_bv()
    .expect("uif1 sort")
}

pub(crate) fn uif2(name: &str, a: &BV, b: &BV, ret_w: u32) -> BV {
    FuncDecl::new(
        name,
        &[
            &Sort::bitvector(a.get_size()),
            &Sort::bitvector(b.get_size()),
        ],
        &Sort::bitvector(ret_w),
    )
    .apply(&[a, b])
    .as_bv()
    .expect("uif2 sort")
}

pub(crate) fn uif3(name: &str, a: &BV, b: &BV, c: &BV, ret_w: u32) -> BV {
    FuncDecl::new(
        name,
        &[
            &Sort::bitvector(a.get_size()),
            &Sort::bitvector(b.get_size()),
            &Sort::bitvector(c.get_size()),
        ],
        &Sort::bitvector(ret_w),
    )
    .apply(&[a, b, c])
    .as_bv()
    .expect("uif3 sort")
}

pub(crate) fn uif4(name: &str, a: &BV, b: &BV, c: &BV, d: &BV, ret_w: u32) -> BV {
    FuncDecl::new(
        name,
        &[
            &Sort::bitvector(a.get_size()),
            &Sort::bitvector(b.get_size()),
            &Sort::bitvector(c.get_size()),
            &Sort::bitvector(d.get_size()),
        ],
        &Sort::bitvector(ret_w),
    )
    .apply(&[a, b, c, d])
    .as_bv()
    .expect("uif4 sort")
}

pub(crate) fn extract_hi_lo(ast: &Dynamic) -> Option<(u32, u32)> {
    let ctx = ast.get_ctx().get_z3_context();
    unsafe {
        let app = z3_sys::Z3_to_app(ctx, ast.get_z3_ast())?;
        let decl = z3_sys::Z3_get_app_decl(ctx, app)?;
        if z3_sys::Z3_get_decl_num_parameters(ctx, decl) < 2 {
            return None;
        }
        Some((
            z3_sys::Z3_get_decl_int_parameter(ctx, decl, 0) as u32,
            z3_sys::Z3_get_decl_int_parameter(ctx, decl, 1) as u32,
        ))
    }
}

fn rust_mask(x: u64, width: u32) -> u64 {
    if width == 0 {
        0
    } else if width >= 64 {
        x
    } else {
        x & ((1u64 << width) - 1)
    }
}

pub(crate) fn rust_clz(x: u64, width: u32) -> u64 {
    if width == 0 {
        return 0;
    }
    if width >= 64 {
        return x.leading_zeros() as u64;
    }
    if x == 0 {
        return width as u64;
    }
    (x << (64 - width)).leading_zeros() as u64
}

pub(crate) fn rust_ctz(x: u64, width: u32) -> u64 {
    if width == 0 {
        return 0;
    }
    let x = rust_mask(x, width);
    if x == 0 {
        return width as u64;
    }
    x.trailing_zeros() as u64
}

pub(crate) fn rust_popcnt(x: u64, width: u32) -> u64 {
    rust_mask(x, width).count_ones() as u64
}

pub(crate) fn rust_bswap(x: u64, width: u32) -> u64 {
    match width {
        16 => (x as u16).swap_bytes() as u64,
        32 => (x as u32).swap_bytes() as u64,
        64 => x.swap_bytes(),
        _ => rust_mask(x, width),
    }
}

pub(crate) fn rust_ror(x: u64, n: u64, width: u32) -> u64 {
    let x = rust_mask(x, width);
    if width == 0 {
        return 0;
    }
    let n = (n % width as u64) as u32;
    if n == 0 {
        return x;
    }
    rust_mask((x >> n) | (x << (width - n)), width)
}

pub(crate) fn rust_rol(x: u64, n: u64, width: u32) -> u64 {
    let x = rust_mask(x, width);
    if width == 0 {
        return 0;
    }
    let n = (n % width as u64) as u32;
    if n == 0 {
        return x;
    }
    rust_mask((x << n) | (x >> (width - n)), width)
}

/// SipHash IV ("somepseudorandomlygeneratedbytes").
const SIP_C0: u64 = 0x736f_6d65_7073_6575;
const SIP_C1: u64 = 0x646f_7261_6e64_6f6d;
const SIP_C2: u64 = 0x6c79_6765_6e65_7261;
const SIP_C3: u64 = 0x7465_6462_7974_6573;

pub(crate) fn rust_sipround(v0: u64, v1: u64, v2: u64, v3: u64) -> [u64; 4] {
    let mut v0 = v0.wrapping_add(v1);
    let mut v1 = v1.rotate_left(13);
    v1 ^= v0;
    v0 = v0.rotate_left(32);
    let mut v2 = v2.wrapping_add(v3);
    let mut v3 = v3.rotate_left(16);
    v3 ^= v2;
    v0 = v0.wrapping_add(v3);
    v3 = v3.rotate_left(21);
    v3 ^= v0;
    v2 = v2.wrapping_add(v1);
    v1 = v1.rotate_left(17);
    v1 ^= v2;
    v2 = v2.rotate_left(32);
    [v0, v1, v2, v3]
}

fn rust_sip_init(k0: u64, k1: u64, m: u64) -> [u64; 4] {
    [k0 ^ SIP_C0, k1 ^ SIP_C1, k0 ^ SIP_C2, k1 ^ SIP_C3 ^ m]
}

pub(crate) fn rust_sipc(k0: u64, k1: u64, m: u64) -> [u64; 4] {
    let [v0, v1, v2, v3] = rust_sip_init(k0, k1, m);
    rust_sipround(v0, v1, v2, v3)
}

fn rust_siphash(c_rounds: u32, d_rounds: u32, k0: u64, k1: u64, m: u64) -> u64 {
    let [mut v0, mut v1, mut v2, mut v3] = rust_sip_init(k0, k1, m);
    for _ in 0..c_rounds {
        [v0, v1, v2, v3] = rust_sipround(v0, v1, v2, v3);
    }
    v0 ^= m;
    v2 ^= 0xff;
    for _ in 0..d_rounds {
        [v0, v1, v2, v3] = rust_sipround(v0, v1, v2, v3);
    }
    v0 ^ v1 ^ v2 ^ v3
}

pub(crate) fn rust_siphash13(k0: u64, k1: u64, m: u64) -> u64 {
    rust_siphash(1, 3, k0, k1, m)
}

pub(crate) fn rust_siphash24(k0: u64, k1: u64, m: u64) -> u64 {
    rust_siphash(2, 4, k0, k1, m)
}

pub(crate) fn bv_equiv_result(a: &BV, b: &BV) -> SatResult {
    if a.get_size() != b.get_size() {
        return SatResult::Sat;
    }
    // Don't `simplify`: that's unpreemptable and hangs on compiler-rt blobs.
    // The solver timeout is the think budget.
    if ast_id(a) == ast_id(b) {
        return SatResult::Unsat;
    }
    crate::sat::check(&a.eq(b).not(), PROOF_TIMEOUT_MS)
}

pub(crate) fn unique_nodes(expr: &dyn Ast) -> usize {
    let mut n = 0usize;
    for_each_app(expr, |_, _| {
        n += 1;
        false
    });
    n
}

/// Stop after `cap` so flatten/eval can cheap-bail on compiler-rt blobs.
pub(crate) fn unique_nodes_at_most(expr: &dyn Ast, cap: usize) -> usize {
    let mut n = 0usize;
    for_each_app(expr, |_, _| {
        n += 1;
        n >= cap
    });
    n
}
