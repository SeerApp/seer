mod cursor;
mod parsed_arg;

use codama_nodes::{
    AccountNode, DefaultValueStrategy, DiscriminatorNode, InstructionNode, NestedTypeNodeTrait,
    RootNode, StructTypeNode, ValueNode,
};
use solana_instruction_error::InstructionError;
use crate::{
    idl::{
        codama::parsed_arg::{
            eq_instruciton_input_value_node, eq_value_node, get_parsed_arg_value,
            get_view_struct_type_node,
        },
        cursor::Cursor,
        parsed_arg::{ParsedArg, ParsedArgValue},
        types::{ParsedAccount, ParsedInstruction, ProgramIdentifier},
        IdlTreeParser,
    },
};

pub struct CodamaIdlLookup {
    root_node: RootNode,
}

impl IdlTreeParser for CodamaIdlLookup {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction> {
        if let Some(ix) = self.get_instruction_node(data) {
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
                    idx + 1 == arg_count,
                    None,
                );

                if let Some(default_value_strategy) = argument.default_value_strategy {
                    match default_value_strategy {
                        DefaultValueStrategy::Omitted => {
                            continue;
                        }
                        _ => {}
                    }
                }

                if let Some(parsed_arg_value) = parsed_arg_value {
                    parsed_args.push(ParsedArg {
                        name: argument.name.clone().to_string(),
                        value: parsed_arg_value,
                    });
                }
            }

            Some(ParsedInstruction {
                id: ProgramIdentifier::Default,
                name: String::from(ix.name.clone()),
                account_names,
                args: parsed_args,
            })
        } else {
            None
        }
    }

    fn get_account(&self, data: &[u8]) -> Option<ParsedAccount> {
        if let Some(ax) = self.get_account_node(data) {
            let mut cur = Cursor::new(data);
            let inner_struct: &StructTypeNode = ax.data.get_nested_type_node();

            let defined_types = &self.root_node.program.defined_types;

            let parsed_arg = ParsedArg {
                name: String::from(ax.name.clone()),
                value: ParsedArgValue::Struct(get_view_struct_type_node(
                    inner_struct,
                    &mut cur,
                    defined_types,
                    false,
                )),
            };

            Some(ParsedAccount {
                id: ProgramIdentifier::Default,
                data: parsed_arg,
            })
        } else {
            None
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
                .map(|e| format!("{}: {}", e.name.as_ref(), e.message))
                .unwrap_or_else(|| InstructionError::Custom(code).to_string()),
            _ => error.to_string(),
        }
    }
}

impl CodamaIdlLookup {
    pub fn from_json_str(idl_json: &str) -> Result<Self, serde_json::Error> {
        let root_node: RootNode = serde_json::from_str(idl_json)?;
        Ok(Self { root_node })
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
            .map_or(false, |parsed| cmp(&parsed))
    }

    fn discriminator_matches<'a, F>(
        &'a self,
        data: &[u8],
        is_last: bool,
        discriminator: &DiscriminatorNode,
        resolve_field: F,
    ) -> bool
    where
        F: FnOnce(
            &str,
        ) -> Option<(
            &'a codama_nodes::TypeNode,
            Box<dyn Fn(&ParsedArgValue) -> bool + 'a>,
        )>,
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
                let (ty, cmp) = match resolve_field(&field_name) {
                    Some(v) => v,
                    None => return false,
                };
                self.parse_at_offset_matches(data, node.offset, is_last, ty, move |parsed| {
                    cmp(parsed)
                })
            }
            DiscriminatorNode::Size(node) => data.len() == node.size,
        }
    }

    fn get_instruction_node(&self, data: &[u8]) -> Option<&InstructionNode> {
        self.root_node
            .program
            .instructions
            .iter()
            .filter(|ix| ix.discriminators.len() == 1)
            .find_map(|ix| {
                self.discriminator_matches(data, true, &ix.discriminators[0], |field_name| {
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
                })
                .then_some(ix)
            })
    }

    fn get_account_node(&self, data: &[u8]) -> Option<&AccountNode> {
        let matched = self
            .root_node
            .program
            .accounts
            .iter()
            .filter(|ax| ax.discriminators.len() == 1)
            .find_map(|ax| {
                self.discriminator_matches(data, true, &ax.discriminators[0], |field_name| {
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
                })
                .then_some(ax)
            });

        matched.or_else(|| {
            let accounts = &self.root_node.program.accounts;
            if accounts.len() != 1 {
                return None;
            }
            let ax = &accounts[0];
            if !ax.discriminators.is_empty() {
                return None;
            }
            (ax.size == Some(data.len())).then_some(ax)
        })
    }

    // fn get_message(&self, message: String) -> String;
}
