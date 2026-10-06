//! Concrete sampling evaluator over rewritten Z3 DAGs.
//!
//! Bind free 64-bit atoms, fold ALU/UIF nodes, and decide whether a guessed
//! compact form matches the bloated compiler DAG on enough samples.

use std::cell::RefCell;
use std::collections::HashMap;

use z3::ast::{Ast, Bool, Dynamic, BV};
use z3::DeclKind;

use crate::astwalk::ast_id;

use super::ast::{
    extract_hi_lo, rust_bswap, rust_clz, rust_ctz, rust_popcnt, rust_rol, rust_ror, rust_sipc,
    rust_siphash13, rust_siphash24, rust_sipround,
};
mod pack;
mod samples;

pub(crate) use pack::*;
pub(crate) use samples::*;

pub(crate) fn eval_bool_env(formula: &Bool, binds: &[(&BV, u64)]) -> Option<bool> {
    eval_bool(&Dynamic::from(formula), &EvalEnv::from_binds(binds))
}

pub(crate) fn eval_at(expr: &BV, var: &BV, val: u64) -> Option<u64> {
    eval_env(expr, &[(var, val)])
}

pub(crate) fn eval_env(expr: &BV, binds: &[(&BV, u64)]) -> Option<u64> {
    eval_dyn(&Dynamic::from(expr), &EvalEnv::from_binds(binds))
}

pub(crate) struct EvalEnv {
    by_id: HashMap<usize, u64>,
    by_name: HashMap<String, u64>,
    /// Bound extracts of a parent: `parent_id -> [(hi, lo, value)]`.
    slices: HashMap<usize, Vec<(u32, u32, u64)>>,
    /// Bound `parent ≫ k`: `parent_id -> [(k, value)]`.
    shifts: HashMap<usize, Vec<(u32, u64)>>,
    bv_memo: RefCell<HashMap<usize, Option<u64>>>,
    bool_memo: RefCell<HashMap<usize, Option<bool>>>,
}

impl EvalEnv {
    fn from_binds(binds: &[(&BV, u64)]) -> Self {
        let mut by_id = HashMap::new();
        let mut by_name = HashMap::new();
        let mut slices: HashMap<usize, Vec<(u32, u32, u64)>> = HashMap::new();
        let mut shifts: HashMap<usize, Vec<(u32, u64)>> = HashMap::new();
        for &(var, val) in binds {
            by_id.insert(ast_id(var), val);
            if var.num_children() == 0 {
                by_name.insert(var.decl().name(), val);
            }
            let d = Dynamic::from(var);
            match d.decl().kind() {
                DeclKind::Extract => {
                    if let (Some((hi, lo)), Some(src)) = (extract_hi_lo(&d), d.nth_child(0)) {
                        slices.entry(ast_id(&src)).or_default().push((hi, lo, val));
                    }
                }
                DeclKind::Blshr => {
                    if let (Some(src), Some(k)) = (
                        d.nth_child(0).and_then(|c| c.as_bv()),
                        d.nth_child(1)
                            .and_then(|c| c.as_bv())
                            .and_then(|c| c.as_u64()),
                    ) {
                        if k < 64 {
                            shifts
                                .entry(ast_id(&src))
                                .or_default()
                                .push((k as u32, val));
                        }
                    }
                }
                _ => {}
            }
            if is_pure_byte_pack(var) {
                bind_pack_bytes(&d, val, &mut by_id);
            }
        }
        Self {
            by_id,
            by_name,
            slices,
            shifts,
            bv_memo: RefCell::new(HashMap::new()),
            bool_memo: RefCell::new(HashMap::new()),
        }
    }

    fn lookup(&self, d: &Dynamic, width: u32) -> Option<u64> {
        if let Some(&v) = self.by_id.get(&ast_id(d)) {
            return Some(mask_width(v, width));
        }
        if d.num_children() == 0 {
            return self
                .by_name
                .get(&d.decl().name())
                .copied()
                .map(|v| mask_width(v, width));
        }
        self.lookup_slice(d, width)
            .or_else(|| self.lookup_shift(d, width))
    }

    fn lookup_slice(&self, d: &Dynamic, width: u32) -> Option<u64> {
        if d.decl().kind() != DeclKind::Extract {
            return None;
        }
        let (hi, lo) = extract_hi_lo(d)?;
        let src = d.nth_child(0)?;
        if let Some(&v) = self.by_id.get(&ast_id(&src)) {
            return Some(mask_width(
                (v >> lo) & mask_width(u64::MAX, hi - lo + 1),
                width,
            ));
        }
        if let Some(slices) = self.slices.get(&ast_id(&src)) {
            for &(bhi, blo, val) in slices {
                if bhi >= hi && blo <= lo {
                    let bits = (val >> (lo - blo)) & mask_width(u64::MAX, hi - lo + 1);
                    return Some(mask_width(bits, width));
                }
            }
        }
        for &(k, val) in self.shifts.get(&ast_id(&src))? {
            if lo >= k && hi - k < 64 {
                let bits = (val >> (lo - k)) & mask_width(u64::MAX, hi - lo + 1);
                return Some(mask_width(bits, width));
            }
        }
        None
    }

    fn lookup_shift(&self, d: &Dynamic, width: u32) -> Option<u64> {
        if d.decl().kind() != DeclKind::Blshr {
            return None;
        }
        let src = d.nth_child(0)?;
        let k = d.nth_child(1)?.as_bv()?.as_u64()? as u32;
        for &(k0, val) in self.shifts.get(&ast_id(&src))? {
            if k >= k0 {
                let dlt = k - k0;
                let bits = if dlt >= 64 { 0 } else { val >> dlt };
                return Some(mask_width(bits, width));
            }
        }
        None
    }
}

pub(crate) fn mask_width(v: u64, width: u32) -> u64 {
    if width == 0 {
        0
    } else if width >= 64 {
        v
    } else {
        v & ((1u64 << width) - 1)
    }
}

fn sip_out(name: &str, prefix: &str) -> Option<usize> {
    let rest = name.strip_prefix(prefix)?;
    let i = rest.parse::<usize>().ok()?;
    (i < 4).then_some(i)
}

fn eval_sip_uif(name: &str, ev: impl Fn(usize) -> Option<u64>) -> Option<u64> {
    if let Some(i) = sip_out(name, "sipround") {
        return Some(rust_sipround(ev(0)?, ev(1)?, ev(2)?, ev(3)?)[i]);
    }
    if let Some(i) = sip_out(name, "sipc") {
        return Some(rust_sipc(ev(0)?, ev(1)?, ev(2)?)[i]);
    }
    if let Some(i) = sip_out(name, "sipd") {
        return Some(rust_sipround(ev(0)?, ev(1)?, ev(2)? ^ 0xff, ev(3)?)[i]);
    }
    match name {
        "siphash13" => Some(rust_siphash13(ev(0)?, ev(1)?, ev(2)?)),
        "siphash24" => Some(rust_siphash24(ev(0)?, ev(1)?, ev(2)?)),
        _ => None,
    }
}

pub(crate) fn eval_dyn(d: &Dynamic, env: &EvalEnv) -> Option<u64> {
    let id = ast_id(d);
    if let Some(&cached) = env.bv_memo.borrow().get(&id) {
        return cached;
    }
    let v = eval_dyn_uncached(d, env);
    env.bv_memo.borrow_mut().insert(id, v);
    v
}

fn eval_dyn_uncached(d: &Dynamic, env: &EvalEnv) -> Option<u64> {
    if let Some(bv) = d.as_bv() {
        if let Some(v) = bv.as_u64() {
            return Some(v);
        }
        let w = bv.get_size();
        if let Some(v) = env.lookup(d, w) {
            return Some(v);
        }
        if d.num_children() == 0 {
            return None;
        }
        let n = d.num_children();
        let ev = |i: usize| d.nth_child(i).and_then(|c| eval_dyn(&c, env));
        let child_w = |i: usize| d.nth_child(i).and_then(|c| c.as_bv()).map(|b| b.get_size());
        let v = match d.decl().kind() {
            DeclKind::Uninterpreted => match d.decl().name().as_str() {
                "clz" => rust_clz(ev(0)?, child_w(0)?),
                "ctz" => rust_ctz(ev(0)?, child_w(0)?),
                "popcnt" => rust_popcnt(ev(0)?, child_w(0)?),
                "bswap" => rust_bswap(ev(0)?, child_w(0)?),
                "ror" => rust_ror(ev(0)?, ev(1)?, child_w(0)?),
                "rol" => rust_rol(ev(0)?, ev(1)?, child_w(0)?),
                "uitofp" => (ev(0)? as f64).to_bits(),
                "sitofp" => (ev(0)? as i64 as f64).to_bits(),
                "f64_mul" => f64_mul_bits(Some(ev(0)?), Some(ev(1)?))?,
                "f64_exp" => (ev(0)? >> 52) & 0x7ff,
                "f64_sign" => ev(0)? >> 63,
                "f64_mant" => ev(0)? & ((1u64 << 52) - 1),
                "fptoui" => {
                    let f = f64::from_bits(ev(0)?);
                    if !f.is_finite() || f < 0.0 {
                        return None;
                    }
                    f as u64
                }
                "fptosi" => (f64::from_bits(ev(0)?) as i64) as u64,
                "umul128_hi" => (((ev(0)? as u128) * (ev(1)? as u128)) >> 64) as u64,
                "umul128_lo" => ((ev(0)? as u128) * (ev(1)? as u128)) as u64,
                n => eval_sip_uif(n, ev)?,
            },
            DeclKind::Badd => {
                let mut acc = ev(0)?;
                for i in 1..n {
                    acc = acc.wrapping_add(ev(i)?);
                }
                acc
            }
            DeclKind::Bsub => ev(0)?.wrapping_sub(ev(1)?),
            DeclKind::Bmul => {
                let mut acc = ev(0)?;
                for i in 1..n {
                    acc = acc.wrapping_mul(ev(i)?);
                }
                acc
            }
            DeclKind::Band => {
                let mut acc = ev(0)?;
                for i in 1..n {
                    acc &= ev(i)?;
                }
                acc
            }
            DeclKind::Bor => {
                let mut acc = ev(0)?;
                for i in 1..n {
                    acc |= ev(i)?;
                }
                acc
            }
            DeclKind::Bxor => {
                let mut acc = ev(0)?;
                for i in 1..n {
                    acc ^= ev(i)?;
                }
                acc
            }
            DeclKind::Bshl => {
                let a = ev(0)?;
                let s = ev(1)?;
                if s >= w as u64 {
                    0
                } else {
                    a << s
                }
            }
            DeclKind::Blshr => {
                let a = ev(0)?;
                let s = ev(1)?;
                if s >= w as u64 {
                    0
                } else {
                    a >> s
                }
            }
            DeclKind::Bashr => {
                let a = ev(0)?;
                let s = ev(1)?;
                let signed = if w == 64 {
                    a as i64
                } else if a & (1u64 << (w - 1)) != 0 {
                    (a as i64) | !mask_width(u64::MAX, w) as i64
                } else {
                    a as i64
                };
                if s >= w as u64 {
                    if signed < 0 {
                        mask_width(u64::MAX, w)
                    } else {
                        0
                    }
                } else {
                    ((signed >> s) as u64) & mask_width(u64::MAX, w)
                }
            }
            DeclKind::Bnot => !ev(0)?,
            DeclKind::Bneg => ev(0)?.wrapping_neg(),
            DeclKind::Concat => {
                let mut acc = 0u64;
                for i in 0..n {
                    let cw = child_w(i)?;
                    acc = (acc << cw) | mask_width(ev(i)?, cw);
                }
                acc
            }
            DeclKind::Extract => {
                let (hi, lo) = extract_hi_lo(d)?;
                (ev(0)? >> lo) & mask_width(u64::MAX, hi - lo + 1)
            }
            DeclKind::ZeroExt => ev(0)?,
            DeclKind::SignExt => {
                let iw = child_w(0)?;
                let a = ev(0)?;
                if iw == 0 || iw >= 64 {
                    a
                } else if a & (1u64 << (iw - 1)) != 0 {
                    a | !mask_width(u64::MAX, iw)
                } else {
                    a
                }
            }
            DeclKind::Ite => {
                if eval_bool(&d.nth_child(0)?, env)? {
                    ev(1)?
                } else {
                    ev(2)?
                }
            }
            _ => return None,
        };
        return Some(mask_width(v, w));
    }
    None
}

pub(crate) fn eval_bool(d: &Dynamic, env: &EvalEnv) -> Option<bool> {
    let id = ast_id(d);
    if let Some(&cached) = env.bool_memo.borrow().get(&id) {
        return cached;
    }
    let v = eval_bool_uncached(d, env);
    env.bool_memo.borrow_mut().insert(id, v);
    v
}

fn eval_bool_uncached(d: &Dynamic, env: &EvalEnv) -> Option<bool> {
    if let Some(b) = d.as_bool() {
        if let Some(v) = b.as_bool() {
            return Some(v);
        }
    }
    let n = d.num_children();
    let evb = |i: usize| d.nth_child(i).and_then(|c| eval_bool(&c, env));
    let evv = |i: usize| d.nth_child(i).and_then(|c| eval_dyn(&c, env));
    match d.decl().kind() {
        DeclKind::True => Some(true),
        DeclKind::False => Some(false),
        DeclKind::And => {
            for i in 0..n {
                if !evb(i)? {
                    return Some(false);
                }
            }
            Some(true)
        }
        DeclKind::Or => {
            for i in 0..n {
                if evb(i)? {
                    return Some(true);
                }
            }
            Some(false)
        }
        DeclKind::Not => Some(!evb(0)?),
        DeclKind::Eq => {
            if d.nth_child(0)
                .as_ref()
                .is_some_and(|c| c.as_bool().is_some())
            {
                Some(evb(0)? == evb(1)?)
            } else {
                Some(evv(0)? == evv(1)?)
            }
        }
        DeclKind::Ite => {
            if evb(0)? {
                evb(1)
            } else {
                evb(2)
            }
        }
        DeclKind::Uleq => Some(evv(0)? <= evv(1)?),
        DeclKind::Ult => Some(evv(0)? < evv(1)?),
        DeclKind::Ugeq => Some(evv(0)? >= evv(1)?),
        DeclKind::Ugt => Some(evv(0)? > evv(1)?),
        DeclKind::Sleq => Some((evv(0)? as i64) <= (evv(1)? as i64)),
        DeclKind::Slt => Some((evv(0)? as i64) < (evv(1)? as i64)),
        DeclKind::Sgeq => Some((evv(0)? as i64) >= (evv(1)? as i64)),
        DeclKind::Sgt => Some((evv(0)? as i64) > (evv(1)? as i64)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rewrite::ast::{uif1, uif2};
    use crate::rewrite::idiom::clz_uif;
    use z3::ast::BV;
    use z3::{FuncDecl, Sort};

    #[test]
    fn eval_miss_is_linear_on_reconvergent_dag() {
        let x = BV::new_const("w_1", 64);
        let mut acc = x.clone();
        for _ in 0..24 {
            acc = acc.bvxor(&acc).bvadd(&acc);
        }
        let t = std::time::Instant::now();
        assert_eq!(eval_env(&acc, &[]), None);
        assert!(
            t.elapsed().as_millis() < 100,
            "unbound eval re-walked a shared DAG"
        );
    }

    #[test]
    fn eval_folds_bit_uifs() {
        let x = BV::new_const("w", 64);
        let n = BV::new_const("k", 64);
        assert_eq!(eval_at(&uif1("ctz", &x, 64), &x, 8), Some(3));
        assert_eq!(eval_at(&uif1("popcnt", &x, 64), &x, 7), Some(3));
        assert_eq!(eval_at(&uif1("bswap", &x, 64), &x, 1), Some(1u64 << 56));
        let ror = uif2("ror", &x, &n, 64);
        assert_eq!(eval_env(&ror, &[(&x, 1), (&n, 8)]), Some(1u64 << 56));
        let rol = uif2("rol", &x, &n, 64);
        assert_eq!(eval_env(&rol, &[(&x, 1), (&n, 8)]), Some(1u64 << 8));
    }

    #[test]
    fn eval_folds_clz_uif() {
        let x = BV::new_const("w", 64);
        let e = clz_uif(&x);
        assert_eq!(eval_at(&e, &x, 1), Some(63));
        assert_eq!(eval_at(&e, &x, 0), Some(64));
        assert_eq!(eval_at(&e, &x, 1u64 << 63), Some(0));
    }

    #[test]
    fn eval_folds_fptoui_uif() {
        let y = BV::new_const("w_float", 64);
        let u = uif1("fptoui", &y, 64);
        assert_eq!(eval_env(&u, &[(&y, 2.5f64.to_bits())]), Some(2));
        assert_eq!(eval_env(&u, &[(&y, 1.0f64.to_bits())]), Some(1));
        assert_eq!(eval_env(&u, &[(&y, 297.0f64.to_bits())]), Some(297));
    }

    fn rent_slices() -> (BV, BV, BV) {
        let uif = FuncDecl::new(
            "uif_sol_get_rent_sysvar",
            &[&Sort::bitvector(64)],
            &Sort::bitvector(128),
        );
        let wide = uif
            .apply(&[&BV::from_u64(0, 64)])
            .as_bv()
            .expect("rent sysvar");
        (wide.extract(63, 0), wide.extract(127, 64), wide)
    }

    #[test]
    fn eval_binds_distinct_uif_extracts() {
        let (lo, hi, _) = rent_slices();
        let sum = lo.bvadd(&hi);
        assert_eq!(eval_env(&sum, &[(&lo, 1), (&hi, 2)]), Some(3));
        assert_eq!(sampling_vars(&sum).len(), 2);
    }

    #[test]
    fn eval_covers_narrower_extract_of_bound_slice() {
        let (_, hi, wide) = rent_slices();
        let mid = wide.extract(115, 96).zero_ext(44);
        let hi_bits = 0x1234_5678_9abc_def0u64;
        assert_eq!(
            eval_env(&mid, &[(&hi, hi_bits)]),
            Some(((hi_bits >> 32) & 0xfffff) as u64)
        );
    }

    #[test]
    fn eval_covers_extract_of_parent_from_bound_shift() {
        let b = BV::new_const("w_b", 64);
        let b8 = b.bvlshr(&BV::from_u64(8, 64));
        let lo = b.extract(39, 8).zero_ext(32);
        let vb = 0x0123_4567_89ab_cdefu64;
        assert_eq!(eval_env(&lo, &[(&b8, vb)]), Some(vb & 0xffff_ffff));
        let hi = b.bvlshr(&BV::from_u64(40, 64));
        assert_eq!(eval_env(&hi, &[(&b8, vb)]), Some(vb >> 32));
    }
}
