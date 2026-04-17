use std::collections::VecDeque;

use crate::{
    dwarf::source_die::{SourceDie, SourceDieType},
    idl::types::{ParsedAccount, ParsedInstruction},
    tree::loc::Loc,
};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_instruction_error::InstructionError;
use solana_program::clock::Epoch;
use solana_pubkey::Pubkey;

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
    #[serde(skip_serializing, skip_deserializing, default = "TreeError::deser_placeholder_error")]
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

pub enum RootChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke {
        tree_index: usize,
        step_order: u64,
    },
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
    Invoke {
        tree_index: usize,
        step_order: u64,
    },
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
    Invoke {
        tree_index: usize,
        step_order: u64,
    },
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

impl From<&EntrypointChildren> for RootChildren {
    fn from(value: &EntrypointChildren) -> Self {
        match value {
            EntrypointChildren::Log(l) => RootChildren::Log(l.clone()),
            EntrypointChildren::Account(a) => RootChildren::Account(a.clone()),
            _ => panic!("Invalid conversion from EntrypointChildren to RootChildren"),
        }
    }
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
            Some(EntrypointChildren::Entrypoint(e)) => e.push_invoke_root(new_tree_index, step_order),
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
