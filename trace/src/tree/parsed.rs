use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_pubkey::Pubkey;

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

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberFormat {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    ShortU16,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ParsedArg {
    pub name: String,
    pub value: ParsedArgValue,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "type", content = "value")]
pub enum ParsedArgValue {
    Amount(ViewAmountTypeNode),
    Array(ViewArrayTypeNode),
    Boolean(ViewBooleanTypeNode),
    Bytes(ViewBytesTypeNode),
    DateTime(ViewDateTimeTypeNode),
    Enum(ViewEnumTypeNode),
    HiddenPrefix(ViewHiddenPrefixTypeNode),
    HiddenSuffix(ViewHiddenSuffixTypeNode),
    Map(ViewMapTypeNode),
    Number(ViewNumberTypeNode),
    Struct(ViewStructTypeNode),
    Option(ViewOptionTypeNode),
    PublicKey(ViewPublicKeyTypeNode),
    Tuple(ViewTupleTypeNode),
    String(ViewStringTypeNode),
    Set(ViewSetTypeNode),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewNumberTypeNode {
    pub value: String,
    pub format: NumberFormat,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewStructTypeNode {
    pub fields: Vec<ViewStructFieldTypeNode>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewStructFieldTypeNode {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub docs: Vec<String>,
    #[serde(skip, default)]
    pub byte_offset: Option<usize>,
    pub value: ParsedArgValue,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewOptionTypeNode {
    pub value: Box<Option<ParsedArgValue>>,
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewPublicKeyTypeNode {
    #[serde_as(as = "DisplayFromStr")]
    pub value: Pubkey,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewStringTypeNode {
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewAmountTypeNode {
    pub decimals: u8,
    pub unit: Option<String>,
    pub number: ViewNumberTypeNode,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewBooleanTypeNode {
    pub value: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewArrayTypeNode {
    pub values: Vec<ParsedArgValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewSetTypeNode {
    pub values: Vec<ParsedArgValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewTupleTypeNode {
    pub items: Vec<ParsedArgValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewBytesTypeNode {
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewDateTimeTypeNode {
    pub value: String,
    pub format: NumberFormat,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewHiddenPrefixTypeNode {
    pub prefix: Vec<ParsedArgValue>,
    pub value: Box<ParsedArgValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewHiddenSuffixTypeNode {
    pub suffix: Vec<ParsedArgValue>,
    pub value: Box<ParsedArgValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewMapTypeNode {
    pub entries: Vec<ViewMapEntryTypeNode>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewMapEntryTypeNode {
    pub key: ParsedArgValue,
    pub value: ParsedArgValue,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewEnumTypeNode {
    pub discriminant: String,
    pub name: String,
    pub value: ViewEnumValue,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum ViewEnumValue {
    Empty,
    Tuple(ViewTupleTypeNode),
    Struct(ViewStructTypeNode),
}
