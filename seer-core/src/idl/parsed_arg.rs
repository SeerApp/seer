use codama_nodes::{Docs, NumberFormat};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_pubkey::Pubkey;

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
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub docs: Docs,
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

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ParsedArgByteOffset {
    pub path: String,
    pub byte_offset: usize,
}

pub fn collect_parsed_arg_byte_offsets(root: &ParsedArg) -> Vec<ParsedArgByteOffset> {
    let mut out = Vec::new();
    collect_value_offsets(&root.value, root.name.as_str(), &mut out);
    out
}

fn collect_value_offsets(value: &ParsedArgValue, path: &str, out: &mut Vec<ParsedArgByteOffset>) {
    match value {
        ParsedArgValue::Struct(s) => {
            for field in &s.fields {
                let field_path = format!("{path}.{}", field.name);
                if let Some(byte_offset) = field.byte_offset {
                    out.push(ParsedArgByteOffset {
                        path: field_path.clone(),
                        byte_offset,
                    });
                }
                collect_value_offsets(&field.value, &field_path, out);
            }
        }
        ParsedArgValue::Tuple(t) => {
            for (idx, item) in t.items.iter().enumerate() {
                let item_path = format!("{path}[{idx}]");
                collect_value_offsets(item, &item_path, out);
            }
        }
        ParsedArgValue::Array(a) => {
            for (idx, item) in a.values.iter().enumerate() {
                let item_path = format!("{path}[{idx}]");
                collect_value_offsets(item, &item_path, out);
            }
        }
        ParsedArgValue::Set(s) => {
            for (idx, item) in s.values.iter().enumerate() {
                let item_path = format!("{path}[{idx}]");
                collect_value_offsets(item, &item_path, out);
            }
        }
        ParsedArgValue::Map(m) => {
            for (idx, entry) in m.entries.iter().enumerate() {
                let key_path = format!("{path}[{idx}].key");
                collect_value_offsets(&entry.key, &key_path, out);
                let value_path = format!("{path}[{idx}].value");
                collect_value_offsets(&entry.value, &value_path, out);
            }
        }
        ParsedArgValue::Option(o) => {
            if let Some(inner) = o.value.as_ref() {
                let inner_path = format!("{path}.value");
                collect_value_offsets(inner, &inner_path, out);
            }
        }
        ParsedArgValue::HiddenPrefix(h) => {
            for (idx, prefix) in h.prefix.iter().enumerate() {
                let prefix_path = format!("{path}.prefix[{idx}]");
                collect_value_offsets(prefix, &prefix_path, out);
            }
            let value_path = format!("{path}.value");
            collect_value_offsets(&h.value, &value_path, out);
        }
        ParsedArgValue::HiddenSuffix(h) => {
            let value_path = format!("{path}.value");
            collect_value_offsets(&h.value, &value_path, out);
            for (idx, suffix) in h.suffix.iter().enumerate() {
                let suffix_path = format!("{path}.suffix[{idx}]");
                collect_value_offsets(suffix, &suffix_path, out);
            }
        }
        ParsedArgValue::Enum(e) => match &e.value {
            ViewEnumValue::Empty => {}
            ViewEnumValue::Tuple(t) => {
                for (idx, item) in t.items.iter().enumerate() {
                    let item_path = format!("{path}.{}.tuple[{idx}]", e.name);
                    collect_value_offsets(item, &item_path, out);
                }
            }
            ViewEnumValue::Struct(s) => {
                for field in &s.fields {
                    let field_path = format!("{path}.{}.{}", e.name, field.name);
                    if let Some(byte_offset) = field.byte_offset {
                        out.push(ParsedArgByteOffset {
                            path: field_path.clone(),
                            byte_offset,
                        });
                    }
                    collect_value_offsets(&field.value, &field_path, out);
                }
            }
        },
        ParsedArgValue::Amount(_)
        | ParsedArgValue::Boolean(_)
        | ParsedArgValue::Bytes(_)
        | ParsedArgValue::DateTime(_)
        | ParsedArgValue::Number(_)
        | ParsedArgValue::PublicKey(_)
        | ParsedArgValue::String(_) => {}
    }
}
