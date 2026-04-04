use core::panic;

use crate::idl::{
    cursor::Cursor,
    parsed_arg::{
        ParsedArgValue, ViewAmountTypeNode, ViewArrayTypeNode, ViewBooleanTypeNode,
        ViewBytesTypeNode, ViewDateTimeTypeNode, ViewEnumTypeNode, ViewEnumValue,
        ViewHiddenPrefixTypeNode, ViewHiddenSuffixTypeNode, ViewMapEntryTypeNode, ViewMapTypeNode,
        ViewNumberTypeNode, ViewOptionTypeNode, ViewPublicKeyTypeNode, ViewSetTypeNode,
        ViewStringTypeNode, ViewStructFieldTypeNode, ViewStructTypeNode, ViewTupleTypeNode,
    },
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use codama_nodes::{
    AmountTypeNode, ArrayTypeNode, BooleanTypeNode, BytesEncoding, BytesValueNode,
    DateTimeTypeNode, DefinedTypeNode, EnumTypeNode, EnumVariantTypeNode, HiddenPrefixTypeNode,
    HiddenSuffixTypeNode, InstructionInputValueNode, MapTypeNode, NestedTypeNodeTrait, Number,
    NumberFormat, NumberTypeNode, NumberValueNode, OptionTypeNode, SetTypeNode, StringTypeNode,
    StructTypeNode, TupleTypeNode, TypeNode, ValueNode,
};
use crate::idl::codama::cursor::CodamaCursor;

pub fn get_parsed_arg_value<'a>(
    origin: &TypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
    passed_len: Option<usize>,
) -> Option<ParsedArgValue> {
    match origin {
        TypeNode::Link(link) => {
            let resolved = defined_types
                .iter()
                .find(|dt| dt.name == link.name)
                .unwrap_or_else(|| {
                    panic!("defined type not found for link {:?}", link.name);
                });
            get_parsed_arg_value(&resolved.r#type, cur, defined_types, is_last, passed_len)
        }
        TypeNode::Amount(a) => Some(ParsedArgValue::Amount(get_view_amount_type_node(a, cur))),
        TypeNode::Array(a) => Some(ParsedArgValue::Array(get_view_array_type_node(
            a,
            cur,
            defined_types,
            is_last,
        ))),
        TypeNode::Boolean(b) => Some(ParsedArgValue::Boolean(get_view_boolean_type_node(b, cur))),
        TypeNode::Bytes(_) => {
            if !is_last && passed_len.is_none() {
                panic!("No passed length detected for non-final argument");
            }
            Some(ParsedArgValue::Bytes(get_view_bytes_type_node(
                cur, passed_len,
            )))
        }
        TypeNode::DateTime(d) => Some(ParsedArgValue::DateTime(get_view_date_time_type_node(
            d, cur,
        ))),
        TypeNode::Enum(e) => Some(ParsedArgValue::Enum(get_view_enum_type_node(
            e,
            cur,
            defined_types,
            is_last,
        ))),
        TypeNode::HiddenPrefix(h) => Some(ParsedArgValue::HiddenPrefix(
            get_view_hidden_prefix_type_node(h, cur, defined_types, is_last),
        )),
        TypeNode::HiddenSuffix(h) => Some(ParsedArgValue::HiddenSuffix(
            get_view_hidden_suffix_type_node(h, cur, defined_types, is_last),
        )),
        TypeNode::Map(m) => Some(ParsedArgValue::Map(get_view_map_type_node(
            m,
            cur,
            defined_types,
            is_last,
        ))),
        TypeNode::Number(n) => Some(ParsedArgValue::Number(get_view_number_type_node(n, cur))),
        TypeNode::Struct(s) => Some(ParsedArgValue::Struct(get_view_struct_type_node(
            s,
            cur,
            defined_types,
            is_last,
        ))),
        TypeNode::Option(o) => Some(ParsedArgValue::Option(get_view_option_type_node(
            o,
            cur,
            defined_types,
            is_last,
        ))),
        TypeNode::PublicKey(_) => Some(ParsedArgValue::PublicKey(get_view_public_key_type_node(
            cur,
        ))),
        TypeNode::Tuple(t) => Some(ParsedArgValue::Tuple(get_view_tuple_type_node(
            t,
            cur,
            defined_types,
            is_last,
        ))),
        TypeNode::String(s) => {
            if !is_last && passed_len.is_none() {
                panic!("No passed length detected for non-final argument");
            }
            Some(ParsedArgValue::String(get_view_string_type_node(
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
                get_parsed_arg_value(&r.item, cur, defined_types, is_last, passed_len)
            } else {
                None
            }
        }
        TypeNode::Sentinel(_) => panic!("Debugger not yet equipped for Sentinel IDL layouts"),
        TypeNode::Set(s) => Some(ParsedArgValue::Set(get_view_set_type_node(
            s,
            cur,
            defined_types,
            is_last,
        ))),
        TypeNode::SizePrefix(s) => cur.get_dynamic_value(s, defined_types, is_last),
        TypeNode::SolAmount(s) => Some(ParsedArgValue::Number(get_view_number_type_node(
            s.number.get_nested_type_node(),
            cur,
        ))),
        TypeNode::ZeroableOption(_) => {
            panic!("Debugger not yet equipped for ZeroableOption IDL layouts")
        }
    }
}

pub fn eq_value_node(parsed_arg_value: &ParsedArgValue, value: &ValueNode) -> bool {
    match (parsed_arg_value, value) {
        (ParsedArgValue::Number(arg), ValueNode::Number(node)) => {
            let node: &NumberValueNode = node;
            eq_number_value(arg, node.number)
        }
        (ParsedArgValue::Bytes(arg), ValueNode::Bytes(node)) => {
            let node: &BytesValueNode = node;
            arg.value == get_view_bytes_type_node_from_value(node).value
        }
        _ => false,
    }
}

pub fn eq_instruciton_input_value_node(
    parsed_arg_value: &ParsedArgValue,
    value: &InstructionInputValueNode,
) -> bool {
    match (parsed_arg_value, value) {
        (ParsedArgValue::Number(arg), InstructionInputValueNode::Number(node)) => {
            let node: &NumberValueNode = node;
            eq_number_value(arg, node.number)
        }
        (ParsedArgValue::Bytes(arg), InstructionInputValueNode::Bytes(node)) => {
            let node: &BytesValueNode = node;
            arg.value == get_view_bytes_type_node_from_value(node).value
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

pub fn get_view_number_type_node<'a>(
    origin: &NumberTypeNode,
    cur: &mut Cursor<'a>,
) -> ViewNumberTypeNode {
    ViewNumberTypeNode {
        value: cur.get_number_value(origin),
        format: origin.format.clone(),
    }
}

pub fn get_view_struct_type_node<'a>(
    origin: &StructTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewStructTypeNode {
    let mut fields = vec![];

    for field in &origin.fields {
        let view_field: ViewStructFieldTypeNode = ViewStructFieldTypeNode {
            name: String::from(field.name.clone()),
            docs: field.docs.clone(),
            value: get_parsed_arg_value(&field.r#type, cur, defined_types, is_last, None)
                .expect("Struct field cannot be residual"),
        };

        fields.push(view_field);
    }

    ViewStructTypeNode { fields }
}

pub fn get_view_option_type_node<'a>(
    origin: &OptionTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewOptionTypeNode {
    ViewOptionTypeNode {
        value: Box::new(cur.get_option_value(origin, defined_types, is_last)),
    }
}

pub fn get_view_public_key_type_node<'a>(cur: &mut Cursor<'a>) -> ViewPublicKeyTypeNode {
    ViewPublicKeyTypeNode {
        value: cur.get_pubkey_value(),
    }
}

pub fn get_view_string_type_node<'a>(
    origin: &StringTypeNode,
    cur: &mut Cursor<'a>,
    passed_len: Option<usize>,
) -> ViewStringTypeNode {
    ViewStringTypeNode {
        value: cur.get_string_value(origin, passed_len),
    }
}

pub fn get_view_amount_type_node<'a>(
    origin: &AmountTypeNode,
    cur: &mut Cursor<'a>,
) -> ViewAmountTypeNode {
    ViewAmountTypeNode {
        decimals: origin.decimals.clone(),
        unit: origin.unit.clone(),
        number: get_view_number_type_node(origin.number.get_nested_type_node(), cur),
    }
}

pub fn get_view_boolean_type_node<'a>(
    origin: &BooleanTypeNode,
    cur: &mut Cursor<'a>,
) -> ViewBooleanTypeNode {
    let raw = cur.get_number_value(origin.size.get_nested_type_node());
    let value = raw.parse::<i128>().map(|v| v != 0).unwrap_or(false);

    ViewBooleanTypeNode { value }
}

// pub fn get_view_boolean_type_node_from_value(value: bool) -> ViewBooleanTypeNode {
//     ViewBooleanTypeNode { value }
// }

pub fn get_view_array_type_node<'a>(
    origin: &ArrayTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewArrayTypeNode {
    ViewArrayTypeNode {
        values: cur.get_array_value(origin, defined_types, is_last),
    }
}

pub fn get_view_set_type_node<'a>(
    origin: &SetTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewSetTypeNode {
    ViewSetTypeNode {
        values: cur.get_set_value(origin, defined_types, is_last),
    }
}

pub fn get_view_tuple_type_node<'a>(
    origin: &TupleTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewTupleTypeNode {
    let items = origin
        .items
        .iter()
        .map(|item| {
            get_parsed_arg_value(item, cur, defined_types, is_last, None)
                .expect("Tuple items cannot be residual")
        })
        .collect();

    ViewTupleTypeNode { items }
}

pub fn get_view_bytes_type_node<'a>(
    cur: &mut Cursor<'a>,
    passed_len: Option<usize>,
) -> ViewBytesTypeNode {
    ViewBytesTypeNode {
        value: cur.get_bytes_value(passed_len),
    }
}

pub fn get_view_bytes_type_node_from_value<'a>(value: &BytesValueNode) -> ViewBytesTypeNode {
    let final_value = match value.encoding {
        BytesEncoding::Base16 => value.data.clone(),
        BytesEncoding::Base58 => hex::encode(bs58::decode(&value.data).into_vec().ok().unwrap()),
        BytesEncoding::Base64 => hex::encode(STANDARD.decode(&value.data).unwrap()),
        BytesEncoding::Utf8 => hex::encode(value.data.as_bytes()),
    };
    ViewBytesTypeNode { value: final_value }
}

pub fn get_view_date_time_type_node<'a>(
    origin: &DateTimeTypeNode,
    cur: &mut Cursor<'a>,
) -> ViewDateTimeTypeNode {
    let number = origin.number.get_nested_type_node();
    ViewDateTimeTypeNode {
        value: cur.get_number_value(number),
        format: number.format.clone(),
    }
}

pub fn get_view_hidden_prefix_type_node<'a>(
    origin: &HiddenPrefixTypeNode<TypeNode>,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewHiddenPrefixTypeNode {
    let mut prefix = vec![];
    for constant in &origin.prefix {
        prefix.push(
            get_parsed_arg_value(&*constant.r#type, cur, defined_types, is_last, None)
                .expect("Hidden prefixes cannot be residual"),
        );
    }
    ViewHiddenPrefixTypeNode {
        prefix,
        value: Box::new(
            get_parsed_arg_value(&*origin.r#type, cur, defined_types, is_last, None)
                .expect("Hidden prefix values cannot be residual"),
        ),
    }
}

pub fn get_view_hidden_suffix_type_node<'a>(
    origin: &HiddenSuffixTypeNode<TypeNode>,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewHiddenSuffixTypeNode {
    let value = Box::new(
        get_parsed_arg_value(&*origin.r#type, cur, defined_types, is_last, None)
            .expect("Hidden suffix values cannot be residual"),
    );
    let mut suffix = vec![];

    for constant in &origin.suffix {
        suffix.push(
            get_parsed_arg_value(&*constant.r#type, cur, defined_types, is_last, None)
                .expect("Hidden suffixes cannot be residual"),
        );
    }
    ViewHiddenSuffixTypeNode { suffix, value }
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

pub fn get_view_map_type_node<'a>(
    origin: &MapTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewMapTypeNode {
    let pairs = cur.get_map_value(origin, defined_types, is_last);
    let entries = pairs
        .into_iter()
        .map(|(key, value)| ViewMapEntryTypeNode { key, value })
        .collect();
    ViewMapTypeNode { entries }
}

pub fn get_view_enum_type_node<'a>(
    origin: &EnumTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> ViewEnumTypeNode {
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
        EnumVariantTypeNode::Empty(ev) => ViewEnumTypeNode {
            discriminant,
            name: ev.name.to_string(),
            value: ViewEnumValue::Empty,
        },
        EnumVariantTypeNode::Struct(ev) => ViewEnumTypeNode {
            discriminant,
            name: ev.name.to_string(),
            value: ViewEnumValue::Struct(get_view_struct_type_node(
                ev.r#struct.get_nested_type_node(),
                cur,
                defined_types,
                is_last,
            )),
        },
        EnumVariantTypeNode::Tuple(ev) => ViewEnumTypeNode {
            discriminant,
            name: ev.name.to_string(),
            value: ViewEnumValue::Tuple(get_view_tuple_type_node(
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
