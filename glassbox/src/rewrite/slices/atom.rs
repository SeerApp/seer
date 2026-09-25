//! A named leaf. Expand a load-def when it is a pack (several slices), so
//! occupancy peel can stitch OR/concat. Do not expand an opaque full-word
//! def — that inlines `lo32(w_262)` back into `lo32(w_260 ÷ 10⁹)`.

use std::collections::{HashMap, HashSet};

use z3::ast::{Ast, BV};

use crate::astwalk::ast_id;
use crate::rewrite::ast::unique_nodes_at_most;

use super::bit::BitSlice;
use super::bv_slices;

pub(super) fn peel(
    bv: &BV,
    defs: &HashMap<String, BV>,
    expanding: &mut HashSet<String>,
) -> Option<Vec<BitSlice>> {
    if let Some(def) = defs.get(&bv.decl().name()) {
        if ast_id(def) != ast_id(bv) && expanding.insert(bv.decl().name()) {
            if unique_nodes_at_most(def, 64) >= 64 {
                expanding.remove(&bv.decl().name());
                return Some(vec![BitSlice::all(bv)]);
            }
            let out = bv_slices(def, defs, expanding);
            expanding.remove(&bv.decl().name());
            if let Some(ref slices) = out {
                let opaque_word = slices.len() == 1
                    && slices[0].is_full()
                    && ast_id(&slices[0].root) == ast_id(def);
                if !opaque_word {
                    return out;
                }
            }
        }
    }
    Some(vec![BitSlice::all(bv)])
}
