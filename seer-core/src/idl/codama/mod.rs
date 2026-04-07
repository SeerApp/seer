mod ctx;
mod cursor;
mod parsed_arg;
mod schema;

use std::cell::RefCell;
use std::collections::HashSet;

use crate::idl::{
    codama::{
        ctx::{CodamaParseCtx, ParseSite},
        parsed_arg::{
            eq_instruciton_input_value_node, eq_value_node, get_parsed_arg_value,
            get_view_struct_type_node,
        },
        schema::analyze_codama_program,
    },
    cursor::Cursor,
    parsed_arg::{ParsedArg, ParsedArgValue},
    types::{ParsedAccount, ParsedInstruction, ProgramIdentifier},
    IdlIssue, IdlIssues, IdlProgramContext, IdlTreeParser,
};
use codama_nodes::{
    DefaultValueStrategy, DiscriminatorNode, InstructionNode, NestedTypeNodeTrait, RootNode,
    StructTypeNode, TypeNode, ValueNode,
};
use solana_instruction_error::InstructionError;

pub struct CodamaIdlLookup {
    root_node: RootNode,
    idl_issues: RefCell<IdlIssues>,
    skipped_instructions: RefCell<HashSet<String>>,
    skipped_accounts: RefCell<HashSet<String>>,
}

impl IdlTreeParser for CodamaIdlLookup {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction> {
        let ix = self.get_instruction_node(data)?;
        let mut issues = self.idl_issues.borrow_mut();
        let mut ctx = CodamaParseCtx {
            issues: &mut *issues,
            site: ParseSite::RuntimeInstruction {
                name: ix.name.to_string(),
            },
            path: vec![],
        };

        let mut parsed_args = vec![];
        let mut account_names = vec![];

        for account in &ix.accounts {
            account_names.push(String::from(account.name.clone()));
        }

        let arg_count = ix.arguments.len();
        let mut cur = Cursor::new(data);
        for (idx, argument) in ix.arguments.iter().enumerate() {
            ctx.path = vec![argument.name.to_string()];
            let parsed_arg_value = get_parsed_arg_value(
                &mut ctx,
                &argument.r#type,
                &mut cur,
                &self.root_node.program.defined_types,
                idx + 1 == arg_count,
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
        let idx = self.find_matching_account_index(data)?;
        let program = &self.root_node.program;
        let ax = &program.accounts[idx];
        let mut issues = self.idl_issues.borrow_mut();
        let mut ctx = CodamaParseCtx {
            issues: &mut *issues,
            site: ParseSite::RuntimeAccount {
                name: ax.name.to_string(),
            },
            path: vec![],
        };
        let mut cur = Cursor::new(data);
        let inner_struct: &StructTypeNode = ax.data.get_nested_type_node();
        let defined_types = &program.defined_types;

        let parsed_arg = ParsedArg {
            name: String::from(ax.name.clone()),
            value: ParsedArgValue::Struct(get_view_struct_type_node(
                inner_struct,
                &mut cur,
                &mut ctx,
                defined_types,
                false,
            )?),
        };

        Some(ParsedAccount {
            id: ProgramIdentifier::Default,
            data: parsed_arg,
        })
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
        let ctx = IdlProgramContext::new(
            root_node.program.name.to_string(),
            root_node.program.public_key.clone(),
        );
        let mut issues = IdlIssues::new(ctx);
        let mut skipped_instructions = HashSet::new();
        let mut skipped_accounts = HashSet::new();
        analyze_codama_program(
            &root_node.program,
            &mut issues,
            &mut skipped_instructions,
            &mut skipped_accounts,
        );
        Ok(Self {
            root_node,
            idl_issues: RefCell::new(issues),
            skipped_instructions: RefCell::new(skipped_instructions),
            skipped_accounts: RefCell::new(skipped_accounts),
        })
    }

    /// Deterministic `IdlIssue` list for tests and diagnostics (includes schema-time issues).
    pub fn sorted_idl_issues(&self) -> Vec<IdlIssue> {
        self.idl_issues.borrow().sorted_issues()
    }

    fn parse_at_offset_matches<F>(
        &self,
        data: &[u8],
        offset: usize,
        is_last: bool,
        ty: &codama_nodes::TypeNode,
        ctx: &mut CodamaParseCtx<'_>,
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
        get_parsed_arg_value(ctx, ty, &mut cur, defined_types, is_last, None)
            .map_or(false, |parsed| cmp(&parsed))
    }

    fn discriminator_matches<'a, F>(
        &'a self,
        data: &[u8],
        is_last: bool,
        discriminator: &DiscriminatorNode,
        ctx: &mut CodamaParseCtx<'_>,
        resolve_field: F,
    ) -> bool
    where
        F: FnOnce(&str) -> Option<(&'a TypeNode, Box<dyn Fn(&ParsedArgValue) -> bool + 'a>)>,
    {
        match discriminator {
            DiscriminatorNode::Constant(node) => {
                ctx.path.clear();
                self.parse_at_offset_matches(
                    data,
                    node.offset,
                    is_last,
                    &node.constant.r#type,
                    ctx,
                    |parsed| eq_value_node(parsed, &node.constant.value, None),
                )
            }
            DiscriminatorNode::Field(node) => {
                let field_name = node.name.to_string();
                let (ty, cmp) = match resolve_field(&field_name) {
                    Some(v) => v,
                    None => return false,
                };
                let old_path =
                    std::mem::replace(&mut ctx.path, vec![field_name]);
                let out = self.parse_at_offset_matches(data, node.offset, is_last, ty, ctx, move |parsed| {
                    cmp(parsed)
                });
                ctx.path = old_path;
                out
            }
            DiscriminatorNode::Size(node) => data.len() == node.size,
        }
    }

    fn get_instruction_node(&self, data: &[u8]) -> Option<&InstructionNode> {
        let program = &self.root_node.program;
        let skipped = self.skipped_instructions.borrow();
        let mut issues = self.idl_issues.borrow_mut();
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
            let mut ctx = CodamaParseCtx {
                issues: &mut *issues,
                site: ParseSite::DiscriminatorInstruction {
                    name: ix.name.to_string(),
                },
                path: vec![],
            };
            let mut all_discriminators_match = true;
            for discriminator in &ix.discriminators {
                if !self.discriminator_matches(
                    data,
                    true,
                    discriminator,
                    &mut ctx,
                    |field_name| {
                        let argument = ix
                            .arguments
                            .iter()
                            .find(|arg| arg.name.to_string() == field_name)?;
                        let default_value = argument.default_value.as_ref()?;
                        Some((
                            &argument.r#type,
                            Box::new(move |parsed| {
                                eq_instruciton_input_value_node(parsed, default_value, None)
                            }),
                        ))
                    },
                ) {
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

    fn find_matching_account_index(&self, data: &[u8]) -> Option<usize> {
        let accounts = &self.root_node.program.accounts;

        if accounts.len() == 1 {
            let ax = &accounts[0];
            if ax.discriminators.is_empty() && ax.size == Some(data.len()) {
                return Some(0);
            }
        }

        let skipped_acc = self.skipped_accounts.borrow();

        for (i, ax) in accounts.iter().enumerate() {
            if skipped_acc.contains(ax.name.as_ref()) {
                continue;
            }
            if ax.discriminators.is_empty() {
                continue;
            }

            let mut issues = self.idl_issues.borrow_mut();
            let mut ctx = CodamaParseCtx {
                issues: &mut *issues,
                site: ParseSite::DiscriminatorAccount {
                    name: ax.name.to_string(),
                },
                path: vec![],
            };
            let mut all_discriminators_match = true;
            for discriminator in &ax.discriminators {
                if !self.discriminator_matches(data, true, discriminator, &mut ctx, |field_name| {
                    let inner_struct: &StructTypeNode = ax.data.get_nested_type_node();
                    let field = inner_struct
                        .fields
                        .iter()
                        .find(|field| field.name.to_string() == field_name)?;
                    let default_value: &ValueNode = field.default_value.as_ref()?;
                    Some((
                        &field.r#type,
                        Box::new(move |parsed| eq_value_node(parsed, default_value, None)),
                    ))
                }) {
                    all_discriminators_match = false;
                    break;
                }
            }
            if all_discriminators_match {
                return Some(i);
            }
        }

        None
    }
}
