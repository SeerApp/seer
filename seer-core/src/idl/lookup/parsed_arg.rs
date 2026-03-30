use core::panic;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use codama_nodes::{
    AmountTypeNode, ArrayTypeNode, BooleanTypeNode, BytesEncoding, BytesValueNode, CamelCaseString,
    DateTimeTypeNode, Docs, EnumTypeNode, EnumVariantTypeNode, HiddenPrefixTypeNode,
    HiddenSuffixTypeNode, NestedTypeNodeTrait, NumberFormat, NumberTypeNode, OptionTypeNode,
    SetTypeNode, StringTypeNode, StructTypeNode, TupleTypeNode, TypeNode,
};
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;

use crate::idl::lookup::cursor::Cursor;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum ParsedArg {
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

impl ParsedArg {
    pub fn from<'a>(origin: &TypeNode, cur: &mut Cursor<'a>) -> Option<Self> {
        match origin {
            TypeNode::Amount(a) => Some(ParsedArg::Amount(ViewAmountTypeNode::from(a, cur))),
            TypeNode::Array(a) => Some(ParsedArg::Array(ViewArrayTypeNode::from(a, cur))),
            TypeNode::Boolean(b) => Some(ParsedArg::Boolean(ViewBooleanTypeNode::from(b, cur))),
            TypeNode::Bytes(_) => Some(ParsedArg::Bytes(ViewBytesTypeNode::from(cur))),
            TypeNode::DateTime(d) => Some(ParsedArg::DateTime(ViewDateTimeTypeNode::from(d, cur))),
            TypeNode::Enum(e) => Some(ParsedArg::Enum(ViewEnumTypeNode::from(e, cur))),
            TypeNode::HiddenPrefix(h) => Some(ParsedArg::HiddenPrefix(
                ViewHiddenPrefixTypeNode::from(h, cur),
            )),
            TypeNode::HiddenSuffix(h) => Some(ParsedArg::HiddenSuffix(
                ViewHiddenSuffixTypeNode::from(h, cur),
            )),
            TypeNode::Map(m) => Some(ParsedArg::Map(ViewMapTypeNode::from(m, cur))),
            TypeNode::Number(n) => Some(ParsedArg::Number(ViewNumberTypeNode::from(n, cur))),
            TypeNode::Struct(s) => Some(ParsedArg::Struct(ViewStructTypeNode::from(s, cur))),
            TypeNode::Option(o) => Some(ParsedArg::Option(ViewOptionTypeNode::from(o, cur))),
            TypeNode::PublicKey(_) => Some(ParsedArg::PublicKey(ViewPublicKeyTypeNode::from(cur))),
            TypeNode::Tuple(t) => Some(ParsedArg::Tuple(ViewTupleTypeNode::from(t, cur))),
            TypeNode::String(s) => Some(ParsedArg::String(ViewStringTypeNode::from(s, cur))),
            TypeNode::FixedSize(f) => cur.get_fixed_size_value(f),
            TypeNode::PostOffset(p) => cur.get_post_offset_value(p),
            TypeNode::PreOffset(p) => cur.get_pre_offset_value(p),
            TypeNode::RemainderOption(r) if !cur.is_empty() => ParsedArg::from(&r.item, cur),
            TypeNode::Sentinel(_) => panic!("Debugger not yet equipped for Sentinel IDL layouts"),
            TypeNode::Set(s) => Some(ParsedArg::Set(ViewSetTypeNode::from(s, cur))),
            // This implies a significant change to how I currently manage dynamic data structures. 
            TypeNode::SizePrefix(s) => {}
            _ => panic!("Unsupported TypeNode variant for ParsedArg view"),
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
    //             ParsedArg::Boolean(ViewBooleanTypeNode::from_value(v.boolean))
    //         }
    //         ValueNode::Bytes(v) => {
    //             let v = v as &BytesValueNode;
    //             ParsedArg::Bytes(ViewBytesTypeNode::from_value(v))
    //         }
    //         ValueNode::Constant(v) => {
    //             let v = v as &ConstantValueNode;
    //             ParsedArg::from_value(&v.value)
    //         }
    //         ValueNode::Enum(v) => {
    //             let v = v as &EnumValueNode;
    //             ParsedArg::Enum(ViewEnumTypeNode::from_value(v))
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewStructTypeNode {
    pub fields: Vec<ViewStructFieldTypeNode>,
}

impl ViewStructTypeNode {
    pub fn from<'a>(origin: &StructTypeNode, cur: &mut Cursor<'a>) -> Self {
        let mut fields = vec![];

        for field in &origin.fields {
            let view_field: ViewStructFieldTypeNode = ViewStructFieldTypeNode {
                name: field.name.clone(),
                docs: field.docs.clone(),
                value: ParsedArg::from(&field.r#type, cur)
                    .expect("Struct field cannot be residual"),
            };

            fields.push(view_field);
        }

        Self { fields }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewStructFieldTypeNode {
    pub name: CamelCaseString,
    pub docs: Docs,

    pub value: ParsedArg,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewOptionTypeNode {
    pub value: Box<Option<ParsedArg>>,
}

impl ViewOptionTypeNode {
    pub fn from<'a>(origin: &OptionTypeNode, cur: &mut Cursor<'a>) -> Self {
        Self {
            value: Box::new(cur.get_option_value(origin)),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewPublicKeyTypeNode {
    pub value: Pubkey,
}

impl ViewPublicKeyTypeNode {
    pub fn from<'a>(cur: &mut Cursor<'a>) -> Self {
        Self {
            value: cur.get_pubkey_value(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewStringTypeNode {
    pub value: String,
}

impl ViewStringTypeNode {
    pub fn from<'a>(origin: &StringTypeNode, cur: &mut Cursor<'a>) -> Self {
        Self {
            value: cur.get_string_value(origin),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewArrayTypeNode {
    pub values: Vec<ParsedArg>,
}

impl ViewArrayTypeNode {
    pub fn from<'a>(origin: &ArrayTypeNode, cur: &mut Cursor<'a>) -> Self {
        Self {
            values: cur.get_array_value(origin),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewSetTypeNode {
    pub values: Vec<ParsedArg>,
}

impl ViewSetTypeNode {
    pub fn from<'a>(origin: &SetTypeNode, cur: &mut Cursor<'a>) -> Self {
        Self {
            values: cur.get_set_value(origin),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewTupleTypeNode {
    pub items: Vec<ParsedArg>,
}

impl ViewTupleTypeNode {
    pub fn from<'a>(origin: &TupleTypeNode, cur: &mut Cursor<'a>) -> Self {
        let items = origin
            .items
            .iter()
            .map(|item| ParsedArg::from(item, cur).expect("Tuple items cannot be residual"))
            .collect();

        Self { items }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewBytesTypeNode {
    pub value: String,
}

impl ViewBytesTypeNode {
    pub fn from<'a>(cur: &mut Cursor<'a>) -> Self {
        Self {
            value: cur.get_bytes_value(),
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewHiddenPrefixTypeNode {
    pub prefix: Vec<ParsedArg>,
    pub value: Box<ParsedArg>,
}

impl ViewHiddenPrefixTypeNode {
    pub fn from<'a>(origin: &HiddenPrefixTypeNode<TypeNode>, cur: &mut Cursor<'a>) -> Self {
        let mut prefix = vec![];
        for constant in &origin.prefix {
            prefix.push(
                ParsedArg::from(&*constant.r#type, cur)
                    .expect("Hidden prefixes cannot be residual"),
            );
        }

        Self {
            prefix,
            value: Box::new(
                ParsedArg::from(&*origin.r#type, cur)
                    .expect("Hidden prefix values cannot be residual"),
            ),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewHiddenSuffixTypeNode {
    pub suffix: Vec<ParsedArg>,
    pub value: Box<ParsedArg>,
}

impl ViewHiddenSuffixTypeNode {
    pub fn from<'a>(origin: &HiddenSuffixTypeNode<TypeNode>, cur: &mut Cursor<'a>) -> Self {
        // `HiddenSuffixTypeNode` serializes the wrapped `type` first, then the `suffix` constants.
        // Decode the wrapped value first, then consume the suffix from the input cursor.
        let value = Box::new(
            ParsedArg::from(&*origin.r#type, cur).expect("Hidden suffix values cannot be residual"),
        );

        let mut suffix = vec![];
        for constant in &origin.suffix {
            suffix.push(
                ParsedArg::from(&*constant.r#type, cur)
                    .expect("Hidden suffixes cannot be residual"),
            );
        }

        Self { suffix, value }
    }
}

// #[derive(Serialize, Deserialize, Clone, PartialEq)]
// pub struct ViewSentinelTypeNode {
//     pub value: Box<ParsedArg>,
//     pub sentinel: ConstantValueNode,
// }

// impl ViewSentinelTypeNode {
//     pub fn from<'a>(origin: &SentinelTypeNode<TypeNode>, cur: &mut Cursor<'a>) -> Self {
//         Self {
//             value: Box::new(
//                 ParsedArg::from(&origin.r#type, cur).expect("Sentinel values cannot be residual"),
//             ),
//             sentinel: origin.sentinel.clone(),
//         }
//     }
// }

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewMapTypeNode {
    pub entries: Vec<ViewMapEntryTypeNode>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewMapEntryTypeNode {
    pub key: ParsedArg,
    pub value: ParsedArg,
}

impl ViewMapTypeNode {
    pub fn from<'a>(origin: &codama_nodes::MapTypeNode, cur: &mut Cursor<'a>) -> Self {
        let pairs = cur.get_map_value(origin);
        let entries = pairs
            .into_iter()
            .map(|(key, value)| ViewMapEntryTypeNode { key, value })
            .collect();
        Self { entries }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewEnumTypeNode {
    pub discriminant: String,
    pub name: String,
    pub value: ViewEnumValue,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum ViewEnumValue {
    Empty,
    Tuple(ViewTupleTypeNode),
    Struct(ViewStructTypeNode),
}

impl ViewEnumTypeNode {
    pub fn from<'a>(origin: &EnumTypeNode, cur: &mut Cursor<'a>) -> Self {
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
                )),
            },
            EnumVariantTypeNode::Tuple(ev) => Self {
                discriminant,
                name: ev.name.to_string(),
                value: ViewEnumValue::Tuple(ViewTupleTypeNode::from(
                    ev.tuple.get_nested_type_node(),
                    cur,
                )),
            },
        }
    }

    // Sentinel-related
    // pub fn from_value(value: &EnumValueNode) -> Self {
    // }
}
