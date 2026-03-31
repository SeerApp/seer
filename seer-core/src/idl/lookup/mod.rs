pub mod cursor;
pub mod parsed_arg;

use codama_nodes::{
    DefaultValueStrategy, DiscriminatorNode, InstructionInputValueNode, InstructionNode, Number,
    NumberFormat::{self},
    RootNode, TypeNode,
};
use solana_pubkey::Pubkey;

use crate::{
    idl::{
        lookup::{
            cursor::Cursor,
            parsed_arg::{ParsedArg, ParsedArgValue},
        },
        types::{ParsedInstruction, ProgramIdentifier},
    },
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

    pub fn get_instruction(&self, _: &Vec<Pubkey>, data: &mut &[u8]) -> Option<ParsedInstruction> {
        if let Some(ix) = self.get_instruction_node(data) {
            let mut parsed_args = vec![];
            let mut account_names = vec![];

            for account in &ix.accounts {
                account_names.push(String::from(account.name.clone()));
            }

            let arg_count = ix.arguments.len();
            let mut cur = Cursor::new(data);
            for (idx, argument) in ix.arguments.iter().enumerate() {
                let parsed_arg_value = ParsedArgValue::from(
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

    // fn get_message(&self, message: String) -> String;

    // fn get_error(&self, error: Option<InstructionError>) -> Option<String>;

    // fn get_account(&self, data: &Vec<u8>) -> Option<ParsedAccount>;
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use solana_pubkey::Pubkey;

    use crate::program_manager::known_programs::get_known_programs;

    #[test]
    fn test_token_program_initialize_instruction_parse() {
        let (_, token_program_idl_lookup) = &get_known_programs()[0];

        let accounts = vec![
            Pubkey::from_str("Gqdd8HC3FW5dBR2F6aNZagZrGbbUiHiMBq7oA7m8CfU").unwrap(),
            Pubkey::from_str("HYTLyA85bXocKVYnQuNPFqCtF4xXfFSvC17awsonMfV9").unwrap(),
        ];
        let invoke_data: Vec<u8> = vec![
            18, 138, 129, 140, 92, 164, 216, 236, 167, 115, 236, 179, 240, 124, 136, 192, 103, 163,
            246, 23, 107, 172, 219, 65, 180, 86, 198, 48, 116, 73, 165, 107, 248,
        ];

        let mut data: &[u8] = &invoke_data[..];
        let parsed_ix = token_program_idl_lookup.get_instruction(&accounts, &mut data);

        println!("{:?}", parsed_ix);
    }

    #[test]
    fn test_token_program_parse() {
        let (_, token_program_idl_lookup) = &get_known_programs()[0];

        let accounts = vec![
            Pubkey::from_str("Gqdd8HC3FW5dBR2F6aNZagZrGbbUiHiMBq7oA7m8CfU").unwrap(),
            Pubkey::from_str("AKfqTU9gGTCXoAjiZEpKt5x9fBwD3ZU4D3wCJ87KZqTu").unwrap(),
        ];
        let invoke_data: Vec<u8> = vec![
            6, 2, 1, 30, 185, 24, 117, 164, 97, 180, 227, 158, 14, 164, 123, 209, 60, 254, 122,
            218, 196, 232, 100, 78, 46, 201, 109, 241, 146, 1, 242, 62, 118, 123, 76,
        ];

        let mut data: &[u8] = &invoke_data[..];
        let parsed_ix = token_program_idl_lookup.get_instruction(&accounts, &mut data);

        println!("{:?}", parsed_ix);
    }
}
