use crate::idl::{
    codama::cursor::CodamaCursor,
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
    NumberFormat, NumberTypeNode, NumberValueNode, OptionTypeNode, SentinelTypeNode, SetTypeNode,
    StringTypeNode, StructTypeNode, TupleTypeNode, TypeNode, ValueNode, ZeroableOptionTypeNode,
};
use solana_pubkey::Pubkey;

pub fn get_parsed_arg_value(
    origin: &TypeNode,
    cur: &mut Cursor<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
    passed_len: Option<usize>,
) -> Option<ParsedArgValue> {
    match origin {
        TypeNode::Link(link) => {
            let Some(resolved) = defined_types.iter().find(|dt| dt.name == link.name) else {
                crate::seer_warn!(
                    "Codama decode: unresolved defined type link '{:?}'",
                    link.name.as_ref()
                );
                return None;
            };
            get_parsed_arg_value(&resolved.r#type, cur, defined_types, is_last, passed_len)
        }
        TypeNode::Amount(a) => Some(ParsedArgValue::Amount(get_view_amount_type_node(a, cur)?)),
        TypeNode::Array(a) => Some(ParsedArgValue::Array(get_view_array_type_node(
            a,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::Boolean(b) => Some(ParsedArgValue::Boolean(get_view_boolean_type_node(b, cur)?)),
        TypeNode::Bytes(_) => {
            if !is_last && passed_len.is_none() {
                crate::seer_warn!(
                    "Codama decode: bytes/string without explicit length in non-last position"
                );
                return None;
            }
            Some(ParsedArgValue::Bytes(get_view_bytes_type_node(
                cur, passed_len,
            )?))
        }
        TypeNode::DateTime(d) => Some(ParsedArgValue::DateTime(get_view_date_time_type_node(
            d, cur,
        )?)),
        TypeNode::Enum(e) => Some(ParsedArgValue::Enum(get_view_enum_type_node(
            e,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::HiddenPrefix(h) => Some(ParsedArgValue::HiddenPrefix(
            get_view_hidden_prefix_type_node(h, cur, defined_types, is_last)?,
        )),
        TypeNode::HiddenSuffix(h) => Some(ParsedArgValue::HiddenSuffix(
            get_view_hidden_suffix_type_node(h, cur, defined_types, is_last)?,
        )),
        TypeNode::Map(m) => Some(ParsedArgValue::Map(get_view_map_type_node(
            m,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::Number(n) => Some(ParsedArgValue::Number(get_view_number_type_node(n, cur)?)),
        TypeNode::Struct(s) => Some(ParsedArgValue::Struct(get_view_struct_type_node(
            s,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::Option(o) => Some(ParsedArgValue::Option(get_view_option_type_node(
            o,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::PublicKey(_) => Some(ParsedArgValue::PublicKey(get_view_public_key_type_node(
            cur,
        )?)),
        TypeNode::Tuple(t) => Some(ParsedArgValue::Tuple(get_view_tuple_type_node(
            t,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::String(s) => {
            if !is_last && passed_len.is_none() {
                crate::seer_warn!(
                    "Codama decode: bytes/string without explicit length in non-last position"
                );
                return None;
            }
            Some(ParsedArgValue::String(get_view_string_type_node(
                s, cur, passed_len,
            )?))
        }
        TypeNode::FixedSize(f) => cur.get_fixed_size_value(f, defined_types),
        TypeNode::PostOffset(p) => cur.get_post_offset_value(p, defined_types, is_last),
        TypeNode::PreOffset(p) => cur.get_pre_offset_value(p, defined_types, is_last),
        TypeNode::RemainderOption(r) => {
            if !is_last {
                crate::seer_warn!("Codama decode: remainder option found outside final position");
                return None;
            }
            if !cur.is_empty() {
                let value = get_parsed_arg_value(&r.item, cur, defined_types, is_last, passed_len)?;
                Some(ParsedArgValue::Option(ViewOptionTypeNode {
                    value: Box::new(Some(value)),
                }))
            } else {
                Some(ParsedArgValue::Option(ViewOptionTypeNode {
                    value: Box::new(None),
                }))
            }
        }
        TypeNode::Sentinel(s) => Some(ParsedArgValue::Option(get_view_sentinel_type_node(
            s,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::Set(s) => Some(ParsedArgValue::Set(get_view_set_type_node(
            s,
            cur,
            defined_types,
            is_last,
        )?)),
        TypeNode::SizePrefix(s) => cur.get_dynamic_value(s, defined_types, is_last),
        TypeNode::SolAmount(s) => Some(ParsedArgValue::Number(get_view_number_type_node(
            s.number.get_nested_type_node(),
            cur,
        )?)),
        TypeNode::ZeroableOption(z) => Some(ParsedArgValue::Option(
            get_view_zeroable_option_type_node(z, cur, defined_types, is_last)?,
        )),
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
            let Some(expected) = get_view_bytes_type_node_from_value(node) else {
                return false;
            };
            arg.value == expected.value
        }
        (ParsedArgValue::PublicKey(arg), ValueNode::PublicKey(node)) => {
            node.public_key.parse::<Pubkey>().ok() == Some(arg.value)
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
            let Some(expected) = get_view_bytes_type_node_from_value(node) else {
                return false;
            };
            arg.value == expected.value
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
        ) => arg.value.parse::<u64>().ok() == Some(node_number),
        (
            NumberFormat::I8
            | NumberFormat::I16
            | NumberFormat::I32
            | NumberFormat::I64
            | NumberFormat::I128,
            Number::SignedInteger(node_number),
        ) => arg.value.parse::<i64>().ok() == Some(node_number),
        (NumberFormat::F32 | NumberFormat::F64, Number::Float(node_number)) => {
            arg.value.parse::<f64>().ok().map(|v| v == node_number) == Some(true)
        }
        _ => false,
    }
}

pub fn get_view_number_type_node<'a>(
    origin: &NumberTypeNode,
    cur: &mut Cursor<'a>,
) -> Option<ViewNumberTypeNode> {
    Some(ViewNumberTypeNode {
        value: cur.get_number_value(origin)?,
        format: origin.format,
    })
}

pub fn get_view_struct_type_node<'a>(
    origin: &StructTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewStructTypeNode> {
    let mut fields = vec![];
    let field_count = origin.fields.len();

    for (idx, field) in origin.fields.iter().enumerate() {
        let field_byte_offset = cur.absolute_pos();
        let field_is_last = is_last && idx.saturating_add(1) == field_count;
        let value = get_parsed_arg_value(&field.r#type, cur, defined_types, field_is_last, None);
        let Some(value) = value else {
            crate::seer_warn!(
                "Codama decode: failed to decode struct field '{}'",
                field.name.as_ref()
            );
            return None;
        };
        fields.push(ViewStructFieldTypeNode {
            name: String::from(field.name.clone()),
            docs: field.docs.clone(),
            byte_offset: Some(field_byte_offset),
            value,
        });
    }

    Some(ViewStructTypeNode { fields })
}

pub fn get_view_option_type_node<'a>(
    origin: &OptionTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewOptionTypeNode> {
    Some(ViewOptionTypeNode {
        value: Box::new(cur.get_option_value(origin, defined_types, is_last)?),
    })
}

pub fn get_view_zeroable_option_type_node<'a>(
    origin: &ZeroableOptionTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewOptionTypeNode> {
    let parsed = get_parsed_arg_value(&origin.item, cur, defined_types, is_last, None)?;
    let is_none = match origin.zero_value.as_ref() {
        Some(zero_value) => eq_value_node(&parsed, &zero_value.value),
        None => is_default_zero_value(&parsed),
    };
    Some(ViewOptionTypeNode {
        value: Box::new(if is_none { None } else { Some(parsed) }),
    })
}

pub fn get_view_sentinel_type_node<'a>(
    origin: &SentinelTypeNode<TypeNode>,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewOptionTypeNode> {
    let parsed = get_parsed_arg_value(&origin.r#type, cur, defined_types, is_last, None)?;
    let is_none = eq_value_node(&parsed, &origin.sentinel.value);
    Some(ViewOptionTypeNode {
        value: Box::new(if is_none { None } else { Some(parsed) }),
    })
}

fn is_default_zero_value(value: &ParsedArgValue) -> bool {
    match value {
        ParsedArgValue::Number(v) => match v.format {
            NumberFormat::F32 | NumberFormat::F64 => v.value.parse::<f64>().ok() == Some(0.0),
            NumberFormat::I8
            | NumberFormat::I16
            | NumberFormat::I32
            | NumberFormat::I64
            | NumberFormat::I128 => v.value.parse::<i128>().ok() == Some(0),
            _ => v.value.parse::<u128>().ok() == Some(0),
        },
        ParsedArgValue::PublicKey(v) => v.value == Pubkey::default(),
        ParsedArgValue::Boolean(v) => !v.value,
        ParsedArgValue::Bytes(v) => !v.value.is_empty() && v.value.chars().all(|c| c == '0'),
        ParsedArgValue::String(v) => v.value.is_empty(),
        _ => false,
    }
}

pub fn get_view_public_key_type_node<'a>(cur: &mut Cursor<'a>) -> Option<ViewPublicKeyTypeNode> {
    Some(ViewPublicKeyTypeNode {
        value: cur.get_pubkey_value()?,
    })
}

pub fn get_view_string_type_node<'a>(
    origin: &StringTypeNode,
    cur: &mut Cursor<'a>,
    passed_len: Option<usize>,
) -> Option<ViewStringTypeNode> {
    Some(ViewStringTypeNode {
        value: cur.get_string_value(origin, passed_len)?,
    })
}

pub fn get_view_amount_type_node<'a>(
    origin: &AmountTypeNode,
    cur: &mut Cursor<'a>,
) -> Option<ViewAmountTypeNode> {
    Some(ViewAmountTypeNode {
        decimals: origin.decimals,
        unit: origin.unit.clone(),
        number: get_view_number_type_node(origin.number.get_nested_type_node(), cur)?,
    })
}

pub fn get_view_boolean_type_node<'a>(
    origin: &BooleanTypeNode,
    cur: &mut Cursor<'a>,
) -> Option<ViewBooleanTypeNode> {
    let raw = cur.get_number_value(origin.size.get_nested_type_node())?;
    let value = raw.parse::<i128>().map(|v| v != 0).unwrap_or(false);

    Some(ViewBooleanTypeNode { value })
}

pub fn get_view_array_type_node<'a>(
    origin: &ArrayTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewArrayTypeNode> {
    Some(ViewArrayTypeNode {
        values: cur.get_array_value(origin, defined_types, is_last)?,
    })
}

pub fn get_view_set_type_node<'a>(
    origin: &SetTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewSetTypeNode> {
    Some(ViewSetTypeNode {
        values: cur.get_set_value(origin, defined_types, is_last)?,
    })
}

pub fn get_view_tuple_type_node<'a>(
    origin: &TupleTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewTupleTypeNode> {
    let mut items = vec![];
    for (i, item) in origin.items.iter().enumerate() {
        let v = get_parsed_arg_value(item, cur, defined_types, is_last, None);
        let Some(v) = v else {
            crate::seer_warn!("Codama decode: failed to decode tuple item index {}", i);
            return None;
        };
        items.push(v);
    }
    Some(ViewTupleTypeNode { items })
}

pub fn get_view_bytes_type_node<'a>(
    cur: &mut Cursor<'a>,
    passed_len: Option<usize>,
) -> Option<ViewBytesTypeNode> {
    Some(ViewBytesTypeNode {
        value: cur.get_bytes_value(passed_len)?,
    })
}

pub fn get_view_bytes_type_node_from_value(value: &BytesValueNode) -> Option<ViewBytesTypeNode> {
    let final_value = match value.encoding {
        BytesEncoding::Base16 => value.data.clone(),
        BytesEncoding::Base58 => {
            let Some(vec) = bs58::decode(&value.data).into_vec().ok() else {
                crate::seer_warn!("Codama decode: failed decoding bytes literal as base58");
                return None;
            };
            hex::encode(vec)
        }
        BytesEncoding::Base64 => {
            let Ok(bytes) = STANDARD.decode(&value.data) else {
                crate::seer_warn!("Codama decode: failed decoding bytes literal as base64");
                return None;
            };
            hex::encode(bytes)
        }
        BytesEncoding::Utf8 => hex::encode(value.data.as_bytes()),
    };
    Some(ViewBytesTypeNode { value: final_value })
}

pub fn get_view_date_time_type_node<'a>(
    origin: &DateTimeTypeNode,
    cur: &mut Cursor<'a>,
) -> Option<ViewDateTimeTypeNode> {
    let number = origin.number.get_nested_type_node();
    Some(ViewDateTimeTypeNode {
        value: cur.get_number_value(number)?,
        format: number.format,
    })
}

pub fn get_view_hidden_prefix_type_node<'a>(
    origin: &HiddenPrefixTypeNode<TypeNode>,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewHiddenPrefixTypeNode> {
    let mut prefix = vec![];
    for constant in &origin.prefix {
        let v = get_parsed_arg_value(&constant.r#type, cur, defined_types, is_last, None)?;
        prefix.push(v);
    }
    let value = Box::new(get_parsed_arg_value(
        &origin.r#type,
        cur,
        defined_types,
        is_last,
        None,
    )?);
    Some(ViewHiddenPrefixTypeNode { prefix, value })
}

pub fn get_view_hidden_suffix_type_node<'a>(
    origin: &HiddenSuffixTypeNode<TypeNode>,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewHiddenSuffixTypeNode> {
    let value = Box::new(get_parsed_arg_value(
        &origin.r#type,
        cur,
        defined_types,
        is_last,
        None,
    )?);
    let mut suffix = vec![];

    for constant in &origin.suffix {
        let v = get_parsed_arg_value(&constant.r#type, cur, defined_types, is_last, None)?;
        suffix.push(v);
    }
    Some(ViewHiddenSuffixTypeNode { suffix, value })
}

pub fn get_view_map_type_node<'a>(
    origin: &MapTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewMapTypeNode> {
    let pairs = cur.get_map_value(origin, defined_types, is_last)?;
    let entries = pairs
        .into_iter()
        .map(|(key, value)| ViewMapEntryTypeNode { key, value })
        .collect();
    Some(ViewMapTypeNode { entries })
}

pub fn get_view_enum_type_node<'a>(
    origin: &EnumTypeNode,
    cur: &mut Cursor<'a>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewEnumTypeNode> {
    let discriminant = cur.get_number_value(origin.size.get_nested_type_node())?;
    let tag = discriminant.parse::<usize>().unwrap_or(usize::MAX);

    let variant = origin.variants.get(tag).or_else(|| {
        origin.variants.iter().find(|v| match v {
            EnumVariantTypeNode::Empty(ev) => ev.discriminator == Some(tag),
            EnumVariantTypeNode::Struct(ev) => ev.discriminator == Some(tag),
            EnumVariantTypeNode::Tuple(ev) => ev.discriminator == Some(tag),
        })
    });

    let Some(variant) = variant else {
        crate::seer_warn!(
            "Codama decode: unresolved enum discriminant '{}'",
            discriminant
        );
        return None;
    };

    match variant {
        EnumVariantTypeNode::Empty(ev) => Some(ViewEnumTypeNode {
            discriminant,
            name: ev.name.to_string(),
            value: ViewEnumValue::Empty,
        }),
        EnumVariantTypeNode::Struct(ev) => {
            let inner = get_view_struct_type_node(
                ev.r#struct.get_nested_type_node(),
                cur,
                defined_types,
                is_last,
            );
            inner.map(|value| ViewEnumTypeNode {
                discriminant: discriminant.clone(),
                name: ev.name.to_string(),
                value: ViewEnumValue::Struct(value),
            })
        }
        EnumVariantTypeNode::Tuple(ev) => {
            let inner = get_view_tuple_type_node(
                ev.tuple.get_nested_type_node(),
                cur,
                defined_types,
                is_last,
            );
            inner.map(|value| ViewEnumTypeNode {
                discriminant: discriminant.clone(),
                name: ev.name.to_string(),
                value: ViewEnumValue::Tuple(value),
            })
        }
    }
}
