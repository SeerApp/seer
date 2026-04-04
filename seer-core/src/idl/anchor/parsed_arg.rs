use core::panic;
use std::{collections::HashMap, str::FromStr};

use anchor_lang_idl_spec::{
    IdlArrayLen, IdlDefinedFields, IdlField, IdlGenericArg, IdlType, IdlTypeDef, IdlTypeDefGeneric,
    IdlTypeDefTy,
};
use byteorder::{ByteOrder, LittleEndian};
use codama_nodes::NumberFormat;
use solana_pubkey::Pubkey;

use crate::idl::{
    cursor::Cursor,
    parsed_arg::{
        ParsedArgValue, ViewArrayTypeNode, ViewBooleanTypeNode, ViewBytesTypeNode,
        ViewEnumTypeNode, ViewEnumValue, ViewNumberTypeNode, ViewOptionTypeNode,
        ViewPublicKeyTypeNode, ViewStringTypeNode, ViewStructFieldTypeNode, ViewStructTypeNode,
        ViewTupleTypeNode,
    },
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

pub fn get_parsed_arg_value<'a>(
    types: &Vec<IdlTypeDef>,
    arg: &IdlField,
    cursor: &mut Cursor<'a>,
) -> Option<ParsedArgValue> {
    get_parsed_arg_value_from_ty(types, &arg.ty, cursor, HashMap::new())
}

fn get_parsed_arg_value_from_ty<'a>(
    types: &Vec<IdlTypeDef>,
    ty: &IdlType,
    cursor: &mut Cursor<'a>,
    mut generics_maps: HashMap<String, GenericHolder>,
) -> Option<ParsedArgValue> {
    match ty {
        IdlType::Bool => Some(ParsedArgValue::Boolean(ViewBooleanTypeNode {
            value: cursor.take(1)[0] != 0,
        })),
        IdlType::U8 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: cursor.take(1)[0].to_string(),
            format: NumberFormat::U8,
        })),
        IdlType::U16 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_u16(cursor.take(2)).to_string(),
            format: NumberFormat::U16,
        })),
        IdlType::U32 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_u32(cursor.take(4)).to_string(),
            format: NumberFormat::U32,
        })),
        IdlType::U64 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_u64(cursor.take(8)).to_string(),
            format: NumberFormat::U64,
        })),
        IdlType::U128 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_u128(cursor.take(16)).to_string(),
            format: NumberFormat::U128,
        })),
        IdlType::I8 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: (cursor.take(1)[0] as i8).to_string(),
            format: NumberFormat::I8,
        })),
        IdlType::I16 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_i16(cursor.take(2)).to_string(),
            format: NumberFormat::I16,
        })),
        IdlType::I32 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_i32(cursor.take(4)).to_string(),
            format: NumberFormat::I32,
        })),
        IdlType::I64 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_i64(cursor.take(8)).to_string(),
            format: NumberFormat::I64,
        })),
        IdlType::I128 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_i128(cursor.take(16)).to_string(),
            format: NumberFormat::I128,
        })),
        IdlType::F32 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_f32(cursor.take(4)).to_string(),
            format: NumberFormat::F32,
        })),
        IdlType::F64 => Some(ParsedArgValue::Number(ViewNumberTypeNode {
            value: LittleEndian::read_f64(cursor.take(8)).to_string(),
            format: NumberFormat::F64,
        })),
        IdlType::String => {
            let length_prefix = LittleEndian::read_u32(cursor.take(4)) as usize;
            let raw_bytes = cursor.take(length_prefix);
            Some(ParsedArgValue::String(ViewStringTypeNode {
                value: String::from_utf8_lossy(raw_bytes).to_string(),
            }))
        }
        IdlType::Bytes => {
            let length_prefix = LittleEndian::read_u32(cursor.take(4)) as usize;
            let raw_bytes = cursor.take(length_prefix);
            Some(ParsedArgValue::Bytes(ViewBytesTypeNode {
                value: hex::encode(raw_bytes),
            }))
        }
        IdlType::Pubkey => {
            let pubkey_bytes = cursor.take(32);
            let arr: [u8; 32] = pubkey_bytes.try_into().expect("32 bytes");
            Some(ParsedArgValue::PublicKey(ViewPublicKeyTypeNode {
                value: Pubkey::new_from_array(arr),
            }))
        }
        IdlType::Option(o) => {
            if cursor.take(1)[0] == 0 {
                Some(ParsedArgValue::Option(ViewOptionTypeNode {
                    value: Box::new(None),
                }))
            } else {
                Some(ParsedArgValue::Option(ViewOptionTypeNode {
                    value: Box::new(get_parsed_arg_value_from_ty(
                        types,
                        o.as_ref(),
                        cursor,
                        generics_maps.clone(),
                    )),
                }))
            }
        }
        IdlType::Vec(v) => {
            let length_prefix = LittleEndian::read_u32(cursor.take(4)) as usize;
            let values =
                get_listed_values(types, cursor, &generics_maps, v.as_ref(), length_prefix);
            Some(ParsedArgValue::Array(ViewArrayTypeNode { values }))
        }
        IdlType::Array(a, l) => {
            let values = match l {
                IdlArrayLen::Value(v) => get_listed_values(types, cursor, &generics_maps, a.as_ref(), *v),
                IdlArrayLen::Generic(g) => {
                    if let Some(len) = resolve_generic(types, cursor, &generics_maps, g) {
                        match len {
                            ParsedArgValue::Number(n) => get_listed_values(
                                types, 
                                cursor, 
                                &generics_maps, 
                                a.as_ref(), 
                                n.value.parse::<usize>().ok()?,
                            ),
                            _ => panic!("Generic array length does not resolve to a number"),
                        }
                    } else {
                        panic!("Generic array length does not resolve to any value");
                    }
                }
            };
            Some(ParsedArgValue::Array(ViewArrayTypeNode { values: values }))
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
                            _ => panic!(),
                        }
                    }

                    return get_idl_type_def_ty(
                        types,
                        cursor,
                        &generics_maps,
                        &t.ty,
                    )
                }
            }
            panic!("Defined field has no corresponding types entry")
        }
        IdlType::Generic(g) => resolve_generic(types, cursor, &generics_maps, g),
        _ => None,
    }
}

pub fn get_idl_type_def_ty<'a>(
    types: &Vec<IdlTypeDef>,
    cursor: &mut Cursor<'a>,
    generics_maps: &HashMap<String, GenericHolder>,
    ty: &IdlTypeDefTy,
) -> Option<ParsedArgValue> {
    match ty {
        IdlTypeDefTy::Struct { fields } => {
            return get_struct(types, fields, cursor, &generics_maps)
        }
        IdlTypeDefTy::Enum { variants } => {
            for v in variants {
                let value = if let Some(parsed_arg_value) =
                    get_struct(types, &v.fields, cursor, &generics_maps)
                {
                    match parsed_arg_value {
                        ParsedArgValue::Struct(value) => {
                            ViewEnumValue::Struct(value)
                        }
                        ParsedArgValue::Tuple(value) => ViewEnumValue::Tuple(value),
                        _ => panic!("Literally cannot happen"),
                    }
                } else {
                    ViewEnumValue::Empty
                };

                return Some(ParsedArgValue::Enum(ViewEnumTypeNode {
                    name: v.name.clone(),
                    discriminant: "".to_string(),
                    value,
                }));
            }
            panic!("No corresponding enum variant");
        }
        IdlTypeDefTy::Type { alias } => {
            return get_parsed_arg_value_from_ty(
                types,
                alias,
                cursor,
                generics_maps.clone(),
            )
        }
    }
}

fn get_listed_values<'a>(
    types: &Vec<IdlTypeDef>,
    cursor: &mut Cursor<'a>,
    generics_maps: &HashMap<String, GenericHolder>,
    ty: &IdlType,
    length: usize,
) -> Vec<ParsedArgValue> {
    let mut values = vec![];
    for _ in 0..length {
        if let Some(parsed_arg) =
            get_parsed_arg_value_from_ty(types, ty, cursor, generics_maps.clone())
        {
            values.push(parsed_arg);
        }
    }
    values
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
) -> Option<ParsedArgValue> {
    match generics_maps.get(generic).unwrap() {
        GenericHolder::Constant(c) => get_parsed_arg_value_from_constant(c),
        GenericHolder::Generic(g) => {
            get_parsed_arg_value_from_ty(types, g, cursor, generics_maps.clone())
        }
    }
}

fn get_struct<'a>(
    types: &Vec<IdlTypeDef>,
    fields: &Option<IdlDefinedFields>,
    cursor: &mut Cursor<'a>,
    generics_maps: &HashMap<String, GenericHolder>,
) -> Option<ParsedArgValue> {
    if let Some(fields) = fields {
        match fields {
            IdlDefinedFields::Named(named_fields) => {
                let mut return_struct = ViewStructTypeNode { fields: vec![] };
                for f in named_fields {
                    if let Some(value) =
                        get_parsed_arg_value_from_ty(types, &f.ty, cursor, generics_maps.clone())
                    {
                        return_struct.fields.push(ViewStructFieldTypeNode {
                            name: f.name.clone(),
                            docs: f.docs.clone().into(),
                            value,
                        });
                    }
                }
                Some(ParsedArgValue::Struct(return_struct))
            }
            IdlDefinedFields::Tuple(tuple_fields) => {
                let mut return_tuple = ViewTupleTypeNode { items: vec![] };
                for f in tuple_fields {
                    if let Some(value) =
                        get_parsed_arg_value_from_ty(types, &f, cursor, generics_maps.clone())
                    {
                        return_tuple.items.push(value);
                    }
                }
                Some(ParsedArgValue::Tuple(return_tuple))
            }
        }
    } else {
        None
    }
}
