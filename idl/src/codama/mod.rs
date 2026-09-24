mod cursor;
mod parsed_arg;
mod schema;

use std::cell::RefCell;
use std::collections::HashSet;

use crate::{
    codama::{
        parsed_arg::{
            eq_instruciton_input_value_node, eq_value_node, get_parsed_arg_value,
            get_view_struct_type_node,
        },
        schema::analyze_codama_program,
    },
    cursor::Cursor,
    display_error_name, IdlTreeParser,
};
use codama_nodes::{
    DefaultValueStrategy, DiscriminatorNode, InstructionNode, NestedTypeNodeTrait, RootNode,
    StructTypeNode, TypeNode, ValueNode,
};
use seer_core::tree::parsed::{
    ParsedAccount, ParsedArg, ParsedArgValue, ParsedInstruction, ProgramIdentifier,
};
use solana_instruction_error::InstructionError;

pub struct CodamaIdlLookup {
    root_node: RootNode,
    skipped_instructions: RefCell<HashSet<String>>,
    skipped_accounts: RefCell<HashSet<String>>,
}

impl IdlTreeParser for CodamaIdlLookup {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction> {
        let ix = self.get_instruction_node(data)?;

        let mut parsed_args = vec![];
        let mut account_names = vec![];

        for account in &ix.accounts {
            account_names.push(String::from(account.name.clone()));
        }

        let arg_count = ix.arguments.len();
        let mut cur = Cursor::new(data);
        for (idx, argument) in ix.arguments.iter().enumerate() {
            let parsed_arg_value = get_parsed_arg_value(
                &argument.r#type,
                &mut cur,
                &self.root_node.program.defined_types,
                idx.saturating_add(1) == arg_count,
                None,
            );

            if argument.default_value_strategy == Some(DefaultValueStrategy::Omitted) {
                continue;
            }

            if let Some(parsed_arg_value) = parsed_arg_value {
                parsed_args.push(ParsedArg {
                    name: argument.name.clone().to_string(),
                    value: parsed_arg_value,
                });
            } else {
                break;
            }
        }

        Some(ParsedInstruction {
            id: ProgramIdentifier::Default,
            name: String::from(ix.name.clone()),
            account_names,
            args: parsed_args,
        })
    }

    fn get_account(&self, data: &[u8]) -> Option<ParsedAccount> {
        let program = &self.root_node.program;
        let data_len = data.len();
        let mut best_match: Option<(bool, usize, ParsedAccount)> = None;

        for ax in &program.accounts {
            if !self.account_discriminators_match(ax, data) {
                continue;
            }

            let parsed_struct = {
                let mut cur = Cursor::new(data);
                let inner_struct: &StructTypeNode = ax.data.get_nested_type_node();
                let defined_types = &program.defined_types;

                get_view_struct_type_node(inner_struct, &mut cur, defined_types, true)
            };

            if let Some(parsed_struct) = parsed_struct {
                let parsed_arg = ParsedArg {
                    name: String::from(ax.name.clone()),
                    value: ParsedArgValue::Struct(parsed_struct),
                };
                let parsed = ParsedAccount {
                    id: ProgramIdentifier::Default,
                    data: parsed_arg,
                };
                let size_hint = self.account_size_discriminator_hint(ax);
                let exact_size_match = size_hint == data_len;
                match &mut best_match {
                    Some((best_exact, best_size, best_parsed)) => {
                        let should_replace = (exact_size_match && !*best_exact)
                            || (exact_size_match == *best_exact && size_hint > *best_size);
                        if should_replace {
                            *best_exact = exact_size_match;
                            *best_size = size_hint;
                            *best_parsed = parsed;
                        }
                    }
                    None => {
                        best_match = Some((exact_size_match, size_hint, parsed));
                    }
                }
            }
        }

        match best_match {
            Some((_, _, parsed)) => Some(parsed),
            None => {
                seer_core::seer_warn!(
                    "Codama account decode: no account type matched ({} bytes)",
                    data_len
                );
                None
            }
        }
    }

    fn get_error(&self, error: InstructionError) -> String {
        match error {
            InstructionError::Custom(code) => self
                .root_node
                .program
                .errors
                .iter()
                .find(|e| e.code == code as usize)
                .map(|e| format!("{}: {}", display_error_name(e.name.as_ref()), e.message))
                .unwrap_or_else(|| InstructionError::Custom(code).to_string()),
            _ => error.to_string(),
        }
    }
}

impl CodamaIdlLookup {
    fn type_supports_remainder(ty: &TypeNode) -> bool {
        match ty {
            TypeNode::RemainderOption(_) => true,
            TypeNode::Struct(s) => s
                .fields
                .last()
                .is_some_and(|field| Self::type_supports_remainder(&field.r#type)),
            TypeNode::Tuple(t) => t.items.last().is_some_and(Self::type_supports_remainder),
            TypeNode::Array(a) => Self::type_supports_remainder(&a.item),
            TypeNode::Set(s) => Self::type_supports_remainder(&s.item),
            TypeNode::Map(m) => Self::type_supports_remainder(&m.value),
            TypeNode::Option(o) => Self::type_supports_remainder(&o.item),
            TypeNode::Sentinel(s) => Self::type_supports_remainder(&s.r#type),
            TypeNode::ZeroableOption(z) => Self::type_supports_remainder(&z.item),
            TypeNode::HiddenPrefix(h) => Self::type_supports_remainder(&h.r#type),
            TypeNode::HiddenSuffix(h) => Self::type_supports_remainder(&h.r#type),
            TypeNode::FixedSize(f) => Self::type_supports_remainder(&f.r#type),
            TypeNode::SizePrefix(s) => Self::type_supports_remainder(&s.r#type),
            TypeNode::PostOffset(p) => Self::type_supports_remainder(&p.r#type),
            TypeNode::PreOffset(p) => Self::type_supports_remainder(&p.r#type),
            TypeNode::Link(_) => false,
            _ => false,
        }
    }

    fn account_supports_size_extensions(&self, account: &codama_nodes::AccountNode) -> bool {
        let inner_struct: &StructTypeNode = account.data.get_nested_type_node();
        inner_struct
            .fields
            .last()
            .is_some_and(|field| Self::type_supports_remainder(&field.r#type))
    }

    /// Best-effort size hint from codama account discriminators.
    /// When multiple accounts match by size extension, prefer exact size, then largest base size.
    fn account_size_discriminator_hint(&self, account: &codama_nodes::AccountNode) -> usize {
        account
            .discriminators
            .iter()
            .filter_map(|d| match d {
                DiscriminatorNode::Size(node) => Some(node.size),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    pub fn from_json_str(idl_json: &str) -> Result<Self, serde_json::Error> {
        let root_node: RootNode = serde_json::from_str(idl_json)?;
        let mut skipped_instructions = HashSet::new();
        let mut skipped_accounts = HashSet::new();
        analyze_codama_program(
            &root_node.program,
            &mut skipped_instructions,
            &mut skipped_accounts,
        );
        Ok(Self {
            root_node,
            skipped_instructions: RefCell::new(skipped_instructions),
            skipped_accounts: RefCell::new(skipped_accounts),
        })
    }

    fn parse_at_offset_matches<F>(
        &self,
        data: &[u8],
        offset: usize,
        is_last: bool,
        ty: &codama_nodes::TypeNode,
        cmp: F,
    ) -> bool
    where
        F: FnOnce(&ParsedArgValue) -> bool,
    {
        let defined_types = &self.root_node.program.defined_types;

        if offset >= data.len() {
            return false;
        }

        let slice = &data[offset..];
        let mut cur = Cursor::new(slice);
        get_parsed_arg_value(ty, &mut cur, defined_types, is_last, None)
            .is_some_and(|parsed| cmp(&parsed))
    }

    fn discriminator_matches<'a, F>(
        &'a self,
        data: &[u8],
        is_last: bool,
        discriminator: &DiscriminatorNode,
        allow_size_extensions: bool,
        resolve_field: F,
    ) -> bool
    where
        F: FnOnce(&str) -> Option<(&'a TypeNode, Box<dyn Fn(&ParsedArgValue) -> bool + 'a>)>,
    {
        match discriminator {
            DiscriminatorNode::Constant(node) => self.parse_at_offset_matches(
                data,
                node.offset,
                is_last,
                &node.constant.r#type,
                |parsed| eq_value_node(parsed, &node.constant.value),
            ),
            DiscriminatorNode::Field(node) => {
                let field_name = node.name.to_string();
                let Some((ty, cmp)) = resolve_field(&field_name) else {
                    return false;
                };
                self.parse_at_offset_matches(data, node.offset, is_last, ty, move |parsed| {
                    cmp(parsed)
                })
            }
            DiscriminatorNode::Size(node) => {
                data.len() == node.size || (allow_size_extensions && data.len() > node.size)
            }
        }
    }

    fn get_instruction_node(&self, data: &[u8]) -> Option<&InstructionNode> {
        let program = &self.root_node.program;
        let skipped = self.skipped_instructions.borrow();
        let single_instruction_program = program.instructions.len() == 1;

        for ix in &program.instructions {
            if skipped.contains(ix.name.as_ref()) {
                continue;
            }
            if ix.discriminators.is_empty() {
                if single_instruction_program {
                    return Some(ix);
                }
                continue;
            }
            let mut all_discriminators_match = true;
            for discriminator in &ix.discriminators {
                if !self.discriminator_matches(data, true, discriminator, false, |field_name| {
                    let argument = ix
                        .arguments
                        .iter()
                        .find(|arg| arg.name.to_string() == field_name)?;
                    let default_value = argument.default_value.as_ref()?;
                    Some((
                        &argument.r#type,
                        Box::new(move |parsed| {
                            eq_instruciton_input_value_node(parsed, default_value)
                        }),
                    ))
                }) {
                    all_discriminators_match = false;
                    break;
                }
            }
            if all_discriminators_match {
                return Some(ix);
            }
        }
        None
    }

    fn account_discriminators_match(&self, ax: &codama_nodes::AccountNode, data: &[u8]) -> bool {
        if self.skipped_accounts.borrow().contains(ax.name.as_ref()) {
            return false;
        }
        if ax.discriminators.is_empty() {
            return false;
        }

        let allow_size_extensions = self.account_supports_size_extensions(ax);

        for discriminator in &ax.discriminators {
            let matches = self.discriminator_matches(
                data,
                true,
                discriminator,
                allow_size_extensions,
                |field_name| {
                    let inner_struct: &StructTypeNode = ax.data.get_nested_type_node();
                    let field = inner_struct
                        .fields
                        .iter()
                        .find(|field| field.name.to_string() == field_name)?;
                    let default_value: &ValueNode = field.default_value.as_ref()?;
                    Some((
                        &field.r#type,
                        Box::new(move |parsed| eq_value_node(parsed, default_value)),
                    ))
                },
            );
            if !matches {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use crate::display_error_name;

    #[test]
    fn display_error_name_promotes_camel_case_to_pascal_case() {
        assert_eq!(display_error_name("camelCaseError"), "CamelCaseError");
    }

    #[test]
    fn display_error_name_keeps_pascal_case() {
        assert_eq!(display_error_name("AlreadyPascal"), "AlreadyPascal");
    }
}
