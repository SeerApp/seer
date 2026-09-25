//! A bit-window into a Z3 node. Each peel arm shrinks `cur` or stops.

use z3::ast::Dynamic;

pub(crate) struct Window {
    pub cur: Dynamic,
    pub lo: u32,
    pub hi: u32,
}

pub(crate) enum Step {
    /// Peeled one layer; keep walking.
    Next,
    /// `cur` is final (e.g. a padding-zero constant).
    Done,
    /// This node is not a wrapper we handle.
    Stop,
    /// Malformed window; abort peel.
    Fail,
}

impl Window {
    pub(crate) fn of(d: &Dynamic) -> Option<Self> {
        Some(Self {
            cur: d.clone(),
            lo: 0,
            hi: d.as_bv()?.get_size().checked_sub(1)?,
        })
    }

    pub(crate) fn width(&self) -> u32 {
        self.hi - self.lo + 1
    }
}
