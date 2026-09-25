//! Identify aligned PDA-syscall words (`find`/`create` qwords and the bump byte).

use z3::ast::{Ast, BV, Dynamic};

use crate::astwalk::ast_id;
use crate::grammar::Syscall;

use super::slices::peel_bv_window;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PdaSyscall {
    Find,
    Create,
}

/// If `expr` is an aligned qword (or bump byte) of a PDA UIF, return
/// `(syscall, apply-ast-id, field)` where field is `"0"`…`"3"` or `"bump"`.
pub(crate) fn match_pda_word(expr: &BV) -> Option<(PdaSyscall, usize, &'static str)> {
    let (src, lo, hi) = peel_bv_window(&Dynamic::from(expr))?;
    let name = src.decl().name();
    let kind = match Syscall::parse_uif(&name) {
        Some(Syscall::TryFindProgramAddress) => PdaSyscall::Find,
        Some(Syscall::CreateProgramAddress) => PdaSyscall::Create,
        _ => return None,
    };
    let field = match (lo, hi) {
        (0, 63) => "0",
        (64, 127) => "1",
        (128, 191) => "2",
        (192, 255) => "3",
        (256, 263) => "bump",
        _ => return None,
    };
    Some((kind, ast_id(&src), field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use z3::ast::BV;
    use z3::{FuncDecl, Sort};

    fn find_pda() -> BV {
        FuncDecl::new(
            "uif_sol_try_find_program_address",
            &[&Sort::bitvector(64)],
            &Sort::bitvector(264),
        )
        .apply(&[&BV::from_u64(0, 64)])
        .as_bv()
        .unwrap()
    }

    #[test]
    fn names_aligned_qwords_and_bump() {
        let pda = find_pda();
        assert_eq!(match_pda_word(&pda.extract(63, 0)).map(|t| t.2), Some("0"));
        assert_eq!(
            match_pda_word(&pda.extract(127, 64)).map(|t| t.2),
            Some("1")
        );
        assert_eq!(
            match_pda_word(&pda.extract(255, 192)).map(|t| t.2),
            Some("3")
        );
        let bump = pda.extract(263, 256).zero_ext(56);
        assert_eq!(match_pda_word(&bump).map(|t| t.2), Some("bump"));
        assert_eq!(
            match_pda_word(&pda.extract(63, 0)).map(|t| t.0),
            Some(PdaSyscall::Find)
        );
    }

    #[test]
    fn rejects_unaligned_window() {
        let pda = find_pda();
        let shuffled = pda.extract(55, 0).concat(&pda.extract(63, 56));
        assert!(match_pda_word(&shuffled).is_none());
    }

    #[test]
    fn create_has_no_bump_field() {
        let pda = FuncDecl::new(
            "uif_sol_create_program_address",
            &[&Sort::bitvector(64)],
            &Sort::bitvector(256),
        )
        .apply(&[&BV::from_u64(0, 64)])
        .as_bv()
        .unwrap();
        assert_eq!(
            match_pda_word(&pda.extract(191, 128)).map(|t| (t.0, t.2)),
            Some((PdaSyscall::Create, "2"))
        );
        assert!(match_pda_word(&pda.extract(255, 192).extract(7, 0).zero_ext(56)).is_none());
    }
}
