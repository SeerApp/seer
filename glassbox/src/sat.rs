//! Timed Z3 `check()`.
//!
//! One solver per thread, `reset()` between queries, so idiom proofs and
//! tautology filters do not pay `Solver::new()` on every step.

use std::cell::RefCell;

use z3::ast::Bool;
use z3::{Params, SatResult, Solver};

thread_local! {
    static SOLVER: RefCell<Solver> = RefCell::new(Solver::new());
}

pub(crate) fn check(formula: &Bool, timeout_ms: u32) -> SatResult {
    SOLVER.with(|s| {
        let s = s.borrow();
        s.reset();
        let mut params = Params::new();
        params.set_u32("timeout", timeout_ms);
        s.set_params(&params);
        s.assert(formula);
        s.check()
    })
}

pub(crate) fn unsat(formula: &Bool, timeout_ms: u32) -> bool {
    check(formula, timeout_ms) == SatResult::Unsat
}
