pub mod cursor;
pub mod parsed_arg;

use codama_nodes::{
    DefaultValueStrategy, DiscriminatorNode, InstructionInputValueNode, InstructionNode, Number,
    NumberFormat::{self},
    RootNode, TypeNode,
};
use solana_instruction_error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    idl::{lookup::{cursor::Cursor, parsed_arg::ParsedArg}, types::{ParsedAccount, ParsedInstruction}},
    tree::nodes::{RootChildren, TreeRoot},
};

pub struct IdlLookup {
    root_node: RootNode,
}

impl IdlLookup {
    pub fn new(root_node: RootNode) -> Self {
        Self { root_node }
    }

    pub fn parse_tree(&self, _: &mut TreeRoot<RootChildren>) {}

    fn get_instruction(
        &self,
        accounts: &Vec<Pubkey>,
        data: &mut &[u8],
    ) -> Option<&ParsedInstruction> {
        if let Some(ix) = self.get_instruction_node(data) {
            for argument in &ix.arguments {
                let mut cur = Cursor::new(data);
                let parsed_arg = ParsedArg::from(&argument.r#type, &mut cur);

                if let Some(default_value_strategy) = argument.default_value_strategy {
                    match default_value_strategy {
                        DefaultValueStrategy::Omitted => {
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            None
        } else {
            None
        }
    }

    fn get_instruction_node(&self, data: &mut &[u8]) -> Option<&InstructionNode> {
        for ix in &self.root_node.program.instructions {
            if ix.discriminators.len() != 1 {
                continue;
            }

            let matched = match &ix.discriminators[0] {
                DiscriminatorNode::Constant(_) => false,
                DiscriminatorNode::Field(node) => {
                    match ix.arguments.iter().find(|arg| {
                        arg.name.to_string() == node.name.to_string()
                            && matches!(
                                &arg.r#type,
                                TypeNode::Number(nt) if nt.format == NumberFormat::U8
                            )
                    }) {
                        Some(argument) if node.offset < data.len() => {
                            match argument.default_value.as_ref() {
                                Some(InstructionInputValueNode::Number(number)) => {
                                    match number.number {
                                        Number::UnsignedInteger(n) => {
                                            // We explicitly skip non-0 offset cases for now.
                                            if node.offset != 0 {
                                                false
                                            } else {
                                                data[node.offset] == n as u8
                                            }
                                        }
                                        Number::SignedInteger(n) => {
                                            // We explicitly skip non-0 offset cases for now.
                                            if node.offset != 0 {
                                                false
                                            } else {
                                                data[node.offset] == n as u8
                                            }
                                        }
                                        Number::Float(_) => false,
                                    }
                                }
                                _ => false,
                            }
                        }
                        _ => false,
                    }
                }
                DiscriminatorNode::Size(_) => false,
            };

            if matched {
                return Some(ix);
            }
        }

        None
    }

    fn get_message(&self, message: String) -> String;

    fn get_error(&self, error: Option<InstructionError>) -> Option<String>;

    fn get_account(&self, data: &Vec<u8>) -> Option<ParsedAccount>;
}
