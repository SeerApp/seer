use std::{collections::HashMap, str::FromStr};

use anchor_lang_idl_spec::{
    IdlArrayLen, IdlDefinedFields, IdlField, IdlGenericArg, IdlType, IdlTypeDef, IdlTypeDefGeneric,
    IdlTypeDefTy,
};
use byteorder::{ByteOrder, LittleEndian};
use codama_nodes::NumberFormat;
use solana_pubkey::Pubkey;

use crate::idl::{
    anchor::ctx::AnchorParseCtx,
    cursor::Cursor,
    parsed_arg::{
        ParsedArgValue, ViewArrayTypeNode, ViewBooleanTypeNode, ViewBytesTypeNode,
        ViewEnumTypeNode, ViewEnumValue, ViewNumberTypeNode, ViewOptionTypeNode,
        ViewPublicKeyTypeNode, ViewStringTypeNode, ViewStructFieldTypeNode, ViewStructTypeNode,
        ViewTupleTypeNode,
    },
    IdlIssue,
};

#[derive(Clone)]
pub struct ConstHolder {
    ty: IdlType,
    value: String,
}

#[derive(Clone)]
pub enum GenericHolder {
    Generic(IdlType),
    Constant(ConstHolder),
}

fn take_bytes<'a, 'b>(
    cursor: &'b mut Cursor<'a>,
    n: usize,
    ctx: &mut AnchorParseCtx<'_>,
    operation: &'static str,
) -> Option<&'b [u8]> {
    cursor.take_or_else(n, |rem| ctx.note_insufficient_bytes(n, rem, operation))
}

pub fn get_parsed_arg_value<'a>(
    types: &Vec<IdlTypeDef>,
    arg: &IdlField,
    cursor: &mut Cursor<'a>,
    ctx: &mut AnchorParseCtx<'_>,
) -> Option<ParsedArgValue> {
    get_parsed_arg_value_from_ty(types, &arg.ty, cursor, HashMap::new(), ctx)
}

fn get_parsed_arg_value_from_ty<'a>(
    types: &Vec<IdlTypeDef>,
    ty: &IdlType,
    cursor: &mut Cursor<'a>,
    mut generics_maps: HashMap<String, GenericHolder>,
    ctx: &mut AnchorParseCtx<'_>,
) -> Option<ParsedArgValue> {
    match ty {
        IdlType::Bool => {
            let s = take_bytes(cursor, 1, ctx, "bool u8")?;
            Some(ParsedArgValue::Boolean(ViewBooleanTypeNode {
                value: s[0] != 0,
            }))
        }
        IdlType::U8 => {
            let s = take_bytes(cursor, 1, ctx, "u8")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: s[0].to_string(),
                format: NumberFormat::U8,
            }))
        }
        IdlType::U16 => {
            let s = take_bytes(cursor, 2, ctx, "u16")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_u16(s).to_string(),
                format: NumberFormat::U16,
            }))
        }
        IdlType::U32 => {
            let s = take_bytes(cursor, 4, ctx, "u32")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_u32(s).to_string(),
                format: NumberFormat::U32,
            }))
        }
        IdlType::U64 => {
            let s = take_bytes(cursor, 8, ctx, "u64")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_u64(s).to_string(),
                format: NumberFormat::U64,
            }))
        }
        IdlType::U128 => {
            let s = take_bytes(cursor, 16, ctx, "u128")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_u128(s).to_string(),
                format: NumberFormat::U128,
            }))
        }
        IdlType::I8 => {
            let s = take_bytes(cursor, 1, ctx, "i8")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: (s[0] as i8).to_string(),
                format: NumberFormat::I8,
            }))
        }
        IdlType::I16 => {
            let s = take_bytes(cursor, 2, ctx, "i16")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_i16(s).to_string(),
                format: NumberFormat::I16,
            }))
        }
        IdlType::I32 => {
            let s = take_bytes(cursor, 4, ctx, "i32")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_i32(s).to_string(),
                format: NumberFormat::I32,
            }))
        }
        IdlType::I64 => {
            let s = take_bytes(cursor, 8, ctx, "i64")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_i64(s).to_string(),
                format: NumberFormat::I64,
            }))
        }
        IdlType::I128 => {
            let s = take_bytes(cursor, 16, ctx, "i128")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_i128(s).to_string(),
                format: NumberFormat::I128,
            }))
        }
        IdlType::F32 => {
            let s = take_bytes(cursor, 4, ctx, "f32")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_f32(s).to_string(),
                format: NumberFormat::F32,
            }))
        }
        IdlType::F64 => {
            let s = take_bytes(cursor, 8, ctx, "f64")?;
            Some(ParsedArgValue::Number(ViewNumberTypeNode {
                value: LittleEndian::read_f64(s).to_string(),
                format: NumberFormat::F64,
            }))
        }
        IdlType::String => {
            let len_bytes = take_bytes(cursor, 4, ctx, "string length u32")?;
            let len = LittleEndian::read_u32(len_bytes) as usize;
            let raw_bytes = take_bytes(cursor, len, ctx, "string payload")?;
            Some(ParsedArgValue::String(ViewStringTypeNode {
                value: String::from_utf8_lossy(raw_bytes).to_string(),
            }))
        }
        IdlType::Bytes => {
            let len_bytes = take_bytes(cursor, 4, ctx, "bytes length u32")?;
            let len = LittleEndian::read_u32(len_bytes) as usize;
            let raw_bytes = take_bytes(cursor, len, ctx, "bytes payload")?;
            Some(ParsedArgValue::Bytes(ViewBytesTypeNode {
                value: hex::encode(raw_bytes),
            }))
        }
        IdlType::Pubkey => {
            let pubkey_bytes = take_bytes(cursor, 32, ctx, "pubkey (32 bytes)")?;
            let arr: [u8; 32] = pubkey_bytes.try_into().ok()?;
            Some(ParsedArgValue::PublicKey(ViewPublicKeyTypeNode {
                value: Pubkey::new_from_array(arr),
            }))
        }
        IdlType::Option(o) => {
            let tag = take_bytes(cursor, 1, ctx, "option tag u8")?;
            if tag[0] == 0 {
                Some(ParsedArgValue::Option(ViewOptionTypeNode {
                    value: Box::new(None),
                }))
            } else {
                let inner = get_parsed_arg_value_from_ty(
                    types,
                    o.as_ref(),
                    cursor,
                    generics_maps.clone(),
                    ctx,
                );
                if inner.is_none() {
                    ctx.note_decode_residual("option some payload".to_string());
                }
                Some(ParsedArgValue::Option(ViewOptionTypeNode {
                    value: Box::new(inner),
                }))
            }
        }
        IdlType::Vec(v) => {
            let len_bytes = take_bytes(cursor, 4, ctx, "vec length u32")?;
            let length_prefix = LittleEndian::read_u32(len_bytes) as usize;
            let values = get_listed_values(
                types,
                cursor,
                &generics_maps,
                v.as_ref(),
                length_prefix,
                ctx,
            )?;
            Some(ParsedArgValue::Array(ViewArrayTypeNode { values }))
        }
        IdlType::Array(a, l) => {
            let values = match l {
                IdlArrayLen::Value(v) => {
                    get_listed_values(types, cursor, &generics_maps, a.as_ref(), *v, ctx)?
                }
                IdlArrayLen::Generic(g) => {
                    let len = resolve_generic(types, cursor, &generics_maps, g, ctx)?;
                    match len {
                        ParsedArgValue::Number(n) => get_listed_values(
                            types,
                            cursor,
                            &generics_maps,
                            a.as_ref(),
                            n.value.parse::<usize>().ok()?,
                            ctx,
                        )?,
                        _ => return None,
                    }
                }
            };
            Some(ParsedArgValue::Array(ViewArrayTypeNode { values }))
        }
        IdlType::Defined { name, generics } => {
            for t in types {
                if t.name == *name && t.generics.len() == generics.len() {
                    for (tg, g) in t.generics.iter().zip(generics.iter()) {
                        match (tg, g) {
                            (
                                IdlTypeDefGeneric::Const { name, ty },
                                IdlGenericArg::Const { value },
                            ) => {
                                generics_maps.insert(
                                    name.clone(),
                                    GenericHolder::Constant(ConstHolder {
                                        ty: IdlType::from_str(ty).unwrap(),
                                        value: value.clone(),
                                    }),
                                );
                            }
                            (IdlTypeDefGeneric::Type { name }, IdlGenericArg::Type { ty }) => {
                                generics_maps
                                    .insert(name.clone(), GenericHolder::Generic(ty.clone()));
                            }
                            _ => {
                                let expected = match tg {
                                    IdlTypeDefGeneric::Const { name, ty } => {
                                        format!("const `{name}`: `{ty}`")
                                    }
                                    IdlTypeDefGeneric::Type { name } => format!("type `{name}`"),
                                };

                                let got = match g {
                                    IdlGenericArg::Const { value } => {
                                        format!("const value `{value}`")
                                    }
                                    IdlGenericArg::Type { ty } => format!("type `{ty:?}`"),
                                };

                                ctx.issues
                                    .note(IdlIssue::AnchorDefinedTypeGenericArgMismatch {
                                        at: ctx.current_location(),
                                        defined_type: name.to_string(),
                                        expected,
                                        got,
                                    });

                                return None;
                            }
                        }
                    }

                    return get_idl_type_def_ty(types, cursor, &generics_maps, &t.ty, ctx);
                }
            }
            panic!("Defined field has no corresponding types entry")
        }
        IdlType::Generic(g) => resolve_generic(types, cursor, &generics_maps, g, ctx),
        _ => None,
    }
}

pub fn get_idl_type_def_ty<'a>(
    types: &Vec<IdlTypeDef>,
    cursor: &mut Cursor<'a>,
    generics_maps: &HashMap<String, GenericHolder>,
    ty: &IdlTypeDefTy,
    ctx: &mut AnchorParseCtx<'_>,
) -> Option<ParsedArgValue> {
    match ty {
        IdlTypeDefTy::Struct { fields } => get_struct(types, fields, cursor, &generics_maps, ctx),
        IdlTypeDefTy::Enum { variants } => {
            for v in variants {
                let value = if v.fields.is_none() {
                    ViewEnumValue::Empty
                } else if let Some(parsed_arg_value) =
                    get_struct(types, &v.fields, cursor, &generics_maps, ctx)
                {
                    match parsed_arg_value {
                        ParsedArgValue::Struct(value) => ViewEnumValue::Struct(value),
                        ParsedArgValue::Tuple(value) => ViewEnumValue::Tuple(value),
                        _ => return None,
                    }
                } else {
                    return None;
                };

                return Some(ParsedArgValue::Enum(ViewEnumTypeNode {
                    name: v.name.clone(),
                    discriminant: "".to_string(),
                    value,
                }));
            }
            None
        }
        IdlTypeDefTy::Type { alias } => {
            get_parsed_arg_value_from_ty(types, alias, cursor, generics_maps.clone(), ctx)
        }
    }
}

fn get_listed_values<'a>(
    types: &Vec<IdlTypeDef>,
    cursor: &mut Cursor<'a>,
    generics_maps: &HashMap<String, GenericHolder>,
    ty: &IdlType,
    length: usize,
    ctx: &mut AnchorParseCtx<'_>,
) -> Option<Vec<ParsedArgValue>> {
    let mut values = vec![];
    for _ in 0..length {
        let parsed_arg =
            get_parsed_arg_value_from_ty(types, ty, cursor, generics_maps.clone(), ctx)?;
        values.push(parsed_arg);
    }
    Some(values)
}

fn get_parsed_arg_value_from_constant(constant: &ConstHolder) -> Option<ParsedArgValue> {
    match &constant.ty {
        IdlType::Bool => Some(ParsedArgValue::Boolean(ViewBooleanTypeNode {
            value: constant.value.parse::<bool>().ok()?,
        })),
        IdlType::U8 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::U8,
        })),
        IdlType::U16 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::U16,
        })),
        IdlType::U32 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::U32,
        })),
        IdlType::U64 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::U64,
        })),
        IdlType::U128 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::U128,
        })),
        IdlType::I8 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::I8,
        })),
        IdlType::I16 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::I16,
        })),
        IdlType::I32 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::I32,
        })),
        IdlType::I64 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::I64,
        })),
        IdlType::I128 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::I128,
        })),
        IdlType::F32 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::F32,
        })),
        IdlType::F64 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: constant.value.clone(),
            format: NumberFormat::F64,
        })),
        IdlType::String => Some(ParsedArgValue::String(ViewStringTypeNode {
            value: constant.value.clone(),
        })),
        IdlType::Bytes => Some(ParsedArgValue::Bytes(ViewBytesTypeNode {
            value: constant.value.clone(),
        })),
        IdlType::Pubkey => Some(ParsedArgValue::PublicKey(ViewPublicKeyTypeNode {
            value: Pubkey::from_str(&constant.value).expect("Constant Pubkey value"),
        })),
        IdlType::Array(a, l) => {
            let values = match l {
                IdlArrayLen::Value(v) => {
                    let elements: Vec<String> = constant
                        .value
                        .trim_matches(|c| c == '[' || c == ']')
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect();

                    if elements.len() != *v {
                        return None;
                    }

                    let mut values = vec![];
                    for e in elements {
                        let constant_holder = ConstHolder {
                            ty: a.as_ref().clone(),
                            value: e,
                        };

                        if let Some(parsed_arg) =
                            get_parsed_arg_value_from_constant(&constant_holder)
                        {
                            values.push(parsed_arg);
                        }
                    }

                    values
                }
                IdlArrayLen::Generic(_) => panic!("Generics cannot be constant values"),
            };
            Some(ParsedArgValue::Array(ViewArrayTypeNode { values }))
        }
        _ => None,
    }
}

fn resolve_generic<'a>(
    types: &Vec<IdlTypeDef>,
    cursor: &mut Cursor<'a>,
    generics_maps: &HashMap<String, GenericHolder>,
    generic: &String,
    ctx: &mut AnchorParseCtx<'_>,
) -> Option<ParsedArgValue> {
    match generics_maps.get(generic).unwrap() {
        GenericHolder::Constant(c) => get_parsed_arg_value_from_constant(c),
        GenericHolder::Generic(g) => {
            get_parsed_arg_value_from_ty(types, g, cursor, generics_maps.clone(), ctx)
        }
    }
}

fn get_struct<'a>(
    types: &Vec<IdlTypeDef>,
    fields: &Option<IdlDefinedFields>,
    cursor: &mut Cursor<'a>,
    generics_maps: &HashMap<String, GenericHolder>,
    ctx: &mut AnchorParseCtx<'_>,
) -> Option<ParsedArgValue> {
    if let Some(fields) = fields {
        match fields {
            IdlDefinedFields::Named(named_fields) => {
                let mut return_struct = ViewStructTypeNode { fields: vec![] };
                for f in named_fields {
                    ctx.push_path(f.name.clone());
                    let value = get_parsed_arg_value_from_ty(
                        types,
                        &f.ty,
                        cursor,
                        generics_maps.clone(),
                        ctx,
                    );
                    ctx.pop_path();
                    let value = value?;
                    return_struct.fields.push(ViewStructFieldTypeNode {
                        name: f.name.clone(),
                        docs: f.docs.clone().into(),
                        value,
                    });
                }
                Some(ParsedArgValue::Struct(return_struct))
            }
            IdlDefinedFields::Tuple(tuple_fields) => {
                let mut return_tuple = ViewTupleTypeNode { items: vec![] };
                for (i, f) in tuple_fields.iter().enumerate() {
                    ctx.push_path(format!("{i}"));
                    let value =
                        get_parsed_arg_value_from_ty(types, f, cursor, generics_maps.clone(), ctx);
                    ctx.pop_path();
                    return_tuple.items.push(value?);
                }
                Some(ParsedArgValue::Tuple(return_tuple))
            }
        }
    } else {
        None
    }
}
