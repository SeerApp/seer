use std::collections::HashSet;
use std::fmt;

use codama_nodes::{
    DefaultValueStrategy, DefinedTypeNode, EnumVariantTypeNode, NestedTypeNodeTrait, ProgramNode,
    TypeNode,
};

/// Human-readable location for Codama schema validation warnings.
#[derive(Clone, Debug)]
enum SchemaLoc {
    Instruction {
        instruction: String,
        path: Vec<String>,
    },
    Account {
        account: String,
        path: Vec<String>,
    },
    DefinedType {
        type_name: String,
        path: Vec<String>,
    },
}

impl fmt::Display for SchemaLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaLoc::Instruction { instruction, path } => {
                write!(f, "instruction:{instruction}")?;
                if !path.is_empty() {
                    write!(f, "/{}", path.join("/"))?;
                }
                Ok(())
            }
            SchemaLoc::Account { account, path } => {
                write!(f, "account:{account}")?;
                if !path.is_empty() {
                    write!(f, "/{}", path.join("/"))?;
                }
                Ok(())
            }
            SchemaLoc::DefinedType { type_name, path } => {
                write!(f, "defined_type:{type_name}")?;
                if !path.is_empty() {
                    write!(f, "/{}", path.join("/"))?;
                }
                Ok(())
            }
        }
    }
}

fn push_loc_path(loc: &SchemaLoc, segment: impl Into<String>) -> SchemaLoc {
    let seg = segment.into();
    match loc.clone() {
        SchemaLoc::Instruction { instruction, mut path } => {
            path.push(seg);
            SchemaLoc::Instruction { instruction, path }
        }
        SchemaLoc::Account { account, mut path } => {
            path.push(seg);
            SchemaLoc::Account { account, path }
        }
        SchemaLoc::DefinedType { type_name, mut path } => {
            path.push(seg);
            SchemaLoc::DefinedType { type_name, path }
        }
    }
}

/// Single pass after JSON parse: emit schema warnings and names of definitions to skip permanently.
pub fn analyze_codama_program(
    program: &ProgramNode,
    skip_instructions: &mut HashSet<String>,
    skip_accounts: &mut HashSet<String>,
) {
    let single_instruction_program = program.instructions.len() == 1;

    for ix in &program.instructions {
        let ix_name = ix.name.to_string();
        let discriminator_count = ix.discriminators.len();
        let has_valid_instruction_discriminator_shape =
            discriminator_count > 0 || (single_instruction_program && discriminator_count == 0);
        if !has_valid_instruction_discriminator_shape {
            crate::seer_warn!(
                "Codama schema: skipping instruction {:?}: invalid discriminator layout (count={}, single_instruction_program={})",
                ix_name,
                discriminator_count,
                single_instruction_program
            );
            skip_instructions.insert(ix_name);
            continue;
        }

        let runtime_args: Vec<_> = ix
            .arguments
            .iter()
            .filter(|arg| arg.default_value_strategy != Some(DefaultValueStrategy::Omitted))
            .collect();
        let n = runtime_args.len();
        let mut ok = true;
        for (i, arg) in runtime_args.iter().enumerate() {
            let last = i + 1 == n;
            let loc = SchemaLoc::Instruction {
                instruction: ix_name.clone(),
                path: vec![arg.name.to_string()],
            };
            if !validate_type_tree(&arg.r#type, &program.defined_types, last, loc) {
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
        let should_validate = program.accounts.len() == 1 || !acc.discriminators.is_empty();
        if !should_validate {
            continue;
        }

        let inner = acc.data.get_nested_type_node();
        let n = inner.fields.len();
        let mut ok = true;
        for (i, field) in inner.fields.iter().enumerate() {
            let last = i + 1 == n;
            let loc = SchemaLoc::Account {
                account: acc_name.clone(),
                path: vec![field.name.to_string()],
            };
            if !validate_type_tree(&field.r#type, &program.defined_types, last, loc) {
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
    loc: SchemaLoc,
) -> bool {
    match ty {
        TypeNode::Link(link) => {
            let Some(dt) = defined_types.iter().find(|d| d.name == link.name) else {
                crate::seer_warn!(
                    "Codama schema: missing defined type link {:?} at {}",
                    link.name.to_string(),
                    loc
                );
                return false;
            };
            let inner = SchemaLoc::DefinedType {
                type_name: dt.name.to_string(),
                path: vec![],
            };
            validate_type_tree(&dt.r#type, defined_types, is_last, inner)
        }
        TypeNode::Bytes(_) | TypeNode::String(_) if !is_last => {
            crate::seer_warn!(
                "Codama schema: bytes/string field must be last in layout at {}",
                loc
            );
            false
        }
        TypeNode::RemainderOption(_) if !is_last => {
            crate::seer_warn!(
                "Codama schema: remainderOption must be last field at {}",
                loc
            );
            false
        }
        TypeNode::Sentinel(s) => validate_type_tree(&s.r#type, defined_types, is_last, loc),
        TypeNode::ZeroableOption(z) => {
            validate_type_tree(&z.item, defined_types, is_last, loc)
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
                if !validate_type_tree(item, defined_types, last_sib && is_last, iloc) {
                    return false;
                }
            }
            true
        }
        TypeNode::Array(a) => validate_type_tree(&a.item, defined_types, is_last, loc),
        TypeNode::Set(s) => validate_type_tree(&s.item, defined_types, is_last, loc),
        TypeNode::Map(m) => {
            let kloc = push_loc_path(&loc, "key");
            if !validate_type_tree(&m.key, defined_types, is_last, kloc) {
                return false;
            }
            let vloc = push_loc_path(&loc, "value");
            validate_type_tree(&m.value, defined_types, is_last, vloc)
        }
        TypeNode::Option(o) => validate_type_tree(&o.item, defined_types, is_last, loc),
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
                if !validate_type_tree(&c.r#type, defined_types, is_last, ploc) {
                    return false;
                }
            }
            validate_type_tree(&h.r#type, defined_types, is_last, loc)
        }
        TypeNode::HiddenSuffix(h) => {
            if !validate_type_tree(&h.r#type, defined_types, is_last, loc.clone()) {
                return false;
            }
            for (i, c) in h.suffix.iter().enumerate() {
                let sloc = push_loc_path(&loc, format!("suffix[{i}]"));
                if !validate_type_tree(&c.r#type, defined_types, is_last, sloc) {
                    return false;
                }
            }
            true
        }
        TypeNode::FixedSize(f) => validate_type_tree(&f.r#type, defined_types, true, loc),
        TypeNode::SizePrefix(s) => validate_type_tree(&s.r#type, defined_types, true, loc),
        TypeNode::PostOffset(p) => validate_type_tree(&p.r#type, defined_types, is_last, loc),
        TypeNode::PreOffset(p) => validate_type_tree(&p.r#type, defined_types, is_last, loc),
        TypeNode::Amount(_)
        | TypeNode::Boolean(_)
        | TypeNode::DateTime(_)
        | TypeNode::SolAmount(_) => true,
        _ => true,
    }
}
