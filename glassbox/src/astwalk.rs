//! Shared Z3 DAG walk: node identity, env-ident prefixes, visit-once traversal.

use std::collections::HashSet;

use z3::ast::{Ast, Dynamic};

/// Pointer identity of a Z3 AST node (hash-consed; equal structure can share).
pub(crate) fn ast_id(bv: &dyn Ast) -> usize {
    bv.get_z3_ast().as_ptr() as usize
}

/// Walk a Z3 DAG (with sharing) and visit each app/numeral once.
/// `visit` returning true stops the walk; the function then returns true.
pub(crate) fn for_each_app(root: &dyn Ast, mut visit: impl FnMut(&Dynamic, &str) -> bool) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![Dynamic::from_ast(root)];
    while let Some(node) = stack.pop() {
        if !node.is_app() {
            continue;
        }
        if !seen.insert(ast_id(&node)) {
            continue;
        }
        let name = node.decl().name();
        if visit(&node, &name) {
            return true;
        }
        stack.extend(node.children());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::is_env_ident;
    use z3::ast::BV;

    #[test]
    fn env_idents_are_the_tracked_prefixes() {
        assert!(is_env_ident("n_acc0_dup"));
        assert!(is_env_ident("t_0000000001"));
        assert!(is_env_ident("w_num_accounts"));
        assert!(is_env_ident("uif_sol_get_rent_sysvar"));
        assert!(is_env_ident("pack_0"));
        assert!(!is_env_ident("bvadd"));
        assert!(!is_env_ident("x_acc0"));
    }

    #[test]
    fn ast_id_is_stable_for_the_same_node() {
        let x = BV::new_const("w_1", 64);
        assert_eq!(ast_id(&x), ast_id(&x));
        let y = BV::new_const("w_2", 64);
        assert_ne!(ast_id(&x), ast_id(&y));
    }

    #[test]
    fn for_each_app_visits_shared_nodes_once() {
        let x = BV::new_const("w_1", 64);
        let expr = x.bvadd(&x);
        let mut names = Vec::new();
        for_each_app(&expr, |_, name| {
            names.push(name.to_string());
            false
        });
        assert_eq!(names.iter().filter(|n| *n == "w_1").count(), 1);
        assert!(names.iter().any(|n| n == "bvadd"));
    }

    #[test]
    fn for_each_app_stops_when_visit_returns_true() {
        let x = BV::new_const("w_1", 64);
        let y = BV::new_const("w_2", 64);
        let expr = x.bvadd(&y);
        let mut n = 0;
        let stopped = for_each_app(&expr, |_, _| {
            n += 1;
            n >= 1
        });
        assert!(stopped);
        assert_eq!(n, 1);
    }
}
