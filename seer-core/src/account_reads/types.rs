use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_pubkey::Pubkey;

use crate::idl::types::ParsedAccount;
use crate::tree::nodes::TreeAccountLoadKind;

/// One guest load attributed to an account pubkey and classified field/data offset.
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountRead {
    #[serde(default)]
    pub step_order: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub key: Pubkey,
    pub read_kind: TreeAccountLoadKind,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub owner_snapshot: Option<Pubkey>,
}

impl PartialEq for AccountRead {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.step_order == other.step_order
            && self.read_kind == other.read_kind
    }
}

// Backward-compatible aliases while call sites migrate.
pub type TreeAccountLoad = AccountRead;
pub type AccountReadKind = TreeAccountLoadKind;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedReadSpan {
    pub step_order: u64,
    pub step_order_end: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedDataRead {
    pub offset: usize,
    pub bytes_width: usize,
    pub step_order: u64,
    pub step_order_end: u64,
}

mod pubkey_string_serde {
    use serde::Serializer;
    use solana_pubkey::Pubkey;

    pub fn serialize<S>(value: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AggregatedAccountLoadKind {
    ReadKey {
        step_order: u64,
        step_order_end: u64,
    },
    ReadOwner {
        #[serde(with = "pubkey_string_serde")]
        owner: Pubkey,
        step_order: u64,
        step_order_end: u64,
    },
    ReadLamports {
        lamports: u64,
        step_order: u64,
        step_order_end: u64,
    },
    ReadDataLen {
        len: u64,
        step_order: u64,
    },
    ReadData {
        bytes: Vec<u8>,
        reads: Vec<AggregatedDataRead>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parsed: Option<ParsedAccount>,
    },
}

impl<'de> Deserialize<'de> for AggregatedAccountLoadKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(AggregatedAccountLoadKindVisitor)
    }
}

struct AggregatedAccountLoadKindVisitor;

impl<'de> Visitor<'de> for AggregatedAccountLoadKindVisitor {
    type Value = AggregatedAccountLoadKind;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("aggregated account load read_kind")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match v {
            "readKey" => Ok(AggregatedAccountLoadKind::ReadKey {
                step_order: 0,
                step_order_end: 0,
            }),
            other => Err(E::unknown_variant(other, &["readKey"])),
        }
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&v)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut collected: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            let value: serde_json::Value = map.next_value()?;
            collected.insert(key, value);
        }
        aggregated_account_load_kind_from_map(collected)
    }
}

fn aggregated_account_load_kind_from_map<E: de::Error>(
    map: BTreeMap<String, serde_json::Value>,
) -> Result<AggregatedAccountLoadKind, E> {
    if let Some(kind_val) = map.get("kind") {
        let kind = kind_val
            .as_str()
            .ok_or_else(|| E::custom("aggregated read_kind: \"kind\" must be a string"))?;
        return match kind {
            "readKey" => {
                let (step_order, step_order_end) = decode_step_range_from_map(&map);
                Ok(AggregatedAccountLoadKind::ReadKey {
                    step_order,
                    step_order_end,
                })
            }
            "readOwner" => {
                let owner_str = map.get("owner").and_then(|v| v.as_str()).ok_or_else(|| {
                    E::custom("aggregated read_kind readOwner: missing \"owner\"")
                })?;
                let owner = Pubkey::from_str(owner_str).map_err(E::custom)?;
                let (step_order, step_order_end) = decode_step_range_from_map(&map);
                Ok(AggregatedAccountLoadKind::ReadOwner {
                    owner,
                    step_order,
                    step_order_end,
                })
            }
            "readData" => {
                let (bytes, reads, parsed) = decode_read_data_kind_from_map(&map)?;
                Ok(AggregatedAccountLoadKind::ReadData {
                    bytes,
                    reads,
                    parsed,
                })
            }
            "readLamports" => {
                let lamports = map
                    .get("lamports")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| E::custom("aggregated read_kind readLamports: missing \"lamports\""))?;
                let (step_order, step_order_end) = decode_step_range_from_map(&map);
                Ok(AggregatedAccountLoadKind::ReadLamports {
                    lamports,
                    step_order,
                    step_order_end,
                })
            }
            "readDataLen" => {
                let len = map
                    .get("len")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| E::custom("aggregated read_kind readDataLen: missing \"len\""))?;
                let step_order = decode_step_order_only_from_map(&map);
                Ok(AggregatedAccountLoadKind::ReadDataLen { len, step_order })
            }
            other => Err(E::unknown_variant(
                other,
                &[
                    "readKey",
                    "readOwner",
                    "readData",
                    "readLamports",
                    "readDataLen",
                ],
            )),
        };
    }

    if let Some(v) = map.get("readKey") {
        if v.is_null() || v.as_object().map(|o| o.is_empty()).unwrap_or(false) {
            return Ok(AggregatedAccountLoadKind::ReadKey {
                step_order: 0,
                step_order_end: 0,
            });
        }
    }
    if let Some(v) = map.get("readOwner") {
        let inner = v
            .as_object()
            .ok_or_else(|| E::custom("aggregated read_kind readOwner: expected object"))?;
        let owner_str = inner
            .get("owner")
            .and_then(|x| x.as_str())
            .ok_or_else(|| E::custom("aggregated read_kind readOwner: missing owner"))?;
        let owner = Pubkey::from_str(owner_str).map_err(E::custom)?;
        return Ok(AggregatedAccountLoadKind::ReadOwner {
            owner,
            step_order: 0,
            step_order_end: 0,
        });
    }
    if let Some(v) = map.get("readData") {
        let inner: &serde_json::Map<String, serde_json::Value> = v
            .as_object()
            .ok_or_else(|| E::custom("aggregated read_kind readData: expected object"))?;
        let inner_map: BTreeMap<String, serde_json::Value> =
            inner.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let (bytes, reads, parsed) = decode_read_data_kind_from_map(&inner_map)?;
        return Ok(AggregatedAccountLoadKind::ReadData {
            bytes,
            reads,
            parsed,
        });
    }

    Err(E::custom(
        "aggregated read_kind: expected {\"kind\": ...} or legacy readKey/readOwner/readData",
    ))
}

fn decode_step_range_from_map(map: &BTreeMap<String, serde_json::Value>) -> (u64, u64) {
    let start = map
        .get("stepOrder")
        .or_else(|| map.get("step_order"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let end = map
        .get("stepOrderEnd")
        .or_else(|| map.get("step_order_end"))
        .and_then(|v| v.as_u64())
        .unwrap_or(start);
    normalize_step_range(start, end)
}

fn decode_step_order_only_from_map(map: &BTreeMap<String, serde_json::Value>) -> u64 {
    map.get("stepOrder")
        .or_else(|| map.get("step_order"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

fn decode_read_data_fields_from_map<E: de::Error>(
    map: &BTreeMap<String, serde_json::Value>,
) -> Result<(usize, usize, Vec<u8>), E> {
    let (offset, bytes_width) = decode_read_data_span_meta_from_map(map)?;
    let bytes_arr = map
        .get("bytes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| E::custom("readData: missing bytes"))?;
    let mut bytes = Vec::with_capacity(bytes_arr.len());
    for n in bytes_arr {
        let x = n
            .as_u64()
            .ok_or_else(|| E::custom("readData: non-integer byte"))?;
        let b = u8::try_from(x).map_err(|_| E::custom("readData: byte out of range"))?;
        bytes.push(b);
    }
    Ok((offset, bytes_width, bytes))
}

fn decode_read_data_span_meta_from_map<E: de::Error>(
    map: &BTreeMap<String, serde_json::Value>,
) -> Result<(usize, usize), E> {
    let offset = map
        .get("offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| E::custom("readData: missing offset"))? as usize;
    let bytes_width = map
        .get("bytesWidth")
        .or_else(|| map.get("bytes_width"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| E::custom("readData: missing bytesWidth"))? as usize;
    Ok((offset, bytes_width))
}

fn decode_read_data_kind_from_map<E: de::Error>(
    map: &BTreeMap<String, serde_json::Value>,
) -> Result<(Vec<u8>, Vec<AggregatedDataRead>, Option<ParsedAccount>), E> {
    let bytes = map
        .get("bytes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            let mut out = Vec::with_capacity(arr.len());
            for n in arr {
                let x = n
                    .as_u64()
                    .ok_or_else(|| E::custom("readData: non-integer byte"))?;
                let b = u8::try_from(x).map_err(|_| E::custom("readData: byte out of range"))?;
                out.push(b);
            }
            Ok::<Vec<u8>, E>(out)
        })
        .transpose()?
        .unwrap_or_default();

    if let Some(reads_val) = map.get("reads") {
        let reads_arr = reads_val
            .as_array()
            .ok_or_else(|| E::custom("readData: reads must be an array"))?;
        let mut reads = Vec::with_capacity(reads_arr.len());
        for r in reads_arr {
            let obj = r
                .as_object()
                .ok_or_else(|| E::custom("readData: each reads item must be object"))?;
            let obj_map: BTreeMap<String, serde_json::Value> =
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let (offset, bytes_width) = decode_read_data_span_meta_from_map(&obj_map)?;
            let (step_order, step_order_end) = decode_step_range_from_map(&obj_map);
            reads.push(AggregatedDataRead {
                offset,
                bytes_width,
                step_order,
                step_order_end,
            });
        }
        let parsed = map
            .get("parsed")
            .cloned()
            .map(serde_json::from_value::<ParsedAccount>)
            .transpose()
            .map_err(E::custom)?;
        return Ok((bytes, reads, parsed));
    }

    let (offset, bytes_width, _span_bytes) = decode_read_data_fields_from_map(map)?;
    let (step_order, step_order_end) = decode_step_range_from_map(map);
    let parsed = map
        .get("parsed")
        .cloned()
        .map(serde_json::from_value::<ParsedAccount>)
        .transpose()
        .map_err(E::custom)?;
    Ok((
        bytes,
        vec![AggregatedDataRead {
            offset,
            bytes_width,
            step_order,
            step_order_end,
        }],
        parsed,
    ))
}

#[serde_as]
#[derive(Deserialize)]
struct TreeAccountLoadAggregatedWire {
    #[serde(default)]
    step_order: Option<u64>,
    #[serde(default)]
    step_order_end: u64,
    #[serde_as(as = "DisplayFromStr")]
    key: Pubkey,
    read_kind: AggregatedAccountLoadKind,
}

impl From<TreeAccountLoadAggregatedWire> for TreeAccountLoadAggregated {
    fn from(w: TreeAccountLoadAggregatedWire) -> Self {
        let read_kind = match w.read_kind {
            AggregatedAccountLoadKind::ReadKey {
                step_order,
                step_order_end,
            } => {
                let (start, end) = normalize_step_range(
                    w.step_order.unwrap_or(step_order),
                    if w.step_order.is_some() { w.step_order_end } else { step_order_end },
                );
                AggregatedAccountLoadKind::ReadKey {
                    step_order: start,
                    step_order_end: end,
                }
            }
            AggregatedAccountLoadKind::ReadOwner {
                owner,
                step_order,
                step_order_end,
            } => {
                let (start, end) = normalize_step_range(
                    w.step_order.unwrap_or(step_order),
                    if w.step_order.is_some() { w.step_order_end } else { step_order_end },
                );
                AggregatedAccountLoadKind::ReadOwner {
                    owner,
                    step_order: start,
                    step_order_end: end,
                }
            }
            AggregatedAccountLoadKind::ReadLamports {
                lamports,
                step_order,
                step_order_end,
            } => {
                let (start, end) = normalize_step_range(
                    w.step_order.unwrap_or(step_order),
                    if w.step_order.is_some() { w.step_order_end } else { step_order_end },
                );
                AggregatedAccountLoadKind::ReadLamports {
                    lamports,
                    step_order: start,
                    step_order_end: end,
                }
            }
            AggregatedAccountLoadKind::ReadDataLen { len, step_order } => {
                AggregatedAccountLoadKind::ReadDataLen {
                    len,
                    step_order: w.step_order.unwrap_or(step_order),
                }
            }
            AggregatedAccountLoadKind::ReadData { bytes, mut reads, parsed } => {
                for read in &mut reads {
                    let (start, end) = normalize_step_range(read.step_order, read.step_order_end);
                    read.step_order = start;
                    read.step_order_end = end;
                }
                AggregatedAccountLoadKind::ReadData { bytes, reads, parsed }
            }
        };
        Self {
            key: w.key,
            read_kind,
        }
    }
}

pub fn normalize_step_range(step_order: u64, step_order_end: u64) -> (u64, u64) {
    if step_order <= step_order_end {
        (step_order, step_order_end)
    } else {
        (step_order_end, step_order)
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(from = "TreeAccountLoadAggregatedWire")]
pub struct TreeAccountLoadAggregated {
    #[serde_as(as = "DisplayFromStr")]
    pub key: Pubkey,
    pub read_kind: AggregatedAccountLoadKind,
}

pub type AccountReadAggregateKind = AggregatedAccountLoadKind;
pub type AccountReadAggregated = TreeAccountLoadAggregated;

/// Raw account load attributed to the program tree (`tree_uid`) that produced it.
#[derive(Clone, Debug, PartialEq)]
pub struct TaggedAccountLoad {
    pub tree_uid: u64,
    pub load: TreeAccountLoad,
}

/// Flattened account read row plus originating program tree uid (for CPI-aware chunking).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TaggedAccountLoadAggregated {
    pub tree_uid: u64,
    #[serde(flatten)]
    pub aggregate: TreeAccountLoadAggregated,
}

pub fn aggregated_account_read_start_step(agg: &TreeAccountLoadAggregated) -> u64 {
    match &agg.read_kind {
        AggregatedAccountLoadKind::ReadKey { step_order, .. } => *step_order,
        AggregatedAccountLoadKind::ReadOwner { step_order, .. } => *step_order,
        AggregatedAccountLoadKind::ReadLamports { step_order, .. } => *step_order,
        AggregatedAccountLoadKind::ReadDataLen { step_order, .. } => *step_order,
        AggregatedAccountLoadKind::ReadData { reads, .. } => {
            reads.iter().map(|r| r.step_order).min().unwrap_or(0)
        }
    }
}
