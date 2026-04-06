use std::collections::HashSet;

use codama_nodes::{
    DefinedTypeNode, EnumVariantTypeNode, NestedTypeNodeTrait, ProgramNode, TypeNode,
};

use crate::idl::{IdlIssue, IdlIssues, IdlLocation};

fn push_loc_path(loc: &IdlLocation, segment: impl Into<String>) -> IdlLocation {
    let mut l = loc.clone();
    match &mut l {
        IdlLocation::SchemaInstruction { path, .. }
        | IdlLocation::SchemaAccount { path, .. }
        | IdlLocation::SchemaDefinedType { path, .. } => path.push(segment.into()),
        _ => {}
    }
    l
}

/// Single pass after JSON parse: record schema issues and names of definitions to skip permanently.
pub fn analyze_codama_program(
    program: &ProgramNode,
    issues: &mut IdlIssues,
    skip_instructions: &mut HashSet<String>,
    skip_accounts: &mut HashSet<String>,
) {
    for ix in &program.instructions {
        let ix_name = ix.name.to_string();
        if ix.discriminators.len() != 1 {
            issues.note_instruction_non_unary_discriminator(
                ix_name.clone(),
                ix.discriminators.len(),
            );
            skip_instructions.insert(ix_name);
            continue;
        }

        let n = ix.arguments.len();
        let mut ok = true;
        for (i, arg) in ix.arguments.iter().enumerate() {
            let last = i + 1 == n;
            let loc = IdlLocation::SchemaInstruction {
                instruction: ix_name.clone(),
                path: vec![arg.name.to_string()],
            };
            if !validate_type_tree(
                &arg.r#type,
                &program.defined_types,
                last,
                loc,
                issues,
            ) {
                ok = false;
                break;
            }
        }
        if !ok {
            skip_instructions.insert(ix_name);
        }
    }

    for acc in &program.accounts {
        let acc_name = acc.name.to_string();
        if acc.discriminators.len() > 1 {
            issues.note_account_non_unary_discriminator(
                acc_name.clone(),
                acc.discriminators.len(),
            );
            skip_accounts.insert(acc_name);
            continue;
        }

        let should_validate =
            program.accounts.len() == 1 || acc.discriminators.len() == 1;
        if !should_validate {
            continue;
        }

        let inner = acc.data.get_nested_type_node();
        let n = inner.fields.len();
        let mut ok = true;
        for (i, field) in inner.fields.iter().enumerate() {
            let last = i + 1 == n;
            let loc = IdlLocation::SchemaAccount {
                account: acc_name.clone(),
                path: vec![field.name.to_string()],
            };
            if !validate_type_tree(
                &field.r#type,
                &program.defined_types,
                last,
                loc,
                issues,
            ) {
                ok = false;
                break;
            }
        }
        if !ok {
            skip_accounts.insert(acc_name);
        }
    }
}

fn validate_type_tree(
    ty: &TypeNode,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
    loc: IdlLocation,
    issues: &mut IdlIssues,
) -> bool {
    match ty {
        TypeNode::Link(link) => {
            let Some(dt) = defined_types.iter().find(|d| d.name == link.name) else {
                issues.note(IdlIssue::MissingDefinedTypeLink {
                    link_name: link.name.to_string(),
                    at: loc,
                });
                return false;
            };
            let inner = IdlLocation::SchemaDefinedType {
                type_name: dt.name.to_string(),
                path: vec![],
            };
            validate_type_tree(&dt.r#type, defined_types, is_last, inner, issues)
        }
        TypeNode::Bytes(_) | TypeNode::String(_) if !is_last => {
            issues.note(IdlIssue::InvalidLayoutBytesOrStringWithoutLength { at: loc });
            false
        }
        TypeNode::RemainderOption(_) if !is_last => {
            issues.note(IdlIssue::InvalidLayoutRemainderOptionNotLast { at: loc });
            false
        }
        TypeNode::Sentinel(_) => {
            issues.note(IdlIssue::UnsupportedSentinelType { at: loc });
            false
        }
        TypeNode::ZeroableOption(_) => {
            issues.note(IdlIssue::UnsupportedZeroableOptionType { at: loc });
            false
        }
        TypeNode::Struct(s) => {
            let n = s.fields.len();
            for (i, field) in s.fields.iter().enumerate() {
                let last_sib = i + 1 == n;
                let floc = push_loc_path(&loc, field.name.to_string());
                if !validate_type_tree(
                    &field.r#type,
                    defined_types,
                    last_sib && is_last,
                    floc,
                    issues,
                ) {
                    return false;
                }
            }
            true
        }
        TypeNode::Tuple(t) => {
            let n = t.items.len();
            for (i, item) in t.items.iter().enumerate() {
                let last_sib = i + 1 == n;
                let iloc = push_loc_path(&loc, i.to_string());
                if !validate_type_tree(
                    item,
                    defined_types,
                    last_sib && is_last,
                    iloc,
                    issues,
                ) {
                    return false;
                }
            }
            true
        }
        TypeNode::Array(a) => validate_type_tree(
            &a.item,
            defined_types,
            is_last,
            loc,
            issues,
        ),
        TypeNode::Set(s) => validate_type_tree(
            &s.item,
            defined_types,
            is_last,
            loc,
            issues,
        ),
        TypeNode::Map(m) => {
            let kloc = push_loc_path(&loc, "key");
            if !validate_type_tree(
                &m.key,
                defined_types,
                is_last,
                kloc,
                issues,
            ) {
                return false;
            }
            let vloc = push_loc_path(&loc, "value");
            validate_type_tree(
                &m.value,
                defined_types,
                is_last,
                vloc,
                issues,
            )
        }
        TypeNode::Option(o) => validate_type_tree(
            &o.item,
            defined_types,
            is_last,
            loc,
            issues,
        ),
        TypeNode::Enum(e) => {
            for v in &e.variants {
                match v {
                    EnumVariantTypeNode::Empty(_) => {}
                    EnumVariantTypeNode::Struct(ev) => {
                        let variant_loc = push_loc_path(&loc, ev.name.to_string());
                        let st = ev.r#struct.get_nested_type_node();
                        let n = st.fields.len();
                        for (i, field) in st.fields.iter().enumerate() {
                            let last_sib = i + 1 == n;
                            let floc = push_loc_path(&variant_loc, field.name.to_string());
                            if !validate_type_tree(
                                &field.r#type,
                                defined_types,
                                last_sib && is_last,
                                floc,
                                issues,
                            ) {
                                return false;
                            }
                        }
                    }
                    EnumVariantTypeNode::Tuple(ev) => {
                        let variant_loc = push_loc_path(&loc, ev.name.to_string());
                        let tup = ev.tuple.get_nested_type_node();
                        let n = tup.items.len();
                        for (i, item) in tup.items.iter().enumerate() {
                            let last_sib = i + 1 == n;
                            let iloc = push_loc_path(&variant_loc, i.to_string());
                            if !validate_type_tree(
                                item,
                                defined_types,
                                last_sib && is_last,
                                iloc,
                                issues,
                            ) {
                                return false;
                            }
                        }
                    }
                }
            }
            true
        }
        TypeNode::HiddenPrefix(h) => {
            for (i, c) in h.prefix.iter().enumerate() {
                let ploc = push_loc_path(&loc, format!("prefix[{i}]"));
                if !validate_type_tree(
                    &c.r#type,
                    defined_types,
                    is_last,
                    ploc,
                    issues,
                ) {
                    return false;
                }
            }
            validate_type_tree(&h.r#type, defined_types, is_last, loc, issues)
        }
        TypeNode::HiddenSuffix(h) => {
            if !validate_type_tree(&h.r#type, defined_types, is_last, loc.clone(), issues) {
                return false;
            }
            for (i, c) in h.suffix.iter().enumerate() {
                let sloc = push_loc_path(&loc, format!("suffix[{i}]"));
                if !validate_type_tree(
                    &c.r#type,
                    defined_types,
                    is_last,
                    sloc,
                    issues,
                ) {
                    return false;
                }
            }
            true
        }
        TypeNode::FixedSize(f) => validate_type_tree(
            &f.r#type,
            defined_types,
            true,
            loc,
            issues,
        ),
        TypeNode::SizePrefix(s) => validate_type_tree(
            &s.r#type,
            defined_types,
            true,
            loc,
            issues,
        ),
        TypeNode::PostOffset(p) => validate_type_tree(
            &p.r#type,
            defined_types,
            is_last,
            loc,
            issues,
        ),
        TypeNode::PreOffset(p) => validate_type_tree(
            &p.r#type,
            defined_types,
            is_last,
            loc,
            issues,
        ),
        TypeNode::Amount(_)
        | TypeNode::Boolean(_)
        | TypeNode::DateTime(_)
        | TypeNode::SolAmount(_) => true,
        _ => true,
    }
}
