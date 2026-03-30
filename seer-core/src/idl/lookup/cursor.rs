use core::panic;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use codama_nodes::{
    ArrayTypeNode, BytesEncoding, CountNode, Endian, FixedSizeTypeNode, MapTypeNode,
    NestedTypeNodeTrait, NumberFormat, NumberTypeNode, OptionTypeNode, PostOffsetStrategy,
    PostOffsetTypeNode, PreOffsetStrategy, PreOffsetTypeNode, SetTypeNode, StringTypeNode,
    TypeNode,
};
use solana_program::short_vec::decode_shortu16_len;
use solana_pubkey::Pubkey;

use crate::idl::lookup::parsed_arg::ParsedArg;

pub struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { buf: data, pos: 0 }
    }

    pub fn clone(&self) -> Self {
        Self {
            buf: self.buf,
            pos: self.pos.clone(),
        }
    }

    fn decode_with_count_node<T, F>(&mut self, count: &CountNode, mut decode_one: F) -> Vec<T>
    where
        F: FnMut(&mut Self) -> Option<T>,
    {
        let mut values = Vec::new();

        match count {
            CountNode::Fixed(fixed) => {
                for _ in 0..fixed.value {
                    if let Some(decoded) = decode_one(self) {
                        values.push(decoded);
                    }
                }
            }
            CountNode::Prefixed(prefix) => {
                let len_raw = self.get_number_value(prefix.prefix.get_nested_type_node());
                let len = len_raw
                    .parse::<usize>()
                    .expect("Failed to read count prefix");

                for _ in 0..len {
                    if let Some(decoded) = decode_one(self) {
                        values.push(decoded);
                    }
                }
            }
            CountNode::Remainder(_) => {
                while self.pos < self.buf.len() {
                    let before = self.pos;
                    let value = decode_one(self);
                    let after = self.pos;

                    // Protect against non-progressing decodes for truncated data.
                    if after == before {
                        break;
                    } else if after > before {
                        panic!("Poorly aligned count nodes");
                    }

                    if let Some(decoded) = value {
                        values.push(decoded);
                    }
                }
            }
        }

        values
    }

    pub fn take(&mut self, n: usize) -> &[u8] {
        let end = self.pos + n;
        if end > self.buf.len() {
            panic!("Taking length exceeding data buffer from Cursor");
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        slice
    }

    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn set_pos_absolute(&mut self, offset: i32) {
        let len = self.buf.len() as i32;

        let new_pos = if offset >= 0 {
            offset
        } else {
            len + offset // from end
        };

        self.pos = new_pos.clamp(0, len) as usize;
    }

    pub fn set_pos_relative(&mut self, offset: i32) {
        self.set_pos_relative_from(offset, self.pos);
    }

    pub fn set_pos_relative_from(&mut self, offset: i32, from: usize) {
        let len = self.buf.len() as i32;
        let new_pos = from as i32 + offset;

        if new_pos < 0 || new_pos > len {
            panic!("Relative offset out of bounds");
        }

        self.pos = new_pos as usize;
    }

    pub fn get_number_value(&mut self, origin: &NumberTypeNode) -> String {
        match origin.format {
            NumberFormat::U8 => self.take(1)[0].to_string(),
            NumberFormat::I8 => (self.take(1)[0] as i8).to_string(),
            NumberFormat::U16 => {
                let slice = self.take(2);
                match origin.endian {
                    Endian::Little => LittleEndian::read_u16(slice).to_string(),
                    Endian::Big => BigEndian::read_u16(slice).to_string(),
                }
            }
            NumberFormat::I16 => {
                let slice = self.take(2);
                match origin.endian {
                    Endian::Little => LittleEndian::read_i16(slice).to_string(),
                    Endian::Big => BigEndian::read_i16(slice).to_string(),
                }
            }

            NumberFormat::U32 => {
                let slice = self.take(4);
                match origin.endian {
                    Endian::Little => LittleEndian::read_u32(slice).to_string(),
                    Endian::Big => BigEndian::read_u32(slice).to_string(),
                }
            }

            NumberFormat::I32 => {
                let slice = self.take(4);
                match origin.endian {
                    Endian::Little => LittleEndian::read_i32(slice).to_string(),
                    Endian::Big => BigEndian::read_i32(slice).to_string(),
                }
            }

            NumberFormat::F32 => {
                let slice = self.take(4);
                match origin.endian {
                    Endian::Little => LittleEndian::read_f32(slice).to_string(),
                    Endian::Big => BigEndian::read_f32(slice).to_string(),
                }
            }

            NumberFormat::U64 => {
                let slice = self.take(8);
                match origin.endian {
                    Endian::Little => LittleEndian::read_u64(slice).to_string(),
                    Endian::Big => BigEndian::read_u64(slice).to_string(),
                }
            }

            NumberFormat::I64 => {
                let slice = self.take(8);
                match origin.endian {
                    Endian::Little => LittleEndian::read_i64(slice).to_string(),
                    Endian::Big => BigEndian::read_i64(slice).to_string(),
                }
            }

            NumberFormat::F64 => {
                let slice = self.take(8);
                match origin.endian {
                    Endian::Little => LittleEndian::read_f64(slice).to_string(),
                    Endian::Big => BigEndian::read_f64(slice).to_string(),
                }
            }

            NumberFormat::U128 => {
                let slice = self.take(16);
                match origin.endian {
                    Endian::Little => LittleEndian::read_u128(slice).to_string(),
                    Endian::Big => BigEndian::read_u128(slice).to_string(),
                }
            }

            NumberFormat::I128 => {
                let slice = self.take(16);
                match origin.endian {
                    Endian::Little => LittleEndian::read_i128(slice).to_string(),
                    Endian::Big => BigEndian::read_i128(slice).to_string(),
                }
            }

            NumberFormat::ShortU16 => {
                // `shortvec<u16>` is variable-length, so we must decode and then
                // advance `cur` by the returned `consumed` byte count.
                let remaining = &self.buf[self.pos..];
                decode_shortu16_len(remaining)
                    .map(|(value, consumed)| {
                        self.pos += consumed;
                        value.to_string()
                    })
                    .unwrap_or_default()
            }
        }
    }

    pub fn get_pubkey_value(&mut self) -> Pubkey {
        let slice = self.take(32);
        let arr: [u8; 32] = slice.try_into().expect("32 bytes");
        Pubkey::new_from_array(arr)
    }

    pub fn get_option_value(&mut self, origin: &OptionTypeNode) -> Option<ParsedArg> {
        // Move tracking index up the prefix amount.
        let number_value = self.get_number_value(origin.prefix.get_nested_type_node());

        // None case.
        if number_value == "0" {
            // Reserve space for absent value.
            if !origin.fixed {
                // Move tracking index up the reserved amount.
                ParsedArg::from(&*origin.item, self);
            }

            None
        // Some case.
        } else if number_value == "1" {
            Some(ParsedArg::from(&*origin.item, self).expect("Option values cannot be residual"))
        } else {
            panic!("Option value prefix is {}", number_value);
        }
    }

    pub fn get_string_value(&mut self, origin: &StringTypeNode) -> String {
        let remaining = &self.buf[self.pos..];
        let (len, consumed) = decode_shortu16_len(remaining).unwrap();
        self.pos += consumed;

        let bytes = self.take(len);

        match origin.encoding {
            BytesEncoding::Base16 => hex::encode(bytes),
            BytesEncoding::Base58 => bs58::encode(bytes).into_string(),
            BytesEncoding::Base64 => STANDARD.encode(bytes),
            BytesEncoding::Utf8 => String::from_utf8_lossy(bytes).to_string(),
        }
    }

    pub fn get_array_value(&mut self, origin: &ArrayTypeNode) -> Vec<ParsedArg> {
        let item = &origin.item;
        self.decode_with_count_node(&origin.count, |cursor| ParsedArg::from(item, cursor))
    }

    pub fn get_set_value(&mut self, origin: &SetTypeNode) -> Vec<ParsedArg> {
        let item = &origin.item;
        self.decode_with_count_node(&origin.count, |cursor| ParsedArg::from(item, cursor))
    }

    pub fn get_map_value(&mut self, origin: &MapTypeNode) -> Vec<(ParsedArg, ParsedArg)> {
        let key = &origin.key;
        let value = &origin.value;
        self.decode_with_count_node(&origin.count, |cursor| {
            let decoded_key = ParsedArg::from(key, cursor).expect("Map values cannot be residual");
            let decoded_value =
                ParsedArg::from(value, cursor).expect("Map values cannot be residual");
            Some((decoded_key, decoded_value))
        })
    }

    pub fn get_bytes_value(&mut self) -> String {
        let remaining = &self.buf[self.pos..];
        let (len, consumed) = decode_shortu16_len(remaining).expect("Bytes cannot be parsed");
        self.pos += consumed;

        let end = self.pos + len;
        let bytes = &self.buf[self.pos..end];
        self.pos = end;

        // Use hex for a stable, reversible representation regardless of UTF-8 validity.
        hex::encode(bytes)
    }

    pub fn get_fixed_size_value(
        &mut self,
        origin: &FixedSizeTypeNode<TypeNode>,
    ) -> Option<ParsedArg> {
        let bytes = self.take(origin.size);

        // Decode the wrapped type from the fixed-size slice only.
        let mut inner = Cursor::new(bytes);
        ParsedArg::from(&origin.r#type, &mut inner)
    }

    pub fn get_post_offset_value(
        &mut self,
        origin: &PostOffsetTypeNode<TypeNode>,
    ) -> Option<ParsedArg> {
        match origin.strategy {
            PostOffsetStrategy::Absolute => {
                let value = ParsedArg::from(&origin.r#type, self);
                self.set_pos_absolute(origin.offset);
                value
            }
            // Identical when decoding.
            PostOffsetStrategy::Padded | PostOffsetStrategy::Relative => {
                let value = ParsedArg::from(&origin.r#type, self);
                self.set_pos_relative(origin.offset);
                value
            }
            PostOffsetStrategy::PreOffset => {
                let start_pos = self.pos;
                let value = ParsedArg::from(&origin.r#type, self);
                self.set_pos_relative_from(origin.offset, start_pos);
                value
            }
        }
    }

    pub fn get_pre_offset_value(
        &mut self,
        origin: &PreOffsetTypeNode<TypeNode>,
    ) -> Option<ParsedArg> {
        match origin.strategy {
            PreOffsetStrategy::Absolute => {
                self.set_pos_absolute(origin.offset);
            }
            PreOffsetStrategy::Padded | PreOffsetStrategy::Relative => {
                self.set_pos_relative(origin.offset);
            }
        }

        ParsedArg::from(&origin.r#type, self)
    }

    // Sentinel-related
    // pub fn get_sentinel_value(&mut self, origin: &SentinelTypeNode<TypeNode>) -> Vec<ParsedArg> {
    //     let mut sentinel_cursor = self.clone();
    //     let sentinel_value = ParsedArg::from(&origin.sentinel.r#type, &mut sentinel_cursor)
    //         .expect("Sentinel cannot be residual value");

    //     origin.sentinel.value
    // }
}
