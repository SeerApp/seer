use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;

use crate::{
    account_reads::types::{normalize_step_range, TaggedAccountLoad, TaggedAccountLoadAggregated},
    dwarf::source_die::{SourceDie, SourceDieType},
    idl::IdlTreeParser,
    idl::types::{ParsedAccount, ParsedInstruction},
    program_manager::program_manager::ProgramManager,
    tree::loc::Loc,
};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_instruction_error::InstructionError;
use solana_program::clock::Epoch;
use solana_pubkey::Pubkey;

pub use crate::account_reads::types::TreeAccountLoad;
pub use crate::account_reads::types::{
    AggregatedAccountLoadKind, AggregatedDataRead, AggregatedReadSpan, TreeAccountLoadAggregated,
};

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct TreeRoot<C> {
    #[serde(default)]
    pub uid: u64,
    #[serde(default)]
    pub step_order: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub sender: Pubkey,
    #[serde_as(as = "DisplayFromStr")]
    pub receiver: Pubkey,
    #[serde_as(as = "Vec<DisplayFromStr>")]
    pub accounts: Vec<Pubkey>,
    pub data: Vec<u8>,
    pub children: Vec<C>,
    pub parsed: Option<ParsedInstruction>,
}

impl<C: PartialEq> PartialEq for TreeRoot<C> {
    fn eq(&self, other: &Self) -> bool {
        self.sender == other.sender
            && self.receiver == other.receiver
            && self.accounts == other.accounts
            && self.data == other.data
            && self.children == other.children
            && self.parsed == other.parsed
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TreeEntrypoint<C> {
    #[serde(default)]
    pub step_order: u64,
    pub instruction: u64,
    pub signature: String,
    pub loc: Loc,
    pub children: Vec<C>,
}

impl PartialEq for TreeEntrypoint<EntrypointChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
    }
}

impl PartialEq for TreeEntrypoint<EntrypointViewChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TreeFnCall<C> {
    #[serde(default)]
    pub step_order: u64,
    pub instruction: u64,
    pub signature: String,
    pub loc: Loc,
    pub children: Vec<C>,
}

impl PartialEq for TreeFnCall<FnCallChildren> {
    fn eq(&self, other: &Self) -> bool {
        let result = self.signature == other.signature && self.loc == other.loc;

        result
    }
}

impl PartialEq for TreeFnCall<FnCallViewChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TreeLog {
    #[serde(default)]
    pub step_order: u64,
    pub message: String,
}

impl TreeLog {
    pub fn new(message: String, step_order: u64) -> Self {
        Self {
            step_order,
            message,
        }
    }
}

impl PartialEq for TreeLog {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
    }
}

fn deserialize_tree_error_message<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Text(String),
        Error(InstructionError),
    }
    Ok(match Repr::deserialize(deserializer)? {
        Repr::Text(s) => s,
        Repr::Error(e) => e.to_string(),
    })
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TreeError {
    #[serde(default)]
    pub step_order: u64,
    /// Human-readable text from [`InstructionError`]; JSON is always a string on write. Reads accept
    /// either a string or legacy tagged `InstructionError` JSON (e.g. `{"Custom": 0}`).
    #[serde(deserialize_with = "deserialize_tree_error_message")]
    pub message: String,
    /// Not persisted on disk (covered by `message`). After `Deserialize`, this is a placeholder.
    #[serde(
        skip_serializing,
        skip_deserializing,
        default = "TreeError::deser_placeholder_error"
    )]
    pub instruction_error: InstructionError,
}

impl TreeError {
    fn deser_placeholder_error() -> InstructionError {
        InstructionError::Custom(0)
    }

    pub fn new(message: InstructionError, step_order: u64) -> Self {
        Self {
            step_order,
            instruction_error: message.clone(),
            message: message.to_string(),
        }
    }
}

impl PartialEq for TreeError {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TreeAccount {
    #[serde(default)]
    pub step_order: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub key: Pubkey,
    pub before: AccountSharedDataWrapper,
    pub after: AccountSharedDataWrapper,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AccountSharedDataWrapper {
    lamports: u64,
    data: Vec<u8>,
    #[serde_as(as = "DisplayFromStr")]
    owner: Pubkey,
    executable: bool,
    rent_epoch: Epoch,
    parsed: Option<ParsedAccount>,
}

impl AccountSharedDataWrapper {
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn owner(&self) -> Pubkey {
        self.owner
    }

    pub fn set_parsed(&mut self, parsed: Option<ParsedAccount>) {
        self.parsed = parsed;
    }
}

impl From<AccountSharedData> for AccountSharedDataWrapper {
    fn from(value: AccountSharedData) -> Self {
        AccountSharedDataWrapper {
            lamports: value.lamports(),
            data: value.data().to_vec(),
            owner: *value.owner(),
            executable: value.executable(),
            rent_epoch: value.rent_epoch(),
            parsed: None,
        }
    }
}

impl PartialEq for TreeAccount {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.before == other.before && self.after == other.after
    }
}

/// Classified read of serialized account state in the SBPF VM (interpreter).
///
/// JSON is **internally tagged** on `kind` (camelCase), for example:
/// `{ "kind": "lamports", "lamports": 4000845221 }`, `{ "kind": "key", "bytes": [...] }`.
/// Deserialization still accepts legacy externally tagged shapes (e.g. `{ "lamports": { "lamports": … } }`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TreeAccountLoadKind {
    Key {
        bytes: Vec<u8>,
    },
    Owner {
        bytes: Vec<u8>,
    },
    Lamports {
        lamports: u64,
    },
    DataLen {
        len: u64,
    },
    RentEpoch {
        rent_epoch: u64,
    },
    Executable {
        byte: u8,
    },
    Data {
        offset: usize,
        #[serde(default)]
        bytes_width: usize,
        #[serde(default)]
        bytes: Vec<u8>,
    },
}

impl<'de> Deserialize<'de> for TreeAccountLoadKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(TreeAccountLoadKindVisitor)
    }
}

struct TreeAccountLoadKindVisitor;

impl<'de> Visitor<'de> for TreeAccountLoadKindVisitor {
    type Value = TreeAccountLoadKind;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("raw account read_kind")
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
        tree_account_load_kind_from_map(collected)
    }
}

fn json_byte_array<E: de::Error>(v: &serde_json::Value) -> Result<Vec<u8>, E> {
    let arr = v
        .as_array()
        .ok_or_else(|| E::custom("read_kind: expected byte array"))?;
    let mut out = Vec::with_capacity(arr.len());
    for n in arr {
        let x = n
            .as_u64()
            .ok_or_else(|| E::custom("read_kind: non-integer byte"))?;
        let b = u8::try_from(x).map_err(|_| E::custom("read_kind: byte out of range"))?;
        out.push(b);
    }
    Ok(out)
}

fn json_usize<E: de::Error>(v: &serde_json::Value, ctx: &str) -> Result<usize, E> {
    let n = v
        .as_u64()
        .ok_or_else(|| E::custom(format!("{ctx}: expected non-negative integer")))?;
    usize::try_from(n).map_err(|_| E::custom(format!("{ctx}: value too large")))
}

fn tree_account_load_kind_from_map<E: de::Error>(
    map: BTreeMap<String, serde_json::Value>,
) -> Result<TreeAccountLoadKind, E> {
    if let Some(kind_val) = map.get("kind") {
        let kind = kind_val
            .as_str()
            .ok_or_else(|| E::custom("read_kind: \"kind\" must be a string"))?;
        return match kind {
            "key" => {
                let bytes = map
                    .get("bytes")
                    .map(json_byte_array)
                    .transpose()?
                    .unwrap_or_default();
                Ok(TreeAccountLoadKind::Key { bytes })
            }
            "owner" => {
                let bytes = map
                    .get("bytes")
                    .map(json_byte_array)
                    .transpose()?
                    .unwrap_or_default();
                Ok(TreeAccountLoadKind::Owner { bytes })
            }
            "lamports" => {
                let lamports = map
                    .get("lamports")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| E::custom("read_kind lamports: missing \"lamports\""))?;
                Ok(TreeAccountLoadKind::Lamports { lamports })
            }
            "dataLen" => {
                let len = map
                    .get("len")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| E::custom("read_kind dataLen: missing \"len\""))?;
                Ok(TreeAccountLoadKind::DataLen { len })
            }
            "rentEpoch" => {
                let rent_epoch = map
                    .get("rentEpoch")
                    .or_else(|| map.get("rent_epoch"))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| E::custom("read_kind rentEpoch: missing \"rentEpoch\""))?;
                Ok(TreeAccountLoadKind::RentEpoch { rent_epoch })
            }
            "executable" => {
                let byte = map
                    .get("byte")
                    .and_then(|v| v.as_u64())
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(|| E::custom("read_kind executable: missing \"byte\""))?;
                Ok(TreeAccountLoadKind::Executable { byte })
            }
            "data" => {
                let offset = map
                    .get("offset")
                    .map(|v| json_usize(v, "read_kind data.offset"))
                    .transpose()?
                    .ok_or_else(|| E::custom("read_kind data: missing \"offset\""))?;
                let bytes_width = map
                    .get("bytesWidth")
                    .or_else(|| map.get("bytes_width"))
                    .map(|v| json_usize(v, "read_kind data.bytesWidth"))
                    .transpose()?
                    .unwrap_or(0);
                let bytes = map
                    .get("bytes")
                    .map(json_byte_array)
                    .transpose()?
                    .unwrap_or_default();
                Ok(TreeAccountLoadKind::Data {
                    offset,
                    bytes_width,
                    bytes,
                })
            }
            other => Err(E::unknown_variant(
                other,
                &[
                    "key",
                    "owner",
                    "lamports",
                    "dataLen",
                    "rentEpoch",
                    "executable",
                    "data",
                ],
            )),
        };
    }

    // Legacy externally tagged JSON, e.g. `{ "lamports": { "lamports": … } }`.
    if let Some(v) = map.get("key") {
        let inner = v
            .as_object()
            .ok_or_else(|| E::custom("read_kind key: expected object"))?;
        let bytes = inner
            .get("bytes")
            .map(json_byte_array)
            .transpose()?
            .unwrap_or_default();
        return Ok(TreeAccountLoadKind::Key { bytes });
    }
    if let Some(v) = map.get("owner") {
        let inner = v
            .as_object()
            .ok_or_else(|| E::custom("read_kind owner: expected object"))?;
        let bytes = inner
            .get("bytes")
            .map(json_byte_array)
            .transpose()?
            .unwrap_or_default();
        return Ok(TreeAccountLoadKind::Owner { bytes });
    }
    if let Some(v) = map.get("lamports") {
        let lamports = if let Some(obj) = v.as_object() {
            obj.get("lamports")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| E::custom("read_kind lamports: missing inner lamports"))?
        } else {
            v.as_u64()
                .ok_or_else(|| E::custom("read_kind lamports: expected number or object"))?
        };
        return Ok(TreeAccountLoadKind::Lamports { lamports });
    }
    if let Some(v) = map.get("dataLen").or_else(|| map.get("data_len")) {
        let inner = v
            .as_object()
            .ok_or_else(|| E::custom("read_kind dataLen: expected object"))?;
        let len = inner
            .get("len")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| E::custom("read_kind dataLen: missing len"))?;
        return Ok(TreeAccountLoadKind::DataLen { len });
    }
    if let Some(v) = map.get("rentEpoch").or_else(|| map.get("rent_epoch")) {
        let rent_epoch = if let Some(obj) = v.as_object() {
            obj.get("rentEpoch")
                .or_else(|| obj.get("rent_epoch"))
                .and_then(|x| x.as_u64())
                .ok_or_else(|| E::custom("read_kind rentEpoch: missing inner field"))?
        } else {
            v.as_u64()
                .ok_or_else(|| E::custom("read_kind rentEpoch: expected number or object"))?
        };
        return Ok(TreeAccountLoadKind::RentEpoch { rent_epoch });
    }
    if let Some(v) = map.get("executable") {
        let inner = v
            .as_object()
            .ok_or_else(|| E::custom("read_kind executable: expected object"))?;
        let byte = inner
            .get("byte")
            .and_then(|x| x.as_u64())
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| E::custom("read_kind executable: missing byte"))?;
        return Ok(TreeAccountLoadKind::Executable { byte });
    }
    if let Some(v) = map.get("data") {
        let inner = v
            .as_object()
            .ok_or_else(|| E::custom("read_kind data: expected object"))?;
        let offset = inner
            .get("offset")
            .map(|x| json_usize(x, "read_kind data.offset"))
            .transpose()?
            .ok_or_else(|| E::custom("read_kind data: missing offset"))?;
        let bytes_width = inner
            .get("bytesWidth")
            .or_else(|| inner.get("bytes_width"))
            .map(|x| json_usize(x, "read_kind data.bytesWidth"))
            .transpose()?
            .unwrap_or(0);
        let bytes = inner
            .get("bytes")
            .map(json_byte_array)
            .transpose()?
            .unwrap_or_default();
        return Ok(TreeAccountLoadKind::Data {
            offset,
            bytes_width,
            bytes,
        });
    }

    Err(E::custom(
        "read_kind: expected {\"kind\": ...} or legacy key/owner/lamports/... tagged object",
    ))
}


#[derive(Clone, PartialEq)]
pub enum RootChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke { tree_index: usize, step_order: u64 },
}

#[derive(Serialize, Deserialize, PartialEq)]
pub enum RootViewChildren {
    Invoke(TreeRoot<RootViewChildren>),
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
}

#[derive(Debug, Clone)]
pub enum EntrypointChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    FnCall(TreeFnCall<FnCallChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke { tree_index: usize, step_order: u64 },
}

#[derive(Serialize, Deserialize, PartialEq)]
pub enum EntrypointViewChildren {
    Invoke(TreeRoot<RootViewChildren>),
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    FnCall(TreeFnCall<FnCallViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
}

#[derive(Debug, Clone)]
pub enum FnCallChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    FnCall(TreeFnCall<FnCallChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke { tree_index: usize, step_order: u64 },
}

#[derive(Serialize, Deserialize, PartialEq)]
pub enum FnCallViewChildren {
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    FnCall(TreeFnCall<FnCallViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke(TreeRoot<RootViewChildren>),
}

impl From<EntrypointChildren> for FnCallChildren {
    fn from(value: EntrypointChildren) -> Self {
        match value {
            EntrypointChildren::Account(a) => FnCallChildren::Account(a),
            EntrypointChildren::Log(l) => FnCallChildren::Log(l),
            EntrypointChildren::Error(err) => FnCallChildren::Error(err),
            EntrypointChildren::Invoke {
                tree_index,
                step_order,
            } => FnCallChildren::Invoke {
                tree_index,
                step_order,
            },
            EntrypointChildren::Entrypoint(e) => FnCallChildren::Entrypoint(e),
            EntrypointChildren::FnCall(f) => FnCallChildren::FnCall(f),
        }
    }
}

fn push_tagged_aggregate_merge(out: &mut Vec<TaggedAccountLoadAggregated>, next: TaggedAccountLoadAggregated) {
    if let Some(prev) = out.last_mut() {
        if prev.tree_uid == next.tree_uid
            && prev.aggregate.key == next.aggregate.key
            && try_merge_aggregated_read_kind(&mut prev.aggregate.read_kind, &next.aggregate.read_kind)
        {
            return;
        }
    }
    out.push(next);
}

fn tagged_data_run_head(loads: &[TaggedAccountLoad], start: usize) -> Option<(Pubkey, usize)> {
    let first = &loads[start].load;
    let TreeAccountLoadKind::Data { .. } = &first.read_kind else {
        return None;
    };
    let key = first.key;
    let mut len = 1usize;
    while start + len < loads.len() {
        let next = &loads[start + len].load;
        if next.key == key && matches!(next.read_kind, TreeAccountLoadKind::Data { .. }) {
            len += 1;
        } else {
            break;
        }
    }
    Some((key, len))
}

fn aggregate_data_run_tagged(
    run: &[TaggedAccountLoad],
    key: Pubkey,
    account_snapshots: &HashMap<Pubkey, AccountStateSnapshot>,
    program_manager: Option<&ProgramManager>,
) -> Vec<TreeAccountLoadAggregated> {
    let loads = run.iter().map(|t| &t.load);
    let data_loads = loads.filter(|load| matches!(load.read_kind, TreeAccountLoadKind::Data { .. }));
    aggregate_data_segments_for_key(collect_data_segments(data_loads), key, account_snapshots, program_manager)
}

pub fn finalize_invoke_account_read_aggregates(
    trees: &[TreeRoot<RootChildren>],
    loads: &[TaggedAccountLoad],
    program_manager: Option<&ProgramManager>,
) -> Vec<TaggedAccountLoadAggregated> {
    let mut account_snapshots: HashMap<Pubkey, AccountStateSnapshot> = HashMap::new();
    for tree in trees {
        collect_latest_account_after_by_key_root(&tree.children, &mut account_snapshots);
    }

    let mut tagged = loads.to_vec();
    tagged.sort_by(|a, b| {
        a.tree_uid
            .cmp(&b.tree_uid)
            .then(a.load.step_order.cmp(&b.load.step_order))
    });

    let mut out: Vec<TaggedAccountLoadAggregated> = Vec::new();
    let mut i = 0usize;
    while i < tagged.len() {
        if let Some((key, run_len)) = tagged_data_run_head(&tagged, i) {
            if run_len >= 2 {
                let run = &tagged[i..i + run_len];
                let aggs = aggregate_data_run_tagged(run, key, &account_snapshots, program_manager);
                if !aggs.is_empty() {
                    for agg in aggs {
                        push_tagged_aggregate_merge(
                            &mut out,
                            TaggedAccountLoadAggregated {
                                tree_uid: tagged[i].tree_uid,
                                aggregate: agg,
                            },
                        );
                    }
                    i += run_len;
                    continue;
                }
            }
        }
        if i + 4 <= tagged.len() {
            let four = [
                &tagged[i].load,
                &tagged[i + 1].load,
                &tagged[i + 2].load,
                &tagged[i + 3].load,
            ];
            if let Some(agg) = try_aggregate_four_raw_loads(four) {
                push_tagged_aggregate_merge(
                    &mut out,
                    TaggedAccountLoadAggregated {
                        tree_uid: tagged[i].tree_uid,
                        aggregate: agg,
                    },
                );
                i += 4;
                continue;
            }
        }
        let load = &tagged[i].load;
        if is_raw_key_or_owner_load(load) {
            i += 1;
            continue;
        }
        if let Some(agg) = try_scalar_raw_load_as_aggregated(load) {
            push_tagged_aggregate_merge(
                &mut out,
                TaggedAccountLoadAggregated {
                    tree_uid: tagged[i].tree_uid,
                    aggregate: agg,
                },
            );
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

impl TreeRoot<RootChildren> {
    /// Flattens *sequential sibling* `Account` diffs that target the same account `key`.
    ///
    /// For runs like `Account(k, before=a1, after=b1)` followed later by
    /// `Account(k, before=a2, after=b2)` (adjacent siblings only), we merge them into:
    /// `Account(k, before=a1, after=b2)`.
    ///
    /// This pass is applied recursively for all levels of nesting in the trace tree.
    pub fn flatten_account_diffs(&mut self) {
        Self::flatten_root_children(&mut self.children);
    }

    fn flatten_root_children(children: &mut Vec<RootChildren>) {
        let old_children = std::mem::take(children);
        let mut new_children: Vec<RootChildren> = Vec::with_capacity(old_children.len());

        for child in old_children {
            match child {
                RootChildren::Invoke {
                    tree_index,
                    step_order,
                } => new_children.push(RootChildren::Invoke {
                    tree_index,
                    step_order,
                }),
                RootChildren::Entrypoint(mut entrypoint) => {
                    Self::flatten_entrypoint_children(&mut entrypoint.children);
                    new_children.push(RootChildren::Entrypoint(entrypoint));
                }
                RootChildren::Account(acc) => {
                    if let Some(RootChildren::Account(prev)) = new_children.last_mut() {
                        if prev.key == acc.key {
                            // Keep the original "before" (from the first element in the run),
                            // but extend the "after" to the last element in the run.
                            prev.after = acc.after;
                            continue;
                        }
                    }

                    new_children.push(RootChildren::Account(acc));
                }
                RootChildren::Log(l) => new_children.push(RootChildren::Log(l)),
                RootChildren::Error(e) => new_children.push(RootChildren::Error(e)),
            }
        }

        *children = new_children;
    }

    fn flatten_entrypoint_children(children: &mut Vec<EntrypointChildren>) {
        let old_children = std::mem::take(children);
        let mut new_children: Vec<EntrypointChildren> = Vec::with_capacity(old_children.len());

        for child in old_children {
            match child {
                EntrypointChildren::Invoke {
                    tree_index,
                    step_order,
                } => {
                    new_children.push(EntrypointChildren::Invoke {
                        tree_index,
                        step_order,
                    });
                }
                EntrypointChildren::Entrypoint(mut entrypoint) => {
                    Self::flatten_entrypoint_children(&mut entrypoint.children);
                    new_children.push(EntrypointChildren::Entrypoint(entrypoint));
                }
                EntrypointChildren::FnCall(mut fn_call) => {
                    Self::flatten_fn_call_children(&mut fn_call.children);
                    new_children.push(EntrypointChildren::FnCall(fn_call));
                }
                EntrypointChildren::Account(acc) => {
                    if let Some(EntrypointChildren::Account(prev)) = new_children.last_mut() {
                        if prev.key == acc.key {
                            prev.after = acc.after;
                            continue;
                        }
                    }

                    new_children.push(EntrypointChildren::Account(acc));
                }
                EntrypointChildren::Log(l) => new_children.push(EntrypointChildren::Log(l)),
                EntrypointChildren::Error(e) => new_children.push(EntrypointChildren::Error(e)),
            }
        }

        *children = new_children;
    }

    fn flatten_fn_call_children(children: &mut Vec<FnCallChildren>) {
        let old_children = std::mem::take(children);
        let mut new_children: Vec<FnCallChildren> = Vec::with_capacity(old_children.len());

        for child in old_children {
            match child {
                FnCallChildren::Entrypoint(mut entrypoint) => {
                    Self::flatten_entrypoint_children(&mut entrypoint.children);
                    new_children.push(FnCallChildren::Entrypoint(entrypoint));
                }
                FnCallChildren::FnCall(mut fn_call) => {
                    Self::flatten_fn_call_children(&mut fn_call.children);
                    new_children.push(FnCallChildren::FnCall(fn_call));
                }
                FnCallChildren::Invoke {
                    tree_index,
                    step_order,
                } => {
                    new_children.push(FnCallChildren::Invoke {
                        tree_index,
                        step_order,
                    });
                }
                FnCallChildren::Account(acc) => {
                    if let Some(FnCallChildren::Account(prev)) = new_children.last_mut() {
                        if prev.key == acc.key {
                            prev.after = acc.after;
                            continue;
                        }
                    }

                    new_children.push(FnCallChildren::Account(acc));
                }
                FnCallChildren::Log(l) => new_children.push(FnCallChildren::Log(l)),
                FnCallChildren::Error(e) => new_children.push(FnCallChildren::Error(e)),
            }
        }

        *children = new_children;
    }

    /// Account reads are persisted to sidecar JSON; this is a no-op on the trace tree.
    pub fn flatten_account_loads(&mut self) {}

    /// See [`Self::flatten_account_loads`].
    pub fn flatten_account_loads_with_program_manager(&mut self, _program_manager: &ProgramManager) {}

}

fn merge_step_ranges(
    left_start: u64,
    left_end: u64,
    right_start: u64,
    right_end: u64,
) -> (u64, u64) {
    let (l0, l1) = normalize_step_range(left_start, left_end);
    let (r0, r1) = normalize_step_range(right_start, right_end);
    (l0.min(r0), l1.max(r1))
}

fn try_merge_aggregated_read_kind(
    left: &mut AggregatedAccountLoadKind,
    right: &AggregatedAccountLoadKind,
) -> bool {
    match (left, right) {
        (
            AggregatedAccountLoadKind::ReadKey {
                step_order: l_start,
                step_order_end: l_end,
            },
            AggregatedAccountLoadKind::ReadKey {
                step_order: r_start,
                step_order_end: r_end,
            },
        ) => {
            (*l_start, *l_end) = merge_step_ranges(*l_start, *l_end, *r_start, *r_end);
            true
        }
        (
            AggregatedAccountLoadKind::ReadOwner {
                owner: l_owner,
                step_order: l_start,
                step_order_end: l_end,
            },
            AggregatedAccountLoadKind::ReadOwner {
                owner: r_owner,
                step_order: r_start,
                step_order_end: r_end,
            },
        ) if l_owner == r_owner => {
            (*l_start, *l_end) = merge_step_ranges(*l_start, *l_end, *r_start, *r_end);
            true
        }
        (
            AggregatedAccountLoadKind::ReadLamports {
                lamports: l_lamports,
                step_order: l_start,
                step_order_end: l_end,
            },
            AggregatedAccountLoadKind::ReadLamports {
                lamports: r_lamports,
                step_order: r_start,
                step_order_end: r_end,
            },
        ) if l_lamports == r_lamports => {
            (*l_start, *l_end) = merge_step_ranges(*l_start, *l_end, *r_start, *r_end);
            true
        }
        (
            AggregatedAccountLoadKind::ReadDataLen {
                len: l_len,
                step_order: l_step,
            },
            AggregatedAccountLoadKind::ReadDataLen {
                len: r_len,
                step_order: r_step,
            },
        ) if l_len == r_len => {
            *l_step = (*l_step).min(*r_step);
            true
        }
        _ => false,
    }
}

/// Single-step raw loads of lamports or data length become one [`TreeAccountLoadAggregated`] row.
fn try_scalar_raw_load_as_aggregated(load: &TreeAccountLoad) -> Option<TreeAccountLoadAggregated> {
    let step_order = load.step_order;
    let step_order_end = load.step_order;
    let key = load.key;
    match &load.read_kind {
        TreeAccountLoadKind::Lamports { lamports } => Some(TreeAccountLoadAggregated {
            key,
            read_kind: AggregatedAccountLoadKind::ReadLamports {
                lamports: *lamports,
                step_order,
                step_order_end,
            },
        }),
        TreeAccountLoadKind::DataLen { len } => Some(TreeAccountLoadAggregated {
            key,
            read_kind: AggregatedAccountLoadKind::ReadDataLen {
                len: *len,
                step_order,
            },
        }),
        _ => None,
    }
}

fn try_aggregate_four_raw_loads(loads: [&TreeAccountLoad; 4]) -> Option<TreeAccountLoadAggregated> {
    let k = loads[0].key;
    if loads[1].key != k || loads[2].key != k || loads[3].key != k {
        return None;
    }

    let step_order = loads
        .iter()
        .map(|l| l.step_order)
        .min()
        .expect("four loads");
    let step_order_end = loads
        .iter()
        .map(|l| l.step_order)
        .max()
        .expect("four loads");

    let mut key_chunks: Vec<&[u8]> = Vec::with_capacity(4);
    let mut all_key = true;
    for l in &loads {
        match &l.read_kind {
            TreeAccountLoadKind::Key { bytes } => key_chunks.push(bytes.as_slice()),
            _ => {
                all_key = false;
                break;
            }
        }
    }
    if all_key && bytes_match_in_supported_orders(&key_chunks, k.as_ref()) {
        return Some(TreeAccountLoadAggregated {
            key: k,
            read_kind: AggregatedAccountLoadKind::ReadKey {
                step_order,
                step_order_end,
            },
        });
    }

    let mut owner_chunks: Vec<&[u8]> = Vec::with_capacity(4);
    let mut all_owner = true;
    for l in &loads {
        match &l.read_kind {
            TreeAccountLoadKind::Owner { bytes } => owner_chunks.push(bytes.as_slice()),
            _ => {
                all_owner = false;
                break;
            }
        }
    }
    if all_owner {
        let owner = loads[0].owner_snapshot?;
        if loads[1].owner_snapshot != Some(owner)
            || loads[2].owner_snapshot != Some(owner)
            || loads[3].owner_snapshot != Some(owner)
        {
            return None;
        }
        if bytes_match_in_supported_orders(&owner_chunks, owner.as_ref()) {
            return Some(TreeAccountLoadAggregated {
                key: k,
                read_kind: AggregatedAccountLoadKind::ReadOwner {
                    owner,
                    step_order,
                    step_order_end,
                },
            });
        }
    }

    None
}

fn bytes_match_in_supported_orders(chunks: &[&[u8]], expected: &[u8]) -> bool {
    let total_len: usize = chunks.iter().map(|chunk| chunk.len()).sum();
    if total_len != expected.len() {
        return false;
    }

    if chunks_match_expected_in_order(chunks, expected) {
        return true;
    }

    // Some VM/codegen paths read fixed-size fields in reverse chunk order.
    if chunks_match_expected_in_reverse_order(chunks, expected) {
        return true;
    }

    // For the common 4-load case (8+8+8+8 for key/owner), accept any chunk permutation.
    if chunks.len() == 4 {
        for order in CHUNK_PERMUTATIONS_4 {
            if chunks_match_expected_by_order(chunks, &order, expected) {
                return true;
            }
        }
    }

    false
}

fn is_raw_key_or_owner_load(load: &TreeAccountLoad) -> bool {
    matches!(
        load.read_kind,
        TreeAccountLoadKind::Key { .. } | TreeAccountLoadKind::Owner { .. }
    )
}

fn chunks_match_expected_in_order(chunks: &[&[u8]], expected: &[u8]) -> bool {
    let mut pos = 0usize;
    for chunk in chunks {
        let end = pos.saturating_add(chunk.len());
        if expected.get(pos..end) != Some(*chunk) {
            return false;
        }
        pos = end;
    }
    pos == expected.len()
}

fn chunks_match_expected_in_reverse_order(chunks: &[&[u8]], expected: &[u8]) -> bool {
    let mut pos = 0usize;
    for chunk in chunks.iter().rev() {
        let end = pos.saturating_add(chunk.len());
        if expected.get(pos..end) != Some(*chunk) {
            return false;
        }
        pos = end;
    }
    pos == expected.len()
}

fn chunks_match_expected_by_order(chunks: &[&[u8]], order: &[usize], expected: &[u8]) -> bool {
    let mut pos = 0usize;
    for &idx in order {
        let chunk = chunks[idx];
        let end = pos.saturating_add(chunk.len());
        if expected.get(pos..end) != Some(chunk) {
            return false;
        }
        pos = end;
    }
    pos == expected.len()
}

const CHUNK_PERMUTATIONS_4: [[usize; 4]; 24] = [
    [0, 1, 2, 3],
    [0, 1, 3, 2],
    [0, 2, 1, 3],
    [0, 2, 3, 1],
    [0, 3, 1, 2],
    [0, 3, 2, 1],
    [1, 0, 2, 3],
    [1, 0, 3, 2],
    [1, 2, 0, 3],
    [1, 2, 3, 0],
    [1, 3, 0, 2],
    [1, 3, 2, 0],
    [2, 0, 1, 3],
    [2, 0, 3, 1],
    [2, 1, 0, 3],
    [2, 1, 3, 0],
    [2, 3, 0, 1],
    [2, 3, 1, 0],
    [3, 0, 1, 2],
    [3, 0, 2, 1],
    [3, 1, 0, 2],
    [3, 1, 2, 0],
    [3, 2, 0, 1],
    [3, 2, 1, 0],
];

#[derive(Clone, Copy, Debug)]
struct DataReadSegmentRef<'a> {
    offset: usize,
    width: usize,
    bytes: &'a [u8],
    step_order: u64,
}

#[derive(Clone, Debug)]
struct AccountStateSnapshot {
    data: Vec<u8>,
    owner: Pubkey,
}

fn collect_latest_account_after_by_key_ep(
    children: &[EntrypointChildren],
    out: &mut HashMap<Pubkey, AccountStateSnapshot>,
) {
    for c in children {
        match c {
            EntrypointChildren::Account(a) => {
                out.insert(
                    a.key,
                    AccountStateSnapshot {
                        data: a.after.data().to_vec(),
                        owner: a.after.owner(),
                    },
                );
            }
            EntrypointChildren::Entrypoint(e) => {
                collect_latest_account_after_by_key_ep(&e.children, out);
            }
            EntrypointChildren::FnCall(f) => {
                collect_latest_account_after_by_key_fn(&f.children, out);
            }
            _ => {}
        }
    }
}

fn collect_latest_account_after_by_key_root(
    children: &[RootChildren],
    out: &mut HashMap<Pubkey, AccountStateSnapshot>,
) {
    for c in children {
        match c {
            RootChildren::Account(a) => {
                out.insert(
                    a.key,
                    AccountStateSnapshot {
                        data: a.after.data().to_vec(),
                        owner: a.after.owner(),
                    },
                );
            }
            RootChildren::Entrypoint(e) => {
                collect_latest_account_after_by_key_ep(&e.children, out);
            }
            _ => {}
        }
    }
}

fn collect_latest_account_after_by_key_fn(
    children: &[FnCallChildren],
    out: &mut HashMap<Pubkey, AccountStateSnapshot>,
) {
    for c in children {
        match c {
            FnCallChildren::Account(a) => {
                out.insert(
                    a.key,
                    AccountStateSnapshot {
                        data: a.after.data().to_vec(),
                        owner: a.after.owner(),
                    },
                );
            }
            FnCallChildren::Entrypoint(e) => {
                collect_latest_account_after_by_key_ep(&e.children, out);
            }
            FnCallChildren::FnCall(f) => {
                collect_latest_account_after_by_key_fn(&f.children, out);
            }
            _ => {}
        }
    }
}

fn collect_data_segments<'a, I>(loads: I) -> Vec<DataReadSegmentRef<'a>>
where
    I: IntoIterator<Item = &'a TreeAccountLoad>,
{
    let mut segments = Vec::new();
    for load in loads {
        if let TreeAccountLoadKind::Data {
            offset,
            bytes_width,
            bytes,
        } = &load.read_kind
        {
            let width = if *bytes_width > 0 {
                *bytes_width
            } else {
                bytes.len()
            };
            if width == 0 {
                continue;
            }
            segments.push(DataReadSegmentRef {
                offset: *offset,
                width,
                bytes: bytes.as_slice(),
                step_order: load.step_order,
            });
        }
    }
    segments
}

fn aggregate_data_segments_for_key(
    mut segments: Vec<DataReadSegmentRef<'_>>,
    key: Pubkey,
    account_snapshots: &HashMap<Pubkey, AccountStateSnapshot>,
    program_manager: Option<&ProgramManager>,
) -> Vec<TreeAccountLoadAggregated> {
    if segments.is_empty() {
        return Vec::new();
    }

    segments.sort_by(|a, b| {
        a.offset
            .cmp(&b.offset)
            .then(a.step_order.cmp(&b.step_order))
    });

    let mut reads: Vec<AggregatedDataRead> = Vec::new();
    let mut span_payloads: Vec<Vec<u8>> = Vec::new();
    let mut max_needed_len = 0usize;
    let mut idx = 0usize;
    while idx < segments.len() {
        let first = &segments[idx];
        let region_offset = first.offset;
        let mut region_end = first.offset.saturating_add(first.width);
        let mut region_bytes = vec![0u8; first.width];
        write_segment_bytes(&mut region_bytes, region_offset, first);
        let mut region_step_min = first.step_order;
        let mut region_step_max = first.step_order;
        idx += 1;

        while idx < segments.len() {
            let seg = &segments[idx];
            if seg.offset > region_end {
                break;
            }

            let seg_end = seg.offset.saturating_add(seg.width);
            if seg_end > region_end {
                region_bytes.resize(seg_end.saturating_sub(region_offset), 0);
                region_end = seg_end;
            }
            let write_start = seg.offset.saturating_sub(region_offset);
            let write_end = write_start.saturating_add(seg.width);
            if write_end <= region_bytes.len() {
                write_segment_bytes(&mut region_bytes, region_offset, seg);
            }
            region_step_min = region_step_min.min(seg.step_order);
            region_step_max = region_step_max.max(seg.step_order);
            idx += 1;
        }

        let read_span = region_end.saturating_sub(region_offset);
        let need_len = region_end.max(region_offset.saturating_add(region_bytes.len()));
        max_needed_len = max_needed_len.max(need_len);
        reads.push(AggregatedDataRead {
            offset: region_offset,
            bytes_width: read_span,
            step_order: region_step_min,
            step_order_end: region_step_max,
        });
        span_payloads.push(region_bytes);
    }

    let mut full_bytes = account_snapshots
        .get(&key)
        .map(|s| s.data.clone())
        .filter(|b| !b.is_empty())
        .unwrap_or_default();

    if full_bytes.len() < max_needed_len {
        full_bytes.resize(max_needed_len, 0);
    }

    if full_bytes.iter().all(|b| *b == 0) {
        for (read, span_bytes) in reads.iter().zip(span_payloads.iter()) {
            let end = read.offset.saturating_add(span_bytes.len());
            if end <= full_bytes.len() {
                full_bytes[read.offset..end].copy_from_slice(span_bytes.as_slice());
            }
        }
    }

    let parsed = program_manager.and_then(|pm| {
        let owner = account_snapshots.get(&key).map(|s| s.owner)?;
        let idl_lookup = pm.get_idl_lookup_cached(&owner)?;
        idl_lookup.get_account(&full_bytes)
    });

    vec![TreeAccountLoadAggregated {
        key,
        read_kind: AggregatedAccountLoadKind::ReadData {
            bytes: full_bytes,
            reads,
            parsed,
        },
    }]
}

fn write_segment_bytes(
    region_bytes: &mut [u8],
    region_offset: usize,
    seg: &DataReadSegmentRef<'_>,
) {
    let write_start = seg.offset.saturating_sub(region_offset);
    let write_end = write_start.saturating_add(seg.width);
    if write_end > region_bytes.len() {
        return;
    }
    region_bytes[write_start..write_end].fill(0);
    let copy_len = seg.bytes.len().min(seg.width);
    if copy_len > 0 {
        let copy_end = write_start.saturating_add(copy_len);
        region_bytes[write_start..copy_end].copy_from_slice(&seg.bytes[..copy_len]);
    }
}

impl TreeRoot<RootChildren> {
    pub fn clone_into_view(
        source_index: usize,
        source_roots: &Vec<TreeRoot<RootChildren>>,
    ) -> TreeRoot<RootViewChildren> {
        let root = &source_roots[source_index];

        let mut tree_view: TreeRoot<RootViewChildren> = TreeRoot {
            uid: root.uid,
            step_order: root.step_order,
            sender: root.sender,
            receiver: root.receiver,
            children: vec![],
            data: root.data.clone(),
            accounts: root.accounts.clone(),
            parsed: root.parsed.clone(),
        };

        for child in &root.children {
            match child {
                RootChildren::Entrypoint(e) => {
                    tree_view.children.push(RootViewChildren::Entrypoint(
                        TreeEntrypoint::clone_into_view(&e, source_roots),
                    ));
                }
                RootChildren::Invoke { tree_index, .. } => {
                    tree_view
                        .children
                        .push(RootViewChildren::Invoke(TreeRoot::clone_into_view(
                            *tree_index,
                            source_roots,
                        )));
                }
                RootChildren::Account(a) => tree_view
                    .children
                    .push(RootViewChildren::Account(a.clone())),
                RootChildren::Error(e) => {
                    tree_view.children.push(RootViewChildren::Error(e.clone()))
                }
                RootChildren::Log(l) => tree_view.children.push(RootViewChildren::Log(l.clone())),
            }
        }

        tree_view
    }

    pub fn push_err(&mut self, error_message: InstructionError, step_order: u64) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_err(error_message, step_order),
            _ => self.children.push(RootChildren::Error(TreeError::new(
                error_message,
                step_order,
            ))),
        }
    }

    pub fn push_entrypoint(&mut self, entrypoint: TreeEntrypoint<EntrypointChildren>) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => {
                e.push_entrypoint(entrypoint);
            }
            _ => {
                self.children.push(RootChildren::Entrypoint(entrypoint));
            }
        }
    }

    pub fn push_invoke(&mut self, new_tree_index: usize, step_order: u64) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_invoke(new_tree_index, step_order),
            _ => {
                self.children.push(RootChildren::Invoke {
                    tree_index: new_tree_index,
                    step_order,
                });
            }
        }
    }

    pub fn push_invoke_root(&mut self, new_tree_index: usize, step_order: u64) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_invoke_root(new_tree_index, step_order),
            _ => {
                self.children.push(RootChildren::Invoke {
                    tree_index: new_tree_index,
                    step_order,
                });
            }
        }
    }

    pub fn push_log(&mut self, message: String, step_order: u64) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_log(message, step_order),
            _ => self
                .children
                .push(RootChildren::Log(TreeLog::new(message, step_order))),
        }
    }

    pub fn push_account_diff(&mut self, data: TreeAccount) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_account_diff(data),
            _ => self.children.push(RootChildren::Account(data)),
        }
    }

}

impl TreeEntrypoint<EntrypointChildren> {
    pub fn clone_into_view(
        &self,
        source_roots: &Vec<TreeRoot<RootChildren>>,
    ) -> TreeEntrypoint<EntrypointViewChildren> {
        let mut tree_view: TreeEntrypoint<EntrypointViewChildren> = TreeEntrypoint {
            step_order: self.step_order,
            instruction: self.instruction,
            signature: self.signature.clone(),
            loc: self.loc.clone(),
            children: vec![],
        };

        for child in &self.children {
            match child {
                EntrypointChildren::Entrypoint(e) => {
                    tree_view.children.push(EntrypointViewChildren::Entrypoint(
                        TreeEntrypoint::clone_into_view(&e, source_roots),
                    ));
                }
                EntrypointChildren::FnCall(f) => tree_view.children.push(
                    EntrypointViewChildren::FnCall(TreeFnCall::clone_into_view(&f, source_roots)),
                ),
                EntrypointChildren::Invoke { tree_index, .. } => {
                    tree_view.children.push(EntrypointViewChildren::Invoke(
                        TreeRoot::clone_into_view(*tree_index, source_roots),
                    ));
                }
                EntrypointChildren::Account(a) => tree_view
                    .children
                    .push(EntrypointViewChildren::Account(a.clone())),
                EntrypointChildren::Error(e) => tree_view
                    .children
                    .push(EntrypointViewChildren::Error(e.clone())),
                EntrypointChildren::Log(l) => tree_view
                    .children
                    .push(EntrypointViewChildren::Log(l.clone())),
            }
        }

        tree_view
    }

    pub fn push_err(&mut self, error_message: InstructionError, step_order: u64) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => e.push_err(error_message, step_order),
            Some(EntrypointChildren::FnCall(f)) if f.has_fn_children() => {
                f.push_err(error_message, step_order);
            }
            _ => self.children.push(EntrypointChildren::Error(TreeError::new(
                error_message,
                step_order,
            ))),
        }
    }
    pub fn push_entrypoint(&mut self, entrypoint: TreeEntrypoint<EntrypointChildren>) {
        if *self == entrypoint {
            for new_child in entrypoint.children {
                match (self.children.last_mut(), new_child) {
                    (
                        Some(EntrypointChildren::Entrypoint(e)),
                        EntrypointChildren::Entrypoint(ce),
                    ) => {
                        e.push_entrypoint(ce);
                    }
                    (Some(EntrypointChildren::FnCall(f)), EntrypointChildren::FnCall(cf))
                        if *f == cf =>
                    {
                        f.push_fn_call(cf);
                    }
                    (_, other) => {
                        self.children.push(other);
                    }
                }
            }
        } else {
            match self.children.last_mut() {
                Some(EntrypointChildren::Entrypoint(e)) => {
                    e.push_entrypoint(entrypoint);
                }
                Some(EntrypointChildren::FnCall(f)) => {
                    f.push_entrypoint(entrypoint);
                }
                _ => {
                    self.children
                        .push(EntrypointChildren::Entrypoint(entrypoint));
                }
            }
        }
    }

    pub fn push_call_trace(&mut self, instruction: u64, mut call_trace: VecDeque<SourceDie>) {
        let mut fn_calls = Vec::new();
        let step_order = self.step_order;

        while let Some(source_die) = call_trace.pop_front() {
            match source_die.source_type {
                SourceDieType::Fn => {
                    if let Some(c) = source_die.loc.call {
                        fn_calls.push(TreeFnCall {
                            step_order,
                            instruction,
                            signature: source_die.loc.signature,
                            loc: c,
                            children: vec![],
                        });
                    }
                }
                _ => {}
            }
        }

        self.grow(instruction, &fn_calls, 0);
    }

    fn grow(
        &mut self,
        instruction: u64,
        call_trace: &Vec<TreeFnCall<FnCallChildren>>,
        counter: usize,
    ) {
        let Some(mut tree_fn_call) = call_trace.get(counter).cloned() else {
            return;
        };

        match self.children.last_mut() {
            Some(EntrypointChildren::FnCall(f)) if *f == tree_fn_call => {
                f.grow(instruction, call_trace, counter + 1);
            }
            _ => {
                tree_fn_call.grow(instruction, call_trace, counter + 1);
                self.children.push(EntrypointChildren::FnCall(tree_fn_call));
            }
        }
    }

    pub fn push_invoke(&mut self, new_tree_index: usize, step_order: u64) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => e.push_invoke(new_tree_index, step_order),
            Some(EntrypointChildren::FnCall(f)) => f.push_invoke(new_tree_index, step_order),
            _ => self.children.push(EntrypointChildren::Invoke {
                tree_index: new_tree_index,
                step_order,
            }),
        }
    }

    pub fn push_invoke_root(&mut self, new_tree_index: usize, step_order: u64) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => {
                e.push_invoke_root(new_tree_index, step_order)
            }
            Some(EntrypointChildren::FnCall(f)) if f.has_fn_children() => {
                f.push_invoke_root(new_tree_index, step_order)
            }
            _ => self.children.push(EntrypointChildren::Invoke {
                tree_index: new_tree_index,
                step_order,
            }),
        }
    }

    pub fn push_log(&mut self, message: String, step_order: u64) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => e.push_log(message, step_order),
            Some(EntrypointChildren::FnCall(f)) => f.push_log(message, step_order),
            _ => self
                .children
                .push(EntrypointChildren::Log(TreeLog::new(message, step_order))),
        }
    }

    pub fn push_account_diff(&mut self, data: TreeAccount) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => e.push_account_diff(data),
            Some(EntrypointChildren::FnCall(f)) => f.push_account_diff(data),
            _ => self.children.push(EntrypointChildren::Account(data)),
        }
    }

    pub fn is_superset_of(&self, entrypoint: &TreeEntrypoint<EntrypointChildren>) -> bool {
        if self != entrypoint {
            return false;
        }

        match (self.children.is_empty(), entrypoint.children.is_empty()) {
            (true, false) => false,
            (_, true) => !self.children.is_empty(),
            (false, false) => match (self.children.last(), entrypoint.children.last()) {
                (
                    Some(EntrypointChildren::Entrypoint(e)),
                    Some(EntrypointChildren::Entrypoint(ce)),
                ) => e.is_superset_of(ce),
                (Some(EntrypointChildren::FnCall(f)), Some(EntrypointChildren::FnCall(cf))) => {
                    f.is_superset_of(cf)
                }
                _ => false,
            },
        }
    }
}

impl TreeFnCall<FnCallChildren> {
    pub fn clone_into_view(
        &self,
        source_roots: &Vec<TreeRoot<RootChildren>>,
    ) -> TreeFnCall<FnCallViewChildren> {
        let mut tree_view: TreeFnCall<FnCallViewChildren> = TreeFnCall {
            step_order: self.step_order,
            instruction: self.instruction,
            signature: self.signature.clone(),
            loc: self.loc.clone(),
            children: vec![],
        };

        for child in &self.children {
            match child {
                FnCallChildren::Entrypoint(e) => {
                    tree_view.children.push(FnCallViewChildren::Entrypoint(
                        TreeEntrypoint::clone_into_view(&e, source_roots),
                    ));
                }
                FnCallChildren::FnCall(f) => tree_view.children.push(FnCallViewChildren::FnCall(
                    TreeFnCall::clone_into_view(&f, source_roots),
                )),
                FnCallChildren::Invoke { tree_index, .. } => {
                    tree_view
                        .children
                        .push(FnCallViewChildren::Invoke(TreeRoot::clone_into_view(
                            *tree_index,
                            source_roots,
                        )));
                }
                FnCallChildren::Account(a) => tree_view
                    .children
                    .push(FnCallViewChildren::Account(a.clone())),
                FnCallChildren::Log(l) => {
                    tree_view.children.push(FnCallViewChildren::Log(l.clone()))
                }
                FnCallChildren::Error(e) => tree_view
                    .children
                    .push(FnCallViewChildren::Error(e.clone())),
            };
        }

        tree_view
    }

    pub fn push_err(&mut self, error_message: InstructionError, step_order: u64) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_err(error_message, step_order),
            _ => self.children.push(FnCallChildren::Error(TreeError::new(
                error_message,
                step_order,
            ))),
        }
    }

    pub fn push_entrypoint(&mut self, entrypoint: TreeEntrypoint<EntrypointChildren>) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_entrypoint(entrypoint),
            Some(FnCallChildren::FnCall(f)) if f.has_fn_children() => f.push_entrypoint(entrypoint),
            _ => {
                self.children.push(FnCallChildren::Entrypoint(entrypoint));
            }
        }
    }

    pub fn push_fn_call(&mut self, fn_call: TreeFnCall<FnCallChildren>) {
        for new_child in fn_call.children {
            match (self.children.last_mut(), new_child) {
                (Some(FnCallChildren::Entrypoint(e)), FnCallChildren::Entrypoint(ce)) => {
                    e.push_entrypoint(ce);
                }
                (Some(FnCallChildren::FnCall(f)), FnCallChildren::FnCall(cf)) if *f == cf => {
                    f.push_fn_call(cf);
                }
                (_, other) => {
                    self.children.push(other);
                }
            }
        }
    }

    pub fn grow(
        &mut self,
        instruction: u64,
        call_trace: &Vec<TreeFnCall<FnCallChildren>>,
        counter: usize,
    ) {
        let Some(mut tree_fn_call) = call_trace.get(counter).cloned() else {
            return;
        };

        match self.children.last_mut() {
            Some(FnCallChildren::FnCall(f)) if *f == tree_fn_call => {
                f.grow(instruction, call_trace, counter + 1);
            }
            _ => {
                tree_fn_call.grow(instruction, call_trace, counter + 1);
                self.children.push(FnCallChildren::FnCall(tree_fn_call));
            }
        }
    }

    fn has_fn_children(&self) -> bool {
        self.children
            .iter()
            .any(|child| matches!(child, FnCallChildren::FnCall(_)))
    }

    pub fn push_invoke(&mut self, new_tree_index: usize, step_order: u64) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_invoke(new_tree_index, step_order),
            Some(FnCallChildren::FnCall(f)) => f.push_invoke(new_tree_index, step_order),
            _ => {
                self.children.push(FnCallChildren::Invoke {
                    tree_index: new_tree_index,
                    step_order,
                });
            }
        }
    }

    pub fn push_invoke_root(&mut self, new_tree_index: usize, step_order: u64) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_invoke_root(new_tree_index, step_order),
            Some(FnCallChildren::FnCall(f)) if f.has_fn_children() => {
                f.push_invoke_root(new_tree_index, step_order)
            }
            _ => {
                self.children.push(FnCallChildren::Invoke {
                    tree_index: new_tree_index,
                    step_order,
                });
            }
        }
    }

    pub fn push_log(&mut self, message: String, step_order: u64) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_log(message, step_order),
            Some(FnCallChildren::FnCall(f)) => f.push_log(message, step_order),
            _ => self
                .children
                .push(FnCallChildren::Log(TreeLog::new(message, step_order))),
        }
    }

    pub fn push_account_diff(&mut self, data: TreeAccount) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_account_diff(data),
            Some(FnCallChildren::FnCall(f)) => f.push_account_diff(data),
            _ => self.children.push(FnCallChildren::Account(data)),
        }
    }

    pub fn is_superset_of(&self, entrypoint: &TreeFnCall<FnCallChildren>) -> bool {
        if self != entrypoint {
            return false;
        }

        match (self.children.is_empty(), entrypoint.children.is_empty()) {
            (true, false) => false,
            (_, true) => !self.children.is_empty(),
            (false, false) => match (self.children.last(), entrypoint.children.last()) {
                (Some(FnCallChildren::Entrypoint(e)), Some(FnCallChildren::Entrypoint(ce))) => {
                    e.is_superset_of(ce)
                }
                (Some(FnCallChildren::FnCall(f)), Some(FnCallChildren::FnCall(cf))) => {
                    f.is_superset_of(cf)
                }
                _ => false,
            },
        }
    }
}
