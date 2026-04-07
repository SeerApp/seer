use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use codama_nodes::{
    ArrayTypeNode, BytesEncoding, CountNode, DefinedTypeNode, Endian, FixedSizeTypeNode,
    MapTypeNode, NestedTypeNodeTrait, NumberFormat, NumberTypeNode, OptionTypeNode,
    PostOffsetStrategy, PostOffsetTypeNode, PreOffsetStrategy, PreOffsetTypeNode, SetTypeNode,
    SizePrefixTypeNode, StringTypeNode, TypeNode,
};
use solana_program::short_vec::decode_shortu16_len;
use solana_pubkey::Pubkey;

use crate::idl::{
    codama::{ctx::CodamaParseCtx, parsed_arg::get_parsed_arg_value},
    cursor::Cursor,
    parsed_arg::ParsedArgValue,
    IdlIssue,
};
pub trait CodamaCursor {
    fn decode_with_count_node<T, F>(
        &mut self,
        count: &CountNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
        decode_one: F,
    ) -> Option<Vec<T>>
    where
        F: FnMut(&mut Self, &mut CodamaParseCtx<'_>) -> Option<T>;
    fn get_number_value(
        &mut self,
        origin: &NumberTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
    ) -> Option<String>;
    fn get_pubkey_value(&mut self, ctx: &mut CodamaParseCtx<'_>) -> Option<Pubkey>;
    fn get_option_value(
        &mut self,
        origin: &OptionTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Option<ParsedArgValue>>;
    fn get_string_value(
        &mut self,
        origin: &StringTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        len: Option<usize>,
    ) -> Option<String>;
    fn get_array_value(
        &mut self,
        origin: &ArrayTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Vec<ParsedArgValue>>;
    fn get_set_value(
        &mut self,
        origin: &SetTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Vec<ParsedArgValue>>;
    fn get_map_value(
        &mut self,
        origin: &MapTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Vec<(ParsedArgValue, ParsedArgValue)>>;
    fn get_bytes_value(
        &mut self,
        ctx: &mut CodamaParseCtx<'_>,
        len: Option<usize>,
    ) -> Option<String>;
    fn get_fixed_size_value(
        &mut self,
        origin: &FixedSizeTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
    ) -> Option<ParsedArgValue>;
    fn get_post_offset_value(
        &mut self,
        origin: &PostOffsetTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<ParsedArgValue>;
    fn get_pre_offset_value(
        &mut self,
        origin: &PreOffsetTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<ParsedArgValue>;
    fn get_dynamic_value(
        &mut self,
        origin: &SizePrefixTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<ParsedArgValue>;
}

impl<'a> CodamaCursor for Cursor<'a> {
    fn decode_with_count_node<T, F>(
        &mut self,
        count: &CountNode,
        ctx: &mut CodamaParseCtx<'_>,
        _defined_types: &[DefinedTypeNode],
        _is_last: bool,
        mut decode_one: F,
    ) -> Option<Vec<T>>
    where
        F: FnMut(&mut Self, &mut CodamaParseCtx<'_>) -> Option<T>,
    {
        let mut values = Vec::new();

        match count {
            CountNode::Fixed(fixed) => {
                for _ in 0..fixed.value {
                    let decoded = decode_one(self, ctx)?;
                    values.push(decoded);
                }
            }
            CountNode::Prefixed(prefix) => {
                let len_raw = match self.get_number_value(prefix.prefix.get_nested_type_node(), ctx)
                {
                    Some(s) => s,
                    None => return None,
                };
                let Ok(len) = len_raw.parse::<usize>() else {
                    ctx.issues.note(IdlIssue::PrefixedCountInvalid {
                        at: ctx.current_location(),
                    });
                    return None;
                };

                for _ in 0..len {
                    let decoded = decode_one(self, ctx)?;
                    values.push(decoded);
                }
            }
            CountNode::Remainder(_) => {
                while self.remaining() > 0 {
                    let before = self.pos();
                    let value = decode_one(self, ctx);
                    let after = self.pos();

                    if after == before {
                        break;
                    } else if after < before {
                        ctx.issues.note(IdlIssue::RemainderDecodeCursorRegression {
                            at: ctx.current_location(),
                        });
                        break;
                    }

                    let Some(decoded) = value else {
                        return None;
                    };
                    values.push(decoded);
                }
            }
        }

        Some(values)
    }

    fn get_number_value(
        &mut self,
        origin: &NumberTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
    ) -> Option<String> {
        match origin.format {
            NumberFormat::U8 => {
                let s = self.take_or_else(1, |rem| {
                    ctx.note_insufficient_bytes(1, rem, "number U8");
                })?;
                Some(s[0].to_string())
            }
            NumberFormat::I8 => {
                let s = self.take_or_else(1, |rem| {
                    ctx.note_insufficient_bytes(1, rem, "number I8");
                })?;
                Some((s[0] as i8).to_string())
            }
            NumberFormat::U16 => {
                let slice = self.take_or_else(2, |rem| {
                    ctx.note_insufficient_bytes(2, rem, "number U16");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_u16(slice).to_string(),
                    Endian::Big => BigEndian::read_u16(slice).to_string(),
                })
            }
            NumberFormat::I16 => {
                let slice = self.take_or_else(2, |rem| {
                    ctx.note_insufficient_bytes(2, rem, "number I16");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_i16(slice).to_string(),
                    Endian::Big => BigEndian::read_i16(slice).to_string(),
                })
            }

            NumberFormat::U32 => {
                let slice = self.take_or_else(4, |rem| {
                    ctx.note_insufficient_bytes(4, rem, "number U32");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_u32(slice).to_string(),
                    Endian::Big => BigEndian::read_u32(slice).to_string(),
                })
            }

            NumberFormat::I32 => {
                let slice = self.take_or_else(4, |rem| {
                    ctx.note_insufficient_bytes(4, rem, "number I32");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_i32(slice).to_string(),
                    Endian::Big => BigEndian::read_i32(slice).to_string(),
                })
            }

            NumberFormat::F32 => {
                let slice = self.take_or_else(4, |rem| {
                    ctx.note_insufficient_bytes(4, rem, "number F32");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_f32(slice).to_string(),
                    Endian::Big => BigEndian::read_f32(slice).to_string(),
                })
            }

            NumberFormat::U64 => {
                let slice = self.take_or_else(8, |rem| {
                    ctx.note_insufficient_bytes(8, rem, "number U64");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_u64(slice).to_string(),
                    Endian::Big => BigEndian::read_u64(slice).to_string(),
                })
            }

            NumberFormat::I64 => {
                let slice = self.take_or_else(8, |rem| {
                    ctx.note_insufficient_bytes(8, rem, "number I64");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_i64(slice).to_string(),
                    Endian::Big => BigEndian::read_i64(slice).to_string(),
                })
            }

            NumberFormat::F64 => {
                let slice = self.take_or_else(8, |rem| {
                    ctx.note_insufficient_bytes(8, rem, "number F64");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_f64(slice).to_string(),
                    Endian::Big => BigEndian::read_f64(slice).to_string(),
                })
            }

            NumberFormat::U128 => {
                let slice = self.take_or_else(16, |rem| {
                    ctx.note_insufficient_bytes(16, rem, "number U128");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_u128(slice).to_string(),
                    Endian::Big => BigEndian::read_u128(slice).to_string(),
                })
            }

            NumberFormat::I128 => {
                let slice = self.take_or_else(16, |rem| {
                    ctx.note_insufficient_bytes(16, rem, "number I128");
                })?;
                Some(match origin.endian {
                    Endian::Little => LittleEndian::read_i128(slice).to_string(),
                    Endian::Big => BigEndian::read_i128(slice).to_string(),
                })
            }

            NumberFormat::ShortU16 => {
                let remaining = self.remaining_bytes();
                Some(
                    decode_shortu16_len(remaining)
                        .map(|(value, consumed)| {
                            if !self.set_pos_relative(consumed as i32) {
                                ctx.note_cursor_offset_oob();
                            }
                            value.to_string()
                        })
                        .unwrap_or_default(),
                )
            }
        }
    }

    fn get_pubkey_value(&mut self, ctx: &mut CodamaParseCtx<'_>) -> Option<Pubkey> {
        let slice = match self.take(32) {
            Some(s) => s,
            None => {
                let rem = self.remaining();
                ctx.note_insufficient_bytes(32, rem, "pubkey (32 bytes)");
                return None;
            }
        };
        let arr: [u8; 32] = slice.try_into().ok()?;
        Some(Pubkey::new_from_array(arr))
    }

    fn get_option_value(
        &mut self,
        origin: &OptionTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Option<ParsedArgValue>> {
        let number_value = self.get_number_value(origin.prefix.get_nested_type_node(), ctx)?;

        if number_value == "0" {
            if !origin.fixed {
                get_parsed_arg_value(
                    ctx,
                    &*origin.item,
                    self,
                    defined_types,
                    is_last,
                    None,
                );
            }

            Some(None)
        } else if number_value == "1" {
            let inner = get_parsed_arg_value(
                ctx,
                &*origin.item,
                self,
                defined_types,
                is_last,
                None,
            );
            if inner.is_none() {
                ctx.note_decode_residual("option_some_payload");
            }
            Some(Some(inner?))
        } else {
            ctx.issues.note(IdlIssue::OptionDiscriminantInvalid {
                at: ctx.current_location(),
                prefix: number_value,
            });
            None
        }
    }

    fn get_string_value(
        &mut self,
        origin: &StringTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        len: Option<usize>,
    ) -> Option<String> {
        let len = len.unwrap_or(self.remaining());
        let bytes = match self.take(len) {
            Some(b) => b,
            None => {
                let rem = self.remaining();
                ctx.note_insufficient_bytes(len, rem, "string payload");
                return None;
            }
        };
        Some(match origin.encoding {
            BytesEncoding::Base16 => hex::encode(bytes),
            BytesEncoding::Base58 => bs58::encode(bytes).into_string(),
            BytesEncoding::Base64 => STANDARD.encode(bytes),
            BytesEncoding::Utf8 => String::from_utf8_lossy(bytes).to_string(),
        })
    }

    fn get_array_value(
        &mut self,
        origin: &ArrayTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Vec<ParsedArgValue>> {
        let item = &origin.item;
        self.decode_with_count_node(&origin.count, ctx, defined_types, is_last, |cursor, c| {
            get_parsed_arg_value(c, item, cursor, defined_types, is_last, None)
        })
    }

    fn get_set_value(
        &mut self,
        origin: &SetTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Vec<ParsedArgValue>> {
        let item = &origin.item;
        self.decode_with_count_node(&origin.count, ctx, defined_types, is_last, |cursor, c| {
            get_parsed_arg_value(c, item, cursor, defined_types, is_last, None)
        })
    }

    fn get_map_value(
        &mut self,
        origin: &MapTypeNode,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<Vec<(ParsedArgValue, ParsedArgValue)>> {
        let key = &origin.key;
        let val = &origin.value;
        self.decode_with_count_node(&origin.count, ctx, defined_types, is_last, |cursor, c| {
            let decoded_key = get_parsed_arg_value(c, key, cursor, defined_types, is_last, None)?;
            let decoded_value =
                get_parsed_arg_value(c, val, cursor, defined_types, is_last, None)?;
            Some((decoded_key, decoded_value))
        })
    }

    fn get_bytes_value(
        &mut self,
        ctx: &mut CodamaParseCtx<'_>,
        len: Option<usize>,
    ) -> Option<String> {
        let len = len.unwrap_or(self.remaining());
        let bytes = match self.take(len) {
            Some(b) => b,
            None => {
                let rem = self.remaining();
                ctx.note_insufficient_bytes(len, rem, "bytes payload");
                return None;
            }
        };
        Some(hex::encode(bytes))
    }

    fn get_fixed_size_value(
        &mut self,
        origin: &FixedSizeTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
    ) -> Option<ParsedArgValue> {
        let bytes = match self.take(origin.size) {
            Some(b) => b,
            None => {
                let rem = self.remaining();
                ctx.note_insufficient_bytes(origin.size, rem, "fixedSize slice");
                return None;
            }
        };
        let mut inner = Cursor::new(bytes);
        get_parsed_arg_value(ctx, &origin.r#type, &mut inner, defined_types, true, None)
    }

    fn get_post_offset_value(
        &mut self,
        origin: &PostOffsetTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<ParsedArgValue> {
        match origin.strategy {
            PostOffsetStrategy::Absolute => {
                let value = get_parsed_arg_value(ctx, &origin.r#type, self, defined_types, is_last, None);
                self.set_pos_absolute(origin.offset);
                value
            }
            PostOffsetStrategy::Padded | PostOffsetStrategy::Relative => {
                let value = get_parsed_arg_value(ctx, &origin.r#type, self, defined_types, is_last, None);
                if !self.set_pos_relative(origin.offset) {
                    ctx.note_cursor_offset_oob();
                }
                value
            }
            PostOffsetStrategy::PreOffset => {
                let start_pos = self.pos();
                let value = get_parsed_arg_value(ctx, &origin.r#type, self, defined_types, is_last, None);
                if !self.set_pos_relative_from(origin.offset, start_pos) {
                    ctx.note_cursor_offset_oob();
                }
                value
            }
        }
    }

    fn get_pre_offset_value(
        &mut self,
        origin: &PreOffsetTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<ParsedArgValue> {
        match origin.strategy {
            PreOffsetStrategy::Absolute => {
                self.set_pos_absolute(origin.offset);
            }
            PreOffsetStrategy::Padded | PreOffsetStrategy::Relative => {
                if !self.set_pos_relative(origin.offset) {
                    ctx.note_cursor_offset_oob();
                }
            }
        }

        get_parsed_arg_value(ctx, &origin.r#type, self, defined_types, is_last, None)
    }

    fn get_dynamic_value(
        &mut self,
        origin: &SizePrefixTypeNode<TypeNode>,
        ctx: &mut CodamaParseCtx<'_>,
        defined_types: &[DefinedTypeNode],
        is_last: bool,
    ) -> Option<ParsedArgValue> {
        let string_number = self.get_number_value(origin.prefix.get_nested_type_node(), ctx)?;
        let Ok(passed_len) = string_number.parse::<usize>() else {
            ctx.issues.note(IdlIssue::SizePrefixInvalid {
                at: ctx.current_location(),
                raw: string_number,
            });
            return None;
        };
        get_parsed_arg_value(
            ctx,
            &origin.r#type,
            self,
            defined_types,
            is_last,
            Some(passed_len),
        )
    }
}
