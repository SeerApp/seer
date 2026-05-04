use serde::{Deserialize, Serialize};

use crate::idl::parsed_arg::ParsedArg;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ParsedInstruction {
    pub id: ProgramIdentifier,
    pub name: String,
    pub account_names: Vec<String>,
    pub args: Vec<ParsedArg>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ParsedAccount {
    pub id: ProgramIdentifier,
    pub data: ParsedArg,
}

/// Identifier to inform UI. Used for special cases which require unique
/// display options.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ProgramIdentifier {
    Default,
}
