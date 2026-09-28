use z3::ast::BV;

use super::{merge_origins, SysvarOrigin, WORD_BITS};

/// A bitvector plus whether it derives from text/input/UIF symbols.
#[derive(Clone)]
pub struct SymVal {
    pub bv: BV,
    /// True if this value depends on `t_*`, `n_*`, a UIF producer, or a sysvar.
    pub environmental: bool,
    /// Sysvar producers this value was computed from (empty if none).
    pub origins: Vec<SysvarOrigin>,
}

impl SymVal {
    pub fn env(bv: BV) -> Self {
        Self {
            bv,
            environmental: true,
            origins: Vec::new(),
        }
    }

    pub fn concrete(bv: BV) -> Self {
        Self {
            bv,
            environmental: false,
            origins: Vec::new(),
        }
    }

    pub fn from_u64(v: u64) -> Self {
        Self::concrete(BV::from_u64(v, WORD_BITS))
    }

    pub fn named(name: &str) -> Self {
        Self::env(BV::new_const(name, WORD_BITS))
    }

    /// Combine tracking/provenance from optional operands onto `bv`.
    pub fn combine(a: Option<&Self>, b: Option<&Self>, bv: BV) -> Self {
        let mut origins = Vec::new();
        let mut environmental = false;
        if let Some(s) = a {
            environmental |= s.environmental;
            merge_origins(&mut origins, &s.origins);
        }
        if let Some(s) = b {
            environmental |= s.environmental;
            merge_origins(&mut origins, &s.origins);
        }
        Self {
            bv,
            environmental,
            origins,
        }
    }

    pub fn with_bv(&self, bv: BV) -> Self {
        Self {
            bv,
            environmental: self.environmental,
            origins: self.origins.clone(),
        }
    }
}
