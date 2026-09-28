//! Formal-logic and SMT pretty-printers for path conditions and load defs.

use z3::ast::{Ast, Dynamic, BV};
use z3::DeclKind;

use super::options::DisplayOptions;

/// Binding strength for parenthesizing formal-logic output (higher = tighter).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    Or = 1,
    And = 2,
    Cmp = 3,
    Add = 4,
    Mul = 5,
    Prefix = 6,
    Atom = 7,
}

fn child(ast: &Dynamic, i: usize) -> Option<Dynamic> {
    ast.nth_child(i)
}

fn children(ast: &Dynamic) -> Vec<Dynamic> {
    (0..ast.num_children())
        .filter_map(|i| child(ast, i))
        .collect()
}

/// Concat is binary in the AST (`Z3_mk_concat`). Collect non-concat pieces
/// high-to-low so printers can emit one n-ary call without `simplify`.
fn concat_pieces(ast: &Dynamic) -> Vec<Dynamic> {
    let mut out = Vec::new();
    collect_concat_pieces(ast, &mut out);
    out
}

fn collect_concat_pieces(ast: &Dynamic, out: &mut Vec<Dynamic>) {
    if ast.decl().kind() != DeclKind::Concat {
        out.push(ast.clone());
        return;
    }
    for i in 0..ast.num_children() {
        if let Some(ch) = child(ast, i) {
            collect_concat_pieces(&ch, out);
        }
    }
}

/// Indexed decl params (`extract` hi/lo, `zero_extend` n, …) via the C API.
/// Do not use `ast.to_string()` — that pretty-prints the whole subtree.
fn indexed_params(ast: &Dynamic) -> Option<(String, Vec<u32>)> {
    let ctx = ast.get_ctx().get_z3_context();
    let nums = unsafe {
        let app = z3_sys::Z3_to_app(ctx, ast.get_z3_ast())?;
        let decl = z3_sys::Z3_get_app_decl(ctx, app)?;
        let n = z3_sys::Z3_get_decl_num_parameters(ctx, decl);
        (0..n)
            .map(|i| z3_sys::Z3_get_decl_int_parameter(ctx, decl, i) as u32)
            .collect::<Vec<_>>()
    };
    if nums.is_empty() {
        return None;
    }
    Some((ast.decl().name(), nums))
}

fn fmt_bv_numeral(bv: &BV) -> String {
    let width = bv.get_size();
    if let Some(v) = bv.as_u64() {
        return fmt_u64_const(v, width);
    }
    // Fall back: rewrite SMT `#x…` / `#b…` into `0x…` / `0b…`.
    rewrite_smt_literal(&bv.to_string())
}

pub(crate) fn fmt_u64_const(v: u64, width: u32) -> String {
    // Tiny values read best in decimal.
    if v < 16 && width <= 8 {
        return v.to_string();
    }
    // Prefer hex always for larger values — bit-width is conveyed by slices /
    // context, and hex avoids IDE colour-decorator clashes with `#…`.
    let nibbles = ((width + 3) / 4).max(1) as usize;
    let hex = format!("{v:0width$x}", width = nibbles);
    let trimmed = hex.trim_start_matches('0');
    if trimmed.is_empty() {
        "0x0".into()
    } else {
        format!("0x{trimmed}")
    }
}

fn rewrite_smt_literal(s: &str) -> String {
    if let Some(hex) = s.strip_prefix("#x") {
        let trimmed = hex.trim_start_matches('0');
        if trimmed.is_empty() {
            return "0x0".into();
        }
        return format!("0x{trimmed}");
    }
    if let Some(bin) = s.strip_prefix("#b") {
        if bin.len() <= 4 {
            if let Ok(v) = u64::from_str_radix(bin, 2) {
                return v.to_string();
            }
        }
        if let Ok(v) = u128::from_str_radix(bin, 2) {
            let nibbles = ((bin.len() + 3) / 4).max(1);
            let hex = format!("{v:0width$x}", width = nibbles);
            let trimmed = hex.trim_start_matches('0');
            return if trimmed.is_empty() {
                "0x0".into()
            } else {
                format!("0x{trimmed}")
            };
        }
        let trimmed = bin.trim_start_matches('0');
        if trimmed.is_empty() {
            return "0b0".into();
        }
        return format!("0b{trimmed}");
    }
    s.to_string()
}

fn needs_parens(child_prec: Prec, parent_prec: Prec, assoc_left: bool) -> bool {
    if child_prec > parent_prec {
        return false;
    }
    if child_prec < parent_prec {
        return true;
    }
    // Equal precedence: parenthesize right child of left-assoc ops.
    !assoc_left
}

fn fmt_bin(
    op: &str,
    left: &Dynamic,
    right: &Dynamic,
    prec: Prec,
    parent: Prec,
    budget: &mut usize,
    depth: usize,
) -> String {
    let l = fmt_logic_prec(left, prec, true, budget, depth);
    let r = fmt_logic_prec(right, prec, false, budget, depth);
    let s = format!("{l} {op} {r}");
    if needs_parens(prec, parent, true) {
        format!("({s})")
    } else {
        s
    }
}

fn fmt_nary(
    op: &str,
    args: &[Dynamic],
    prec: Prec,
    parent: Prec,
    budget: &mut usize,
    depth: usize,
) -> String {
    if args.is_empty() {
        return op.to_string();
    }
    let parts: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(i, a)| fmt_logic_prec(a, prec, i == 0, budget, depth))
        .collect();
    let s = parts.join(&format!(" {op} "));
    if needs_parens(prec, parent, true) {
        format!("({s})")
    } else {
        s
    }
}

fn fmt_prefix(op: &str, arg: &Dynamic, parent: Prec, budget: &mut usize, depth: usize) -> String {
    let inner = fmt_logic_prec(arg, Prec::Prefix, true, budget, depth);
    let s = format!("{op}{inner}");
    if needs_parens(Prec::Prefix, parent, true) {
        format!("({s})")
    } else {
        s
    }
}

fn fmt_call(name: &str, args: &[Dynamic], budget: &mut usize, depth: usize) -> String {
    let parts: Vec<String> = args
        .iter()
        .map(|a| fmt_logic_prec(a, Prec::Or, true, budget, depth))
        .collect();
    format!("{name}({})", parts.join(", "))
}

fn fmt_slice(arg: &Dynamic, hi: u32, lo: u32, budget: &mut usize, depth: usize) -> String {
    // Atoms and nested extracts can take `[hi:lo]` directly; other compounds need parens.
    let compound = arg.num_children() > 0 && !matches!(arg.decl().kind(), DeclKind::Extract);
    if compound {
        let inner = fmt_logic_prec(arg, Prec::Or, true, budget, depth);
        format!("({inner})[{hi}:{lo}]")
    } else {
        let base = fmt_logic_prec(arg, Prec::Atom, true, budget, depth);
        format!("{base}[{hi}:{lo}]")
    }
}

fn fmt_logic_prec(
    ast: &Dynamic,
    parent: Prec,
    _assoc_left: bool,
    budget: &mut usize,
    depth: usize,
) -> String {
    if *budget == 0 {
        return "…".into();
    }
    *budget -= 1;

    if depth == 0 {
        return "…".into();
    }
    let depth = depth - 1;

    // Numerals / uninterpreted constants (0 children).
    if ast.num_children() == 0 {
        if let Some(b) = ast.as_bool() {
            if let Some(v) = b.as_bool() {
                return if v { "true" } else { "false" }.into();
            }
        }
        if let Some(bv) = ast.as_bv() {
            if matches!(bv.decl().kind(), DeclKind::Bnum) {
                return fmt_bv_numeral(&bv);
            }
            return bv.decl().name();
        }
        return ast.decl().name();
    }

    let kind = ast.decl().kind();
    let args = children(ast);

    match kind {
        DeclKind::And => fmt_nary("∧", &args, Prec::And, parent, budget, depth),
        DeclKind::Or => fmt_nary("∨", &args, Prec::Or, parent, budget, depth),
        DeclKind::Not => {
            if let Some(a) = args.first() {
                fmt_prefix("¬", a, parent, budget, depth)
            } else {
                "¬?".into()
            }
        }
        DeclKind::Eq => {
            if args.len() == 2 {
                fmt_bin("=", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call("=", &args, budget, depth)
            }
        }
        DeclKind::Uleq => {
            if args.len() == 2 {
                fmt_bin("≤", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call("≤", &args, budget, depth)
            }
        }
        DeclKind::Ult => {
            if args.len() == 2 {
                fmt_bin("<", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call("<", &args, budget, depth)
            }
        }
        DeclKind::Ugeq => {
            if args.len() == 2 {
                fmt_bin("≥", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call("≥", &args, budget, depth)
            }
        }
        DeclKind::Ugt => {
            if args.len() == 2 {
                fmt_bin(">", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call(">", &args, budget, depth)
            }
        }
        DeclKind::Sleq => {
            if args.len() == 2 {
                fmt_bin("≤ₛ", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call("≤ₛ", &args, budget, depth)
            }
        }
        DeclKind::Slt => {
            if args.len() == 2 {
                fmt_bin("<ₛ", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call("<ₛ", &args, budget, depth)
            }
        }
        DeclKind::Sgeq => {
            if args.len() == 2 {
                fmt_bin("≥ₛ", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call("≥ₛ", &args, budget, depth)
            }
        }
        DeclKind::Sgt => {
            if args.len() == 2 {
                fmt_bin(">ₛ", &args[0], &args[1], Prec::Cmp, parent, budget, depth)
            } else {
                fmt_call(">ₛ", &args, budget, depth)
            }
        }
        DeclKind::Badd => {
            if args.len() == 2 {
                fmt_bin("+", &args[0], &args[1], Prec::Add, parent, budget, depth)
            } else {
                fmt_nary("+", &args, Prec::Add, parent, budget, depth)
            }
        }
        DeclKind::Bsub => {
            if args.len() == 2 {
                fmt_bin("−", &args[0], &args[1], Prec::Add, parent, budget, depth)
            } else {
                fmt_call("−", &args, budget, depth)
            }
        }
        DeclKind::Bmul => {
            if args.len() == 2 {
                fmt_bin("×", &args[0], &args[1], Prec::Mul, parent, budget, depth)
            } else {
                fmt_nary("×", &args, Prec::Mul, parent, budget, depth)
            }
        }
        DeclKind::Budiv => {
            if args.len() == 2 {
                fmt_bin("÷", &args[0], &args[1], Prec::Mul, parent, budget, depth)
            } else {
                fmt_call("÷", &args, budget, depth)
            }
        }
        DeclKind::Burem => {
            if args.len() == 2 {
                fmt_bin("mod", &args[0], &args[1], Prec::Mul, parent, budget, depth)
            } else {
                fmt_call("mod", &args, budget, depth)
            }
        }
        DeclKind::Band => {
            if args.len() == 2 {
                fmt_bin("&", &args[0], &args[1], Prec::Mul, parent, budget, depth)
            } else {
                fmt_nary("&", &args, Prec::Mul, parent, budget, depth)
            }
        }
        DeclKind::Bor => {
            if args.len() == 2 {
                fmt_bin("|", &args[0], &args[1], Prec::Add, parent, budget, depth)
            } else {
                fmt_nary("|", &args, Prec::Add, parent, budget, depth)
            }
        }
        DeclKind::Bxor => {
            if args.len() == 2 {
                fmt_bin("⊕", &args[0], &args[1], Prec::Add, parent, budget, depth)
            } else {
                fmt_nary("⊕", &args, Prec::Add, parent, budget, depth)
            }
        }
        DeclKind::Bshl => {
            if args.len() == 2 {
                fmt_bin("≪", &args[0], &args[1], Prec::Mul, parent, budget, depth)
            } else {
                fmt_call("≪", &args, budget, depth)
            }
        }
        DeclKind::Blshr => {
            if args.len() == 2 {
                fmt_bin("≫", &args[0], &args[1], Prec::Mul, parent, budget, depth)
            } else {
                fmt_call("≫", &args, budget, depth)
            }
        }
        DeclKind::Bashr => {
            if args.len() == 2 {
                fmt_bin("≫ₛ", &args[0], &args[1], Prec::Mul, parent, budget, depth)
            } else {
                fmt_call("≫ₛ", &args, budget, depth)
            }
        }
        DeclKind::Bneg => {
            if let Some(a) = args.first() {
                fmt_prefix("−", a, parent, budget, depth)
            } else {
                "−?".into()
            }
        }
        DeclKind::Bnot => {
            if let Some(a) = args.first() {
                fmt_prefix("~", a, parent, budget, depth)
            } else {
                "~?".into()
            }
        }
        DeclKind::Concat => fmt_call("concat", &concat_pieces(ast), budget, depth),
        DeclKind::Extract => {
            if let (Some(a), Some((_, params))) = (args.first(), indexed_params(ast)) {
                if params.len() >= 2 {
                    return fmt_slice(a, params[0], params[1], budget, depth);
                }
            }
            fmt_call("extract", &args, budget, depth)
        }
        DeclKind::ZeroExt => {
            if let (Some(a), Some((_, params))) = (args.first(), indexed_params(ast)) {
                if let Some(n) = params.first() {
                    return fmt_call(&format!("zext_{n}"), std::slice::from_ref(a), budget, depth);
                }
            }
            fmt_call("zext", &args, budget, depth)
        }
        DeclKind::SignExt => {
            if let (Some(a), Some((_, params))) = (args.first(), indexed_params(ast)) {
                if let Some(n) = params.first() {
                    return fmt_call(&format!("sext_{n}"), std::slice::from_ref(a), budget, depth);
                }
            }
            fmt_call("sext", &args, budget, depth)
        }
        DeclKind::Uninterpreted => {
            let name = ast.decl().name();
            if args.is_empty() {
                name
            } else {
                fmt_call(&name, &args, budget, depth)
            }
        }
        _ => {
            let name = ast.decl().name();
            if args.is_empty() {
                rewrite_smt_literal(&name)
            } else {
                fmt_call(&name, &args, budget, depth)
            }
        }
    }
}

fn fmt_logic(ast: &Dynamic, full: bool) -> String {
    let (mut budget, depth) = if full { (2048, 24) } else { (64, 6) };
    fmt_logic_prec(ast, Prec::Or, true, &mut budget, depth)
}

/// Pretty-print an AST without ever stringifying a huge DAG at once (SMT mode).
fn fmt_smt_limited(ast: &Dynamic, depth: usize, budget: &mut usize) -> String {
    if *budget == 0 {
        return "…".into();
    }
    *budget -= 1;

    let n = ast.num_children();
    if depth == 0 {
        return "…".into();
    }
    if n == 0 {
        if let Some(bv) = ast.as_bv() {
            if matches!(bv.decl().kind(), DeclKind::Bnum) {
                return fmt_bv_numeral(&bv);
            }
        }
        if let Some(b) = ast.as_bool() {
            if let Some(v) = b.as_bool() {
                return if v { "true" } else { "false" }.into();
            }
        }
        return ast.decl().name();
    }

    let kids = if ast.decl().kind() == DeclKind::Concat {
        concat_pieces(ast)
    } else {
        (0..n).filter_map(|i| ast.nth_child(i)).collect()
    };
    let name = ast.decl().name();
    let mut parts = Vec::with_capacity(kids.len());
    for ch in &kids {
        parts.push(fmt_smt_limited(ch, depth - 1, budget));
        if *budget == 0 {
            parts.push("…".into());
            break;
        }
    }
    format!("({name} {})", parts.join(" "))
}

pub(crate) fn fmt_ast(ast: &Dynamic, opts: DisplayOptions) -> String {
    // Never call Z3_ast_to_string on a compound node: shared DAGs look
    // small by unique-node count but explode when pretty-printed as a tree.
    if opts.smt {
        let mut budget = if opts.full { 256 } else { 48 };
        let depth = if opts.full { 12 } else { 4 };
        fmt_smt_limited(ast, depth, &mut budget)
    } else {
        fmt_logic(ast, opts.full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use z3::ast::BV;

    #[test]
    fn logic_printer_uses_formal_ops_and_avoids_hash_literals() {
        use z3::ast::Bool;
        let w0 = BV::new_const("w_0", 64);
        let hi = w0.extract(63, 2);
        let lo = w0.extract(1, 0);
        let formula = Bool::and(&[&hi.eq(&BV::from_u64(0, 62)), &lo.bvule(&BV::from_u64(2, 2))]);
        let s = fmt_logic(&Dynamic::from(&formula), true);
        assert!(s.contains('∧'), "got {s}");
        assert!(s.contains('≤'), "got {s}");
        assert!(s.contains("w_0[63:2]"), "got {s}");
        assert!(s.contains("w_0[1:0]"), "got {s}");
        assert!(!s.contains('#'), "got {s}");
    }

    fn packed_bytes(names: &[&str]) -> BV {
        names
            .iter()
            .map(|n| BV::new_const(*n, 8))
            .reduce(|lo, hi| hi.concat(&lo))
            .unwrap()
    }

    #[test]
    fn logic_printer_flattens_nested_concat() {
        let packed = packed_bytes(&["n_00", "n_01", "n_02", "n_03"]);
        let s = fmt_logic(&Dynamic::from(&packed), true);
        assert_eq!(s, "concat(n_03, n_02, n_01, n_00)");
    }

    #[test]
    fn smt_printer_flattens_nested_concat() {
        let packed = packed_bytes(&["n_00", "n_01", "n_02", "n_03"]);
        let s = fmt_ast(
            &Dynamic::from(&packed),
            DisplayOptions {
                full: true,
                smt: true,
                ..DisplayOptions::default()
            },
        );
        assert_eq!(s, "(concat n_03 n_02 n_01 n_00)");
    }

    #[test]
    fn rewrite_smt_literal_avoids_hash_prefix() {
        assert_eq!(rewrite_smt_literal("#x00000030"), "0x30");
        assert_eq!(rewrite_smt_literal("#b10"), "2");
        assert_eq!(rewrite_smt_literal("#b0"), "0");
        assert_eq!(
            rewrite_smt_literal(
                "#b111111111111111111111111111111111111111111111111111111111111000"
            ),
            "0x7ffffffffffffff8"
        );
    }
}
