//! Disjoint `|` — compiler shift-or pack. If each side occupies different
//! result bits (zeros may overlap), merge the non-zero windows like concat.
//! Overlapping data bits is a real OR; leave it opaque.

use std::collections::{HashMap, HashSet};

use z3::ast::{Ast, Dynamic, BV};

use super::bit::{merge_adjacent_slices, slices_width, zero_slice, BitSlice};
use super::bv_slices;

pub(super) fn split(
    bv: &BV,
    defs: &HashMap<String, BV>,
    expanding: &mut HashSet<String>,
) -> Option<Vec<BitSlice>> {
    let d = Dynamic::from(bv);
    let n = d.num_children();
    if n < 2 {
        return None;
    }
    let mut acc = bv_slices(&d.nth_child(0)?.as_bv()?, defs, expanding)?;
    if word_level(&acc, bv.get_size()) {
        return None;
    }
    for i in 1..n {
        let side = bv_slices(&d.nth_child(i)?.as_bv()?, defs, expanding)?;
        if word_level(&side, bv.get_size()) {
            return None;
        }
        acc = merge_disjoint_or(acc, side)?;
        if acc.len() > 32 {
            return None;
        }
    }
    Some(acc)
}

fn word_level(slices: &[BitSlice], w: u32) -> bool {
    slices.len() == 1 && slices[0].is_full() && slices[0].root.get_size() == w
}

struct Occupied {
    hi: u32,
    lo: u32,
    slice: BitSlice,
}

fn occupied(slices: &[BitSlice]) -> Vec<Occupied> {
    let w = slices_width(slices);
    if w == 0 {
        return Vec::new();
    }
    let mut bit_hi = w - 1;
    let mut out = Vec::new();
    for s in slices {
        let lo = bit_hi + 1 - s.width();
        if !s.is_zero() {
            out.push(Occupied {
                hi: bit_hi,
                lo,
                slice: BitSlice {
                    root: s.root.clone(),
                    hi: s.hi,
                    lo: s.lo,
                },
            });
        }
        bit_hi = lo.saturating_sub(1);
    }
    out
}

fn merge_disjoint_or(a: Vec<BitSlice>, b: Vec<BitSlice>) -> Option<Vec<BitSlice>> {
    let w = slices_width(&a);
    if w == 0 || slices_width(&b) != w {
        return None;
    }
    let mut occ = occupied(&a);
    occ.extend(occupied(&b));
    occ.sort_by_key(|o| std::cmp::Reverse(o.hi));
    for pair in occ.windows(2) {
        if pair[1].hi >= pair[0].lo {
            return None;
        }
    }
    let mut cursor = w;
    let mut out = Vec::new();
    for o in occ {
        if o.hi + 1 < cursor {
            out.push(zero_slice(cursor - o.hi - 1));
        }
        out.push(o.slice);
        cursor = o.lo;
    }
    if cursor > 0 {
        out.push(zero_slice(cursor));
    }
    Some(merge_adjacent_slices(out))
}

#[cfg(test)]
mod tests {
    use crate::rewrite::eval::eval_env;
    use crate::rewrite::{alu, pack};
    use crate::state::LoadDef;
    use z3::ast::{Ast, BV};

    fn concat_le(bytes: &[BV]) -> BV {
        let mut acc = bytes[0].clone();
        for b in &bytes[1..] {
            acc = b.concat(&acc);
        }
        acc
    }

    #[test]
    fn peels_shift_or_bytes_to_concat() {
        let lo = BV::new_const("n_acc2_data_0", 8);
        let hi = BV::new_const("n_acc2_data_1", 8);
        let expr = lo
            .zero_ext(56)
            .bvor(&hi.zero_ext(56).bvshl(&BV::from_u64(8, 64)));
        let got = pack(&expr, &[]);
        assert_eq!(got.decl().kind(), z3::DeclKind::Concat);
        assert!(!got.to_string().contains("bvor"), "got {got}");
        assert_eq!(eval_env(&got, &[(&lo, 0x11), (&hi, 0x22)]), Some(0x2211));
    }

    #[test]
    fn peels_reconvergent_or_is_linear() {
        let b = BV::new_const("n_acc0_data_0", 8);
        let mut acc = b.zero_ext(56);
        for _ in 0..20 {
            acc = acc.bvor(&acc);
        }
        let t = std::time::Instant::now();
        let _ = alu(&acc, &[]);
        assert!(
            t.elapsed().as_millis() < 200,
            "bv_slices re-walked a shared DAG"
        );
    }

    #[test]
    fn overlapping_or_stays_or() {
        let a = BV::new_const("w_1", 64);
        let b = BV::new_const("w_2", 64);
        let expr = a.bvor(&b);
        let got = alu(&expr, &[]);
        assert_eq!(got.decl().kind(), z3::DeclKind::Bor);
    }

    #[test]
    fn peels_w197_style_unaligned_qword_then_add() {
        let bytes: Vec<BV> = (197..212)
            .map(|i| BV::new_const(format!("n_acc2_data_431{i}"), 8))
            .collect();
        // bytes[0] = 431197 … bytes[14] = 431211
        let w197 = concat_le(&bytes[0..2]).zero_ext(48);
        let n199 = bytes[2].zero_ext(56).bvshl(&BV::from_u64(0x10, 64));
        let w200 = concat_le(&bytes[3..7]).zero_ext(32);
        let w204 = concat_le(&bytes[7..15]);
        let packed = w200
            .bvmul(&BV::from_u64(0x1000000, 64))
            .bvor(&w197)
            .bvor(&n199)
            .bvor(&w204.bvmul(&BV::from_u64(0x100000000000000, 64)));
        let other = BV::new_const("w_acc2_data_463464", 64);
        let defs = [
            LoadDef {
                name: "w_acc2_data_431197".into(),
                expr: w197.clone(),
            },
            LoadDef {
                name: "w_acc2_data_431200".into(),
                expr: w200.clone(),
            },
            LoadDef {
                name: "w_acc2_data_431204".into(),
                expr: w204.clone(),
            },
            LoadDef {
                name: "w_acc2_data_463464".into(),
                expr: other.clone(),
            },
        ];
        let named_pack = BV::new_const("w_acc2_data_431200", 64)
            .bvmul(&BV::from_u64(0x1000000, 64))
            .bvor(&BV::new_const("w_acc2_data_431197", 64))
            .bvor(&bytes[2].zero_ext(56).bvshl(&BV::from_u64(0x10, 64)))
            .bvor(
                &BV::new_const("w_acc2_data_431204", 64)
                    .bvmul(&BV::from_u64(0x100000000000000, 64)),
            );
        let got = pack(&named_pack.bvadd(&other), &defs);
        assert_eq!(got.decl().kind(), z3::DeclKind::Badd);
        let lhs = got.nth_child(0).unwrap().as_bv().unwrap();
        assert_eq!(lhs.decl().kind(), z3::DeclKind::Concat, "pack {lhs}");
        assert!(!lhs.to_string().contains("bvor"), "pack {lhs}");
        let rhs = got.nth_child(1).unwrap().as_bv().unwrap();
        assert_eq!(rhs.decl().name(), "w_acc2_data_463464");
        let binds: Vec<_> = bytes
            .iter()
            .enumerate()
            .map(|(i, b)| (b, 0xA0 + i as u64))
            .collect();
        let bind_refs: Vec<_> = binds.iter().map(|(b, v)| (*b, *v)).collect();
        let inline = pack(&packed.bvadd(&other), &defs);
        assert_eq!(
            eval_env(&inline, &bind_refs),
            eval_env(&packed.bvadd(&other), &bind_refs)
        );
    }
}
