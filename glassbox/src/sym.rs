//! Tautology detection for path conditions.

use z3::ast::Bool;

use crate::astwalk::for_each_app;
use crate::grammar::is_env_ident;

/// Pretty-printing was the hang; SAT on a huge leftover formula must not
/// block the interpreter either.
const TAUTOLOGY_TIMEOUT_MS: u32 = 50;

fn mentions_env_symbol(f: &Bool) -> bool {
    for_each_app(f, |_, name| is_env_ident(name))
}

/// Capture membership: bool constants and formulas with no env symbols.
/// SAT does not belong here — a 50ms miss must not drop the row.
pub(crate) fn is_syntactic_tautology(formula: &Bool) -> bool {
    formula.as_bool().is_some() || !mentions_env_symbol(formula)
}

/// Hide-side: syntactic, or Z3 proved vacuous inside the think budget.
/// `formula` is already the output of [`crate::rewrite::branch`] — do not
/// `simplify` again.
pub fn is_tautology(formula: &Bool) -> bool {
    if is_syntactic_tautology(formula) {
        return true;
    }

    // φ valid  ⇔  ¬φ unsat
    if crate::sat::unsat(&formula.not(), TAUTOLOGY_TIMEOUT_MS) {
        return true;
    }

    // φ unsat  ⇔  always false
    crate::sat::unsat(formula, TAUTOLOGY_TIMEOUT_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use z3::ast::BV;

    #[test]
    fn tautology_detects_true_false_without_printing() {
        assert!(is_tautology(&Bool::from_bool(true)));
        assert!(is_tautology(&Bool::from_bool(false)));
        let x = BV::from_u64(1, 64);
        assert!(is_tautology(&x.eq(&x)));
    }

    #[test]
    fn tautology_eq_same_env_symbol() {
        let w = BV::new_const("w_acc0_data_len", 64);
        assert!(is_tautology(&w.eq(&w)));
        assert!(is_tautology(&w.eq(&w).not()));
    }

    #[test]
    fn tautology_unsigned_ge_zero() {
        let w = BV::new_const("w_acc0_data_len", 64);
        assert!(is_tautology(&w.bvuge(&BV::from_u64(0, 64))));
        assert!(is_tautology(&w.bvuge(&BV::from_u64(0, 64)).not()));
        assert!(is_tautology(&w.bvult(&BV::from_u64(0, 64))));
    }

    #[test]
    fn tautology_keeps_env_constraints() {
        let w = BV::new_const("w_acc0_data_len", 64);
        let f = w.eq(&BV::from_u64(0, 64)).not();
        assert!(!is_tautology(&f));
        assert!(mentions_env_symbol(&f));
        assert!(!is_syntactic_tautology(&w.eq(&w)));
    }
}
