use core::panic;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use codama_nodes::{
    AmountTypeNode, ArrayTypeNode, BooleanTypeNode, BytesEncoding, BytesValueNode,
    DateTimeTypeNode, DefinedTypeNode, Docs, EnumTypeNode, EnumVariantTypeNode,
    HiddenPrefixTypeNode, HiddenSuffixTypeNode, InstructionInputValueNode, NestedTypeNodeTrait,
    Number, NumberFormat, NumberTypeNode, NumberValueNode, OptionTypeNode, SetTypeNode,
    StringTypeNode, StructTypeNode, TupleTypeNode, TypeNode, ValueNode,
};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_pubkey::Pubkey;

use crate::idl::lookup::cursor::Cursor;

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
    // Sentinel(ViewSentinelTypeNode),
    Tuple(ViewTupleTypeNode),
    String(ViewStringTypeNode),
    Set(ViewSetTypeNode),
}

impl ParsedArgValue {
    pub fn from<'a>(
        origin: &TypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
        passed_len: Option<usize>,
    ) -> Option<Self> {
        match origin {
            TypeNode::Link(link) => {
                let resolved = defined_types
                    .iter()
                    .find(|dt| dt.name == link.name)
                    .unwrap_or_else(|| {
                        panic!("defined type not found for link {:?}", link.name);
                    });
                Self::from(&resolved.r#type, cur, defined_types, is_last, passed_len)
            }
            TypeNode::Amount(a) => Some(ParsedArgValue::Amount(ViewAmountTypeNode::from(a, cur))),
            TypeNode::Array(a) => Some(ParsedArgValue::Array(ViewArrayTypeNode::from(
                a,
                cur,
                defined_types,
                is_last,
            ))),
            TypeNode::Boolean(b) => {
                Some(ParsedArgValue::Boolean(ViewBooleanTypeNode::from(b, cur)))
            }
            TypeNode::Bytes(_) => {
                if !is_last && passed_len.is_none() {
                    panic!("No passed length detected for non-final argument");
                }
                Some(ParsedArgValue::Bytes(ViewBytesTypeNode::from(
                    cur, passed_len,
                )))
            }
            TypeNode::DateTime(d) => {
                Some(ParsedArgValue::DateTime(ViewDateTimeTypeNode::from(d, cur)))
            }
            TypeNode::Enum(e) => Some(ParsedArgValue::Enum(ViewEnumTypeNode::from(
                e,
                cur,
                defined_types,
                is_last,
            ))),
            TypeNode::HiddenPrefix(h) => Some(ParsedArgValue::HiddenPrefix(
                ViewHiddenPrefixTypeNode::from(h, cur, defined_types, is_last),
            )),
            TypeNode::HiddenSuffix(h) => Some(ParsedArgValue::HiddenSuffix(
                ViewHiddenSuffixTypeNode::from(h, cur, defined_types, is_last),
            )),
            TypeNode::Map(m) => Some(ParsedArgValue::Map(ViewMapTypeNode::from(
                m,
                cur,
                defined_types,
                is_last,
            ))),
            TypeNode::Number(n) => Some(ParsedArgValue::Number(ViewNumberTypeNode::from(n, cur))),
            TypeNode::Struct(s) => Some(ParsedArgValue::Struct(ViewStructTypeNode::from(
                s,
                cur,
                defined_types,
                is_last,
            ))),
            TypeNode::Option(o) => Some(ParsedArgValue::Option(ViewOptionTypeNode::from(
                o,
                cur,
                defined_types,
                is_last,
            ))),
            TypeNode::PublicKey(_) => {
                Some(ParsedArgValue::PublicKey(ViewPublicKeyTypeNode::from(cur)))
            }
            TypeNode::Tuple(t) => Some(ParsedArgValue::Tuple(ViewTupleTypeNode::from(
                t,
                cur,
                defined_types,
                is_last,
            ))),
            TypeNode::String(s) => {
                if !is_last && passed_len.is_none() {
                    panic!("No passed length detected for non-final argument");
                }
                Some(ParsedArgValue::String(ViewStringTypeNode::from(
                    s, cur, passed_len,
                )))
            }
            TypeNode::FixedSize(f) => cur.get_fixed_size_value(f, defined_types),
            TypeNode::PostOffset(p) => cur.get_post_offset_value(p, defined_types, is_last),
            TypeNode::PreOffset(p) => cur.get_pre_offset_value(p, defined_types, is_last),
            TypeNode::RemainderOption(r) => {
                if !is_last {
                    panic!("Remained option must be last argument");
                }
                if !cur.is_empty() {
                    ParsedArgValue::from(&r.item, cur, defined_types, is_last, passed_len)
                } else {
                    None
                }
            }
            TypeNode::Sentinel(_) => panic!("Debugger not yet equipped for Sentinel IDL layouts"),
            TypeNode::Set(s) => Some(ParsedArgValue::Set(ViewSetTypeNode::from(
                s,
                cur,
                defined_types,
                is_last,
            ))),
            TypeNode::SizePrefix(s) => cur.get_dynamic_value(s, defined_types, is_last),
            TypeNode::SolAmount(s) => Some(ParsedArgValue::Number(ViewNumberTypeNode::from(
                s.number.get_nested_type_node(),
                cur,
            ))),
            TypeNode::ZeroableOption(_) => {
                panic!("Debugger not yet equipped for ZeroableOption IDL layouts")
            }
        }
    }

    pub fn eq_value_node(&self, value: &ValueNode) -> bool {
        match (self, value) {
            (ParsedArgValue::Number(arg), ValueNode::Number(node)) => {
                let node: &NumberValueNode = node;
                Self::eq_number_value(arg, node.number)
            }
            (ParsedArgValue::Bytes(arg), ValueNode::Bytes(node)) => {
                let node: &BytesValueNode = node;
                arg.value == ViewBytesTypeNode::from_value(node).value
            }
            _ => false,
        }
    }

    pub fn eq_instruciton_input_value_node(&self, value: &InstructionInputValueNode) -> bool {
        match (self, value) {
            (ParsedArgValue::Number(arg), InstructionInputValueNode::Number(node)) => {
                let node: &NumberValueNode = node;
                Self::eq_number_value(arg, node.number)
            }
            (ParsedArgValue::Bytes(arg), InstructionInputValueNode::Bytes(node)) => {
                let node: &BytesValueNode = node;
                arg.value == ViewBytesTypeNode::from_value(node).value
            }
            _ => false,
        }
    }

    fn eq_number_value(arg: &ViewNumberTypeNode, node_number: Number) -> bool {
        match (arg.format, node_number) {
            (
                NumberFormat::U8
                | NumberFormat::U16
                | NumberFormat::U32
                | NumberFormat::U64
                | NumberFormat::U128,
                Number::UnsignedInteger(node_number),
            ) => arg.value.parse::<u64>().unwrap() == node_number,
            (
                NumberFormat::I8
                | NumberFormat::I16
                | NumberFormat::I32
                | NumberFormat::I64
                | NumberFormat::I128,
                Number::SignedInteger(node_number),
            ) => arg.value.parse::<i64>().unwrap() == node_number,
            (NumberFormat::F32 | NumberFormat::F64, Number::Float(node_number)) => {
                arg.value.parse::<f64>().unwrap() == node_number
            }
            _ => false,
        }
    }

    // Sentinel-related
    // pub fn from_value(origin: &ValueNode) -> Self {
    //     match origin {
    //         ValueNode::Array(v) => {
    //             let v = v as &ArrayValueNode;
    //         }
    //         ValueNode::Boolean(v) => {
    //             let v = v as &BooleanValueNode;
    //             ParsedArgValue::Boolean(ViewBooleanTypeNode::from_value(v.boolean))
    //         }
    //         ValueNode::Bytes(v) => {
    //             let v = v as &BytesValueNode;
    //             ParsedArgValue::Bytes(ViewBytesTypeNode::from_value(v))
    //         }
    //         ValueNode::Constant(v) => {
    //             let v = v as &ConstantValueNode;
    //             ParsedArgValue::from_value(&v.value)
    //         }
    //         ValueNode::Enum(v) => {
    //             let v = v as &EnumValueNode;
    //             ParsedArgValue::Enum(ViewEnumTypeNode::from_value(v))
    //         }
    //         ValueNode::Map(v) => {
    //             let v = v as MapValueNode;
    //         }
    //         ValueNode::None(v) => {
    //             let v = v as NoneValueNode;
    //         }
    //         ValueNode::Number(v) => {
    //             let v = v as NumberValueNode;
    //         }
    //         ValueNode::PublicKey(v) => {
    //             let v = v as PublicKeyValueNode;
    //         }
    //         ValueNode::Set(v) => {
    //             let v = v as SetValueNode;
    //         }
    //         ValueNode::Some(v) => {
    //             let v = v as SomeValueNode;
    //         }
    //         ValueNode::String(v) => {
    //             let v = v as StringValueNode;
    //         }
    //         ValueNode::Struct(v) => {
    //             let v = v as StructValueNode;
    //         }
    //         ValueNode::Tuple(v) => {
    //             let v = v as TupleValueNode;
    //         }
    //     }
    // }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewNumberTypeNode {
    pub value: String,
    pub format: NumberFormat,
}

impl ViewNumberTypeNode {
    pub fn from<'a>(origin: &NumberTypeNode, cur: &mut Cursor<'a>) -> Self {
        Self {
            value: cur.get_number_value(origin),
            format: origin.format.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewStructTypeNode {
    pub fields: Vec<ViewStructFieldTypeNode>,
}

impl ViewStructTypeNode {
    pub fn from<'a>(
        origin: &StructTypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        let mut fields = vec![];

        for field in &origin.fields {
            let view_field: ViewStructFieldTypeNode = ViewStructFieldTypeNode {
                name: String::from(field.name.clone()),
                docs: field.docs.clone(),
                value: ParsedArgValue::from(&field.r#type, cur, defined_types, is_last, None)
                    .expect("Struct field cannot be residual"),
            };

            fields.push(view_field);
        }

        Self { fields }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewStructFieldTypeNode {
    pub name: String,
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub docs: Docs,

    pub value: ParsedArgValue,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewOptionTypeNode {
    pub value: Box<Option<ParsedArgValue>>,
}

impl ViewOptionTypeNode {
    pub fn from<'a>(
        origin: &OptionTypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        Self {
            value: Box::new(cur.get_option_value(origin, defined_types, is_last)),
        }
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewPublicKeyTypeNode {
    #[serde_as(as = "DisplayFromStr")]
    pub value: Pubkey,
}

impl ViewPublicKeyTypeNode {
    pub fn from<'a>(cur: &mut Cursor<'a>) -> Self {
        Self {
            value: cur.get_pubkey_value(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewStringTypeNode {
    pub value: String,
}

impl ViewStringTypeNode {
    pub fn from<'a>(
        origin: &StringTypeNode,
        cur: &mut Cursor<'a>,
        passed_len: Option<usize>,
    ) -> Self {
        Self {
            value: cur.get_string_value(origin, passed_len),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewAmountTypeNode {
    pub decimals: u8,
    pub unit: Option<String>,
    pub number: ViewNumberTypeNode,
}

impl ViewAmountTypeNode {
    pub fn from<'a>(origin: &AmountTypeNode, cur: &mut Cursor<'a>) -> Self {
        Self {
            decimals: origin.decimals.clone(),
            unit: origin.unit.clone(),
            number: ViewNumberTypeNode::from(origin.number.get_nested_type_node(), cur),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewBooleanTypeNode {
    pub value: bool,
}

impl ViewBooleanTypeNode {
    pub fn from<'a>(origin: &BooleanTypeNode, cur: &mut Cursor<'a>) -> Self {
        let raw = cur.get_number_value(origin.size.get_nested_type_node());
        let value = raw.parse::<i128>().map(|v| v != 0).unwrap_or(false);

        Self { value }
    }

    pub fn from_value(value: bool) -> Self {
        Self { value }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewArrayTypeNode {
    pub values: Vec<ParsedArgValue>,
}

impl ViewArrayTypeNode {
    pub fn from<'a>(
        origin: &ArrayTypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        Self {
            values: cur.get_array_value(origin, defined_types, is_last),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewSetTypeNode {
    pub values: Vec<ParsedArgValue>,
}

impl ViewSetTypeNode {
    pub fn from<'a>(
        origin: &SetTypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        Self {
            values: cur.get_set_value(origin, defined_types, is_last),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewTupleTypeNode {
    pub items: Vec<ParsedArgValue>,
}

impl ViewTupleTypeNode {
    pub fn from<'a>(
        origin: &TupleTypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        let items = origin
            .items
            .iter()
            .map(|item| {
                ParsedArgValue::from(item, cur, defined_types, is_last, None)
                    .expect("Tuple items cannot be residual")
            })
            .collect();

        Self { items }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewBytesTypeNode {
    pub value: String,
}

impl ViewBytesTypeNode {
    pub fn from<'a>(cur: &mut Cursor<'a>, passed_len: Option<usize>) -> Self {
        Self {
            value: cur.get_bytes_value(passed_len),
        }
    }

    pub fn from_value(value: &BytesValueNode) -> Self {
        let final_value = match value.encoding {
            BytesEncoding::Base16 => value.data.clone(),
            BytesEncoding::Base58 => {
                hex::encode(bs58::decode(&value.data).into_vec().ok().unwrap())
            }
            BytesEncoding::Base64 => hex::encode(STANDARD.decode(&value.data).unwrap()),
            BytesEncoding::Utf8 => hex::encode(value.data.as_bytes()),
        };
        Self { value: final_value }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewDateTimeTypeNode {
    pub value: String,
    pub format: NumberFormat,
}

impl ViewDateTimeTypeNode {
    pub fn from<'a>(origin: &DateTimeTypeNode, cur: &mut Cursor<'a>) -> Self {
        let number = origin.number.get_nested_type_node();
        Self {
            value: cur.get_number_value(number),
            format: number.format.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewHiddenPrefixTypeNode {
    pub prefix: Vec<ParsedArgValue>,
    pub value: Box<ParsedArgValue>,
}

impl ViewHiddenPrefixTypeNode {
    pub fn from<'a>(
        origin: &HiddenPrefixTypeNode<TypeNode>,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        let mut prefix = vec![];
        for constant in &origin.prefix {
            prefix.push(
                ParsedArgValue::from(&*constant.r#type, cur, defined_types, is_last, None)
                    .expect("Hidden prefixes cannot be residual"),
            );
        }

        Self {
            prefix,
            value: Box::new(
                ParsedArgValue::from(&*origin.r#type, cur, defined_types, is_last, None)
                    .expect("Hidden prefix values cannot be residual"),
            ),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewHiddenSuffixTypeNode {
    pub suffix: Vec<ParsedArgValue>,
    pub value: Box<ParsedArgValue>,
}

impl ViewHiddenSuffixTypeNode {
    pub fn from<'a>(
        origin: &HiddenSuffixTypeNode<TypeNode>,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        // `HiddenSuffixTypeNode` serializes the wrapped `type` first, then the `suffix` constants.
        // Decode the wrapped value first, then consume the suffix from the input cursor.
        let value = Box::new(
            ParsedArgValue::from(&*origin.r#type, cur, defined_types, is_last, None)
                .expect("Hidden suffix values cannot be residual"),
        );

        let mut suffix = vec![];
        for constant in &origin.suffix {
            suffix.push(
                ParsedArgValue::from(&*constant.r#type, cur, defined_types, is_last, None)
                    .expect("Hidden suffixes cannot be residual"),
            );
        }

        Self { suffix, value }
    }
}

// #[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
// pub struct ViewSentinelTypeNode {
//     pub value: Box<ParsedArgValue>,
//     pub sentinel: ConstantValueNode,
// }

// impl ViewSentinelTypeNode {
//     pub fn from<'a>(origin: &SentinelTypeNode<TypeNode>, cur: &mut Cursor<'a>) -> Self {
//         Self {
//             value: Box::new(
//                 ParsedArgValue::from(&origin.r#type, cur).expect("Sentinel values cannot be residual"),
//             ),
//             sentinel: origin.sentinel.clone(),
//         }
//     }
// }

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewMapTypeNode {
    pub entries: Vec<ViewMapEntryTypeNode>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ViewMapEntryTypeNode {
    pub key: ParsedArgValue,
    pub value: ParsedArgValue,
}

impl ViewMapTypeNode {
    pub fn from<'a>(
        origin: &codama_nodes::MapTypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        let pairs = cur.get_map_value(origin, defined_types, is_last);
        let entries = pairs
            .into_iter()
            .map(|(key, value)| ViewMapEntryTypeNode { key, value })
            .collect();
        Self { entries }
    }
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

impl ViewEnumTypeNode {
    pub fn from<'a>(
        origin: &EnumTypeNode,
        cur: &mut Cursor<'a>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Self {
        let discriminant = cur.get_number_value(origin.size.get_nested_type_node());
        let tag = discriminant.parse::<usize>().unwrap_or_default();

        let variant = origin
            .variants
            .get(tag)
            .or_else(|| {
                origin.variants.iter().find(|v| match v {
                    EnumVariantTypeNode::Empty(ev) => ev.discriminator == Some(tag),
                    EnumVariantTypeNode::Struct(ev) => ev.discriminator == Some(tag),
                    EnumVariantTypeNode::Tuple(ev) => ev.discriminator == Some(tag),
                })
            })
            .expect("Enum discriminant out of bounds");

        match variant {
            EnumVariantTypeNode::Empty(ev) => Self {
                discriminant,
                name: ev.name.to_string(),
                value: ViewEnumValue::Empty,
            },
            EnumVariantTypeNode::Struct(ev) => Self {
                discriminant,
                name: ev.name.to_string(),
                value: ViewEnumValue::Struct(ViewStructTypeNode::from(
                    ev.r#struct.get_nested_type_node(),
                    cur,
                    defined_types,
                    is_last,
                )),
            },
            EnumVariantTypeNode::Tuple(ev) => Self {
                discriminant,
                name: ev.name.to_string(),
                value: ViewEnumValue::Tuple(ViewTupleTypeNode::from(
                    ev.tuple.get_nested_type_node(),
                    cur,
                    defined_types,
                    is_last,
                )),
            },
        }
    }

    // Sentinel-related
    // pub fn from_value(value: &EnumValueNode) -> Self {
    // }
}
