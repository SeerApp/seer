use crate::idl::{
    codama::{ctx::CodamaParseCtx, cursor::CodamaCursor},
    cursor::Cursor,
    parsed_arg::{
        ParsedArgValue, ViewAmountTypeNode, ViewArrayTypeNode, ViewBooleanTypeNode,
        ViewBytesTypeNode, ViewDateTimeTypeNode, ViewEnumTypeNode, ViewEnumValue,
        ViewHiddenPrefixTypeNode, ViewHiddenSuffixTypeNode, ViewMapEntryTypeNode, ViewMapTypeNode,
        ViewNumberTypeNode, ViewOptionTypeNode, ViewPublicKeyTypeNode, ViewSetTypeNode,
        ViewStringTypeNode, ViewStructFieldTypeNode, ViewStructTypeNode, ViewTupleTypeNode,
    },
    IdlIssue,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use codama_nodes::{
    AmountTypeNode, ArrayTypeNode, BooleanTypeNode, BytesEncoding, BytesValueNode,
    DateTimeTypeNode, DefinedTypeNode, EnumTypeNode, EnumVariantTypeNode, HiddenPrefixTypeNode,
    HiddenSuffixTypeNode, InstructionInputValueNode, MapTypeNode, NestedTypeNodeTrait, Number,
    NumberFormat, NumberTypeNode, NumberValueNode, OptionTypeNode, SetTypeNode, StringTypeNode,
    StructTypeNode, TupleTypeNode, TypeNode, ValueNode, ZeroableOptionTypeNode,
    SentinelTypeNode,
};
use solana_pubkey::Pubkey;

pub fn get_parsed_arg_value(
    ctx: &mut CodamaParseCtx<'_>,
    origin: &TypeNode,
    cur: &mut Cursor<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
    passed_len: Option<usize>,
) -> Option<ParsedArgValue> {
    match origin {
        TypeNode::Link(link) => {
            let Some(resolved) = defined_types.iter().find(|dt| dt.name == link.name) else {
                ctx.issues.note(IdlIssue::MissingDefinedTypeLink {
                    link_name: link.name.to_string(),
                    at: ctx.current_location(),
                });
                return None;
            };
            get_parsed_arg_value(
                ctx,
                &resolved.r#type,
                cur,
                defined_types,
                is_last,
                passed_len,
            )
        }
        TypeNode::Amount(a) => Some(ParsedArgValue::Amount(get_view_amount_type_node(
            a, cur, ctx,
        )?)),
        TypeNode::Array(a) => Some(ParsedArgValue::Array(get_view_array_type_node(
            a, cur, ctx, defined_types, is_last,
        )?)),
        TypeNode::Boolean(b) => Some(ParsedArgValue::Boolean(get_view_boolean_type_node(
            b, cur, ctx,
        )?)),
        TypeNode::Bytes(_) => {
            if !is_last && passed_len.is_none() {
                ctx.issues.note(IdlIssue::InvalidLayoutBytesOrStringWithoutLength {
                    at: ctx.current_location(),
                });
                return None;
            }
            Some(ParsedArgValue::Bytes(get_view_bytes_type_node(
                cur, ctx, passed_len,
            )?))
        }
        TypeNode::DateTime(d) => Some(ParsedArgValue::DateTime(get_view_date_time_type_node(
            d, cur, ctx,
        )?)),
        TypeNode::Enum(e) => Some(ParsedArgValue::Enum(get_view_enum_type_node(
            e, cur, ctx, defined_types, is_last,
        )?)),
        TypeNode::HiddenPrefix(h) => Some(ParsedArgValue::HiddenPrefix(
            get_view_hidden_prefix_type_node(h, cur, ctx, defined_types, is_last)?,
        )),
        TypeNode::HiddenSuffix(h) => Some(ParsedArgValue::HiddenSuffix(
            get_view_hidden_suffix_type_node(h, cur, ctx, defined_types, is_last)?,
        )),
        TypeNode::Map(m) => Some(ParsedArgValue::Map(get_view_map_type_node(
            m, cur, ctx, defined_types, is_last,
        )?)),
        TypeNode::Number(n) => Some(ParsedArgValue::Number(get_view_number_type_node(
            n, cur, ctx,
        )?)),
        TypeNode::Struct(s) => Some(ParsedArgValue::Struct(get_view_struct_type_node(
            s, cur, ctx, defined_types, is_last,
        )?)),
        TypeNode::Option(o) => Some(ParsedArgValue::Option(get_view_option_type_node(
            o, cur, ctx, defined_types, is_last,
        )?)),
        TypeNode::PublicKey(_) => Some(ParsedArgValue::PublicKey(get_view_public_key_type_node(
            cur, ctx,
        )?)),
        TypeNode::Tuple(t) => Some(ParsedArgValue::Tuple(get_view_tuple_type_node(
            t, cur, ctx, defined_types, is_last,
        )?)),
        TypeNode::String(s) => {
            if !is_last && passed_len.is_none() {
                ctx.issues.note(IdlIssue::InvalidLayoutBytesOrStringWithoutLength {
                    at: ctx.current_location(),
                });
                return None;
            }
            Some(ParsedArgValue::String(get_view_string_type_node(
                s, cur, ctx, passed_len,
            )?))
        }
        TypeNode::FixedSize(f) => cur.get_fixed_size_value(f, ctx, defined_types),
        TypeNode::PostOffset(p) => cur.get_post_offset_value(p, ctx, defined_types, is_last),
        TypeNode::PreOffset(p) => cur.get_pre_offset_value(p, ctx, defined_types, is_last),
        TypeNode::RemainderOption(r) => {
            if !is_last {
                ctx.issues.note(IdlIssue::InvalidLayoutRemainderOptionNotLast {
                    at: ctx.current_location(),
                });
                return None;
            }
            if !cur.is_empty() {
                get_parsed_arg_value(ctx, &r.item, cur, defined_types, is_last, passed_len)
            } else {
                None
            }
        }
        TypeNode::Sentinel(s) => Some(ParsedArgValue::Option(
            get_view_sentinel_type_node(s, cur, ctx, defined_types, is_last)?,
        )),
        TypeNode::Set(s) => Some(ParsedArgValue::Set(get_view_set_type_node(
            s, cur, ctx, defined_types, is_last,
        )?)),
        TypeNode::SizePrefix(s) => cur.get_dynamic_value(s, ctx, defined_types, is_last),
        TypeNode::SolAmount(s) => Some(ParsedArgValue::Number(get_view_number_type_node(
            s.number.get_nested_type_node(),
            cur,
            ctx,
        )?)),
        TypeNode::ZeroableOption(z) => Some(ParsedArgValue::Option(
            get_view_zeroable_option_type_node(z, cur, ctx, defined_types, is_last)?,
        )),
    }
}

pub fn eq_value_node(
    parsed_arg_value: &ParsedArgValue,
    value: &ValueNode,
    ctx: Option<&mut CodamaParseCtx<'_>>,
) -> bool {
    match (parsed_arg_value, value) {
        (ParsedArgValue::Number(arg), ValueNode::Number(node)) => {
            let node: &NumberValueNode = node;
            eq_number_value(arg, node.number)
        }
        (ParsedArgValue::Bytes(arg), ValueNode::Bytes(node)) => {
            let node: &BytesValueNode = node;
            let Some(expected) = get_view_bytes_type_node_from_value(node, ctx) else {
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
    ctx: Option<&mut CodamaParseCtx<'_>>,
) -> bool {
    match (parsed_arg_value, value) {
        (ParsedArgValue::Number(arg), InstructionInputValueNode::Number(node)) => {
            let node: &NumberValueNode = node;
            eq_number_value(arg, node.number)
        }
        (ParsedArgValue::Bytes(arg), InstructionInputValueNode::Bytes(node)) => {
            let node: &BytesValueNode = node;
            let Some(expected) = get_view_bytes_type_node_from_value(node, ctx) else {
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
    ctx: &mut CodamaParseCtx<'_>,
) -> Option<ViewNumberTypeNode> {
    Some(ViewNumberTypeNode {
        value: cur.get_number_value(origin, ctx)?,
        format: origin.format.clone(),
    })
}

pub fn get_view_struct_type_node<'a>(
    origin: &StructTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewStructTypeNode> {
    let mut fields = vec![];

    for field in &origin.fields {
        ctx.push_path(field.name.to_string());
        let value = get_parsed_arg_value(ctx, &field.r#type, cur, defined_types, is_last, None);
        let Some(value) = value else {
            ctx.note_decode_residual(format!("struct field `{}`", field.name.as_ref()));
            ctx.pop_path();
            return None;
        };
        ctx.pop_path();
        fields.push(ViewStructFieldTypeNode {
            name: String::from(field.name.clone()),
            docs: field.docs.clone(),
            value,
        });
    }

    Some(ViewStructTypeNode { fields })
}

pub fn get_view_option_type_node<'a>(
    origin: &OptionTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewOptionTypeNode> {
    Some(ViewOptionTypeNode {
        value: Box::new(cur.get_option_value(origin, ctx, defined_types, is_last)?),
    })
}

pub fn get_view_zeroable_option_type_node<'a>(
    origin: &ZeroableOptionTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewOptionTypeNode> {
    let parsed = get_parsed_arg_value(ctx, &origin.item, cur, defined_types, is_last, None)?;
    let is_none = match origin.zero_value.as_ref() {
        Some(zero_value) => eq_value_node(&parsed, &zero_value.value, Some(ctx)),
        None => is_default_zero_value(&parsed),
    };
    Some(ViewOptionTypeNode {
        value: Box::new(if is_none { None } else { Some(parsed) }),
    })
}

pub fn get_view_sentinel_type_node<'a>(
    origin: &SentinelTypeNode<TypeNode>,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewOptionTypeNode> {
    let parsed = get_parsed_arg_value(ctx, &origin.r#type, cur, defined_types, is_last, None)?;
    let is_none = eq_value_node(&parsed, &origin.sentinel.value, Some(ctx));
    Some(ViewOptionTypeNode {
        value: Box::new(if is_none { None } else { Some(parsed) }),
    })
}

fn is_default_zero_value(value: &ParsedArgValue) -> bool {
    match value {
        ParsedArgValue::Number(v) => match v.format {
            NumberFormat::F32 | NumberFormat::F64 => {
                v.value.parse::<f64>().ok() == Some(0.0)
            }
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

pub fn get_view_public_key_type_node<'a>(
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
) -> Option<ViewPublicKeyTypeNode> {
    Some(ViewPublicKeyTypeNode {
        value: cur.get_pubkey_value(ctx)?,
    })
}

pub fn get_view_string_type_node<'a>(
    origin: &StringTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    passed_len: Option<usize>,
) -> Option<ViewStringTypeNode> {
    Some(ViewStringTypeNode {
        value: cur.get_string_value(origin, ctx, passed_len)?,
    })
}

pub fn get_view_amount_type_node<'a>(
    origin: &AmountTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
) -> Option<ViewAmountTypeNode> {
    Some(ViewAmountTypeNode {
        decimals: origin.decimals.clone(),
        unit: origin.unit.clone(),
        number: get_view_number_type_node(origin.number.get_nested_type_node(), cur, ctx)?,
    })
}

pub fn get_view_boolean_type_node<'a>(
    origin: &BooleanTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
) -> Option<ViewBooleanTypeNode> {
    let raw = cur.get_number_value(origin.size.get_nested_type_node(), ctx)?;
    let value = raw.parse::<i128>().map(|v| v != 0).unwrap_or(false);

    Some(ViewBooleanTypeNode { value })
}

pub fn get_view_array_type_node<'a>(
    origin: &ArrayTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewArrayTypeNode> {
    Some(ViewArrayTypeNode {
        values: cur.get_array_value(origin, ctx, defined_types, is_last)?,
    })
}

pub fn get_view_set_type_node<'a>(
    origin: &SetTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewSetTypeNode> {
    Some(ViewSetTypeNode {
        values: cur.get_set_value(origin, ctx, defined_types, is_last)?,
    })
}

pub fn get_view_tuple_type_node<'a>(
    origin: &TupleTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewTupleTypeNode> {
    let mut items = vec![];
    for (i, item) in origin.items.iter().enumerate() {
        let seg = format!("{i}");
        ctx.push_path(seg);
        let v = get_parsed_arg_value(ctx, item, cur, defined_types, is_last, None);
        let Some(v) = v else {
            ctx.note_decode_residual(format!("tuple item index {i}"));
            ctx.pop_path();
            return None;
        };
        ctx.pop_path();
        items.push(v);
    }
    Some(ViewTupleTypeNode { items })
}

pub fn get_view_bytes_type_node<'a>(
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    passed_len: Option<usize>,
) -> Option<ViewBytesTypeNode> {
    Some(ViewBytesTypeNode {
        value: cur.get_bytes_value(ctx, passed_len)?,
    })
}

pub fn get_view_bytes_type_node_from_value(
    value: &BytesValueNode,
    ctx: Option<&mut CodamaParseCtx<'_>>,
) -> Option<ViewBytesTypeNode> {
    let final_value = match value.encoding {
        BytesEncoding::Base16 => value.data.clone(),
        BytesEncoding::Base58 => {
            let Some(vec) = bs58::decode(&value.data).into_vec().ok() else {
                if let Some(c) = ctx {
                    c.issues.note(IdlIssue::BytesLiteralDecodeFailed {
                        at: c.current_location(),
                        encoding: "base58".into(),
                    });
                }
                return None;
            };
            hex::encode(vec)
        }
        BytesEncoding::Base64 => {
            let Ok(bytes) = STANDARD.decode(&value.data) else {
                if let Some(c) = ctx {
                    c.issues.note(IdlIssue::BytesLiteralDecodeFailed {
                        at: c.current_location(),
                        encoding: "base64".into(),
                    });
                }
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
    ctx: &mut CodamaParseCtx<'_>,
) -> Option<ViewDateTimeTypeNode> {
    let number = origin.number.get_nested_type_node();
    Some(ViewDateTimeTypeNode {
        value: cur.get_number_value(number, ctx)?,
        format: number.format.clone(),
    })
}

pub fn get_view_hidden_prefix_type_node<'a>(
    origin: &HiddenPrefixTypeNode<TypeNode>,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewHiddenPrefixTypeNode> {
    let mut prefix = vec![];
    for constant in &origin.prefix {
        let v = get_parsed_arg_value(ctx, &*constant.r#type, cur, defined_types, is_last, None)?;
        prefix.push(v);
    }
    let value = Box::new(get_parsed_arg_value(
        ctx,
        &*origin.r#type,
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
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewHiddenSuffixTypeNode> {
    let value = Box::new(get_parsed_arg_value(
        ctx,
        &*origin.r#type,
        cur,
        defined_types,
        is_last,
        None,
    )?);
    let mut suffix = vec![];

    for constant in &origin.suffix {
        let v = get_parsed_arg_value(ctx, &*constant.r#type, cur, defined_types, is_last, None)?;
        suffix.push(v);
    }
    Some(ViewHiddenSuffixTypeNode { suffix, value })
}

pub fn get_view_map_type_node<'a>(
    origin: &MapTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewMapTypeNode> {
    let pairs = cur.get_map_value(origin, ctx, defined_types, is_last)?;
    let entries = pairs
        .into_iter()
        .map(|(key, value)| ViewMapEntryTypeNode { key, value })
        .collect();
    Some(ViewMapTypeNode { entries })
}

pub fn get_view_enum_type_node<'a>(
    origin: &EnumTypeNode,
    cur: &mut Cursor<'a>,
    ctx: &mut CodamaParseCtx<'_>,
    defined_types: &[DefinedTypeNode],
    is_last: bool,
) -> Option<ViewEnumTypeNode> {
    let discriminant = cur.get_number_value(origin.size.get_nested_type_node(), ctx)?;
    let tag = discriminant.parse::<usize>().unwrap_or(usize::MAX);

    let variant = origin
        .variants
        .get(tag)
        .or_else(|| {
            origin.variants.iter().find(|v| match v {
                EnumVariantTypeNode::Empty(ev) => ev.discriminator == Some(tag),
                EnumVariantTypeNode::Struct(ev) => ev.discriminator == Some(tag),
                EnumVariantTypeNode::Tuple(ev) => ev.discriminator == Some(tag),
            })
        });

    let Some(variant) = variant else {
        ctx.issues.note(IdlIssue::EnumDiscriminantUnresolved {
            at: ctx.current_location(),
            raw_discriminant: discriminant.clone(),
        });
        return None;
    };

    match variant {
        EnumVariantTypeNode::Empty(ev) => Some(ViewEnumTypeNode {
            discriminant,
            name: ev.name.to_string(),
            value: ViewEnumValue::Empty,
        }),
        EnumVariantTypeNode::Struct(ev) => {
            ctx.push_path(ev.name.to_string());
            let inner = get_view_struct_type_node(
                ev.r#struct.get_nested_type_node(),
                cur,
                ctx,
                defined_types,
                is_last,
            );
            ctx.pop_path();
            inner.map(|value| ViewEnumTypeNode {
                discriminant: discriminant.clone(),
                name: ev.name.to_string(),
                value: ViewEnumValue::Struct(value),
            })
        }
        EnumVariantTypeNode::Tuple(ev) => {
            ctx.push_path(ev.name.to_string());
            let inner = get_view_tuple_type_node(
                ev.tuple.get_nested_type_node(),
                cur,
                ctx,
                defined_types,
                is_last,
            );
            ctx.pop_path();
            inner.map(|value| ViewEnumTypeNode {
                discriminant: discriminant.clone(),
                name: ev.name.to_string(),
                value: ViewEnumValue::Tuple(value),
            })
        }
    }
}
