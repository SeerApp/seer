pub mod cursor;
pub mod parsed_arg;

// use codama_nodes::{
//     AccountNode, DefaultValueStrategy, DiscriminatorNode, InstructionNode, NestedTypeNodeTrait,
//     RootNode, StructTypeNode, ValueNode,
// };
// use solana_pubkey::Pubkey;

// use crate::{
//     idl::{
//         lookup::{
//             cursor::Cursor,
//             parsed_arg::{ParsedArg, ParsedArgValue, ViewStructTypeNode},
//         },
//         types::{ParsedAccount, ParsedInstruction, ProgramIdentifier},
//     },
//     tree::nodes::{RootChildren, TreeRoot},
// };

// // pub struct IdlLookup {
// //     root_node: RootNode,
// // }

// pub enum IdlLookup {
//     Anchor()
//     Codama()
// }

// impl IdlLookup {
//     pub fn new(root_node: RootNode) -> Self {
//         Self { root_node }
//     }

//     pub fn parse_tree(&self, _: &mut TreeRoot<RootChildren>) {}

//     fn parse_at_offset_matches<F>(
//         &self,
//         data: &[u8],
//         offset: usize,
//         is_last: bool,
//         ty: &codama_nodes::TypeNode,
//         cmp: F,
//     ) -> bool
//     where
//         F: FnOnce(&ParsedArgValue) -> bool,
//     {
//         let defined_types = &self.root_node.program.defined_types;

//         if offset >= data.len() {
//             return false;
//         }

//         let slice = &data[offset..];
//         let mut cur = Cursor::new(slice);
//         ParsedArgValue::from(ty, &mut cur, defined_types, is_last, None)
//             .map_or(false, |parsed| cmp(&parsed))
//     }

//     fn discriminator_matches<'a, F>(
//         &'a self,
//         data: &[u8],
//         is_last: bool,
//         discriminator: &DiscriminatorNode,
//         resolve_field: F,
//     ) -> bool
//     where
//         F: FnOnce(
//             &str,
//         ) -> Option<(
//             &'a codama_nodes::TypeNode,
//             Box<dyn Fn(&ParsedArgValue) -> bool + 'a>,
//         )>,
//     {
//         match discriminator {
//             DiscriminatorNode::Constant(node) => {
//                 self.parse_at_offset_matches(data, node.offset, is_last, &node.constant.r#type, |parsed| {
//                     parsed.eq_value_node(&node.constant.value)
//                 })
//             }
//             DiscriminatorNode::Field(node) => {
//                 let field_name = node.name.to_string();
//                 let (ty, cmp) = match resolve_field(&field_name) {
//                     Some(v) => v,
//                     None => return false,
//                 };
//                 self.parse_at_offset_matches(data, node.offset, is_last, ty, move |parsed| cmp(parsed))
//             }
//             DiscriminatorNode::Size(node) => data.len() == node.size,
//         }
//     }

//     pub fn get_instruction(&self, _: &Vec<Pubkey>, data: &[u8]) -> Option<ParsedInstruction> {
//         if let Some(ix) = self.get_instruction_node(data) {
//             let mut parsed_args = vec![];
//             let mut account_names = vec![];

//             for account in &ix.accounts {
//                 account_names.push(String::from(account.name.clone()));
//             }

//             let arg_count = ix.arguments.len();
//             let mut cur = Cursor::new(data);
//             for (idx, argument) in ix.arguments.iter().enumerate() {
//                 let parsed_arg_value = ParsedArgValue::from(
//                     &argument.r#type,
//                     &mut cur,
//                     &self.root_node.program.defined_types,
//                     idx + 1 == arg_count,
//                     None,
//                 );

//                 if let Some(default_value_strategy) = argument.default_value_strategy {
//                     match default_value_strategy {
//                         DefaultValueStrategy::Omitted => {
//                             continue;
//                         }
//                         _ => {}
//                     }
//                 }

//                 if let Some(parsed_arg_value) = parsed_arg_value {
//                     parsed_args.push(ParsedArg {
//                         name: argument.name.clone().to_string(),
//                         value: parsed_arg_value,
//                     });
//                 }
//             }

//             Some(ParsedInstruction {
//                 id: ProgramIdentifier::Default,
//                 name: String::from(ix.name.clone()),
//                 account_names,
//                 args: parsed_args,
//             })
//         } else {
//             None
//         }
//     }

//     fn get_instruction_node(&self, data: &[u8]) -> Option<&InstructionNode> {
//         self.root_node
//             .program
//             .instructions
//             .iter()
//             .filter(|ix| ix.discriminators.len() == 1)
//             .find_map(|ix| {
//                 self.discriminator_matches(data, true, &ix.discriminators[0], |field_name| {
//                     let argument = ix
//                         .arguments
//                         .iter()
//                         .find(|arg| arg.name.to_string() == field_name)?;
//                     let default_value = argument.default_value.as_ref()?;
//                     Some((
//                         &argument.r#type,
//                         Box::new(move |parsed| {
//                             parsed.eq_instruciton_input_value_node(default_value)
//                         }),
//                     ))
//                 })
//                 .then_some(ix)
//             })
//     }

//     pub fn get_account(&self, data: &[u8]) -> Option<ParsedAccount> {
//         if let Some(ax) = self.get_account_node(data) {
//             let mut cur = Cursor::new(data);
//             let inner_struct: &StructTypeNode = ax.data.get_nested_type_node();

//             let defined_types = &self.root_node.program.defined_types;

//             let parsed_arg = ParsedArg {
//                 name: String::from(ax.name.clone()),
//                 value: ParsedArgValue::Struct(ViewStructTypeNode::from(
//                     inner_struct,
//                     &mut cur,
//                     defined_types,
//                     false,
//                 )),
//             };

//             Some(ParsedAccount {
//                 id: ProgramIdentifier::Default,
//                 data: parsed_arg,
//             })
//         } else {
//             None
//         }
//     }

//     fn get_account_node(&self, data: &[u8]) -> Option<&AccountNode> {
//         self.root_node
//             .program
//             .accounts
//             .iter()
//             .filter(|ax| ax.discriminators.len() == 1)
//             .find_map(|ax| {
//                 self.discriminator_matches(data, true, &ax.discriminators[0], |field_name| {
//                     let inner_struct: &StructTypeNode = ax.data.get_nested_type_node();
//                     let field = inner_struct
//                         .fields
//                         .iter()
//                         .find(|field| field.name.to_string() == field_name)?;
//                     let default_value: &ValueNode = field.default_value.as_ref()?;
//                     Some((
//                         &field.r#type,
//                         Box::new(move |parsed| parsed.eq_value_node(default_value)),
//                     ))
//                 })
//                 .then_some(ax)
//             })
//     }

//     // fn get_message(&self, message: String) -> String;

//     // fn get_error(&self, error: Option<InstructionError>) -> Option<String>;
// }

