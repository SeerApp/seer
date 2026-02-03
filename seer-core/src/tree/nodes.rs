use std::collections::VecDeque;

use crate::{
    dwarf::source_die::{SourceDie, SourceDieType},
    tree::loc::Loc,
};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_program::clock::Epoch;
use solana_pubkey::Pubkey;

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct TreeRoot<C> {
    #[serde_as(as = "DisplayFromStr")]
    pub sender: Pubkey,
    #[serde_as(as = "DisplayFromStr")]
    pub receiver: Pubkey,
    pub children: Vec<C>,
}

#[derive(Serialize, Deserialize)]
pub struct TreeEntrypoint<C> {
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

#[derive(Serialize, Deserialize)]
pub struct TreeFnCall<C> {
    pub instruction: u64,
    pub signature: String,
    pub loc: Loc,
    pub children: Vec<C>,
}

impl PartialEq for TreeFnCall<FnCallChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
    }
}

impl Clone for TreeFnCall<FnCallChildren> {
    fn clone(&self) -> Self {
        Self {
            instruction: self.instruction,
            signature: self.signature.clone(),
            loc: self.loc.clone(),
            children: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TreeLog {
    pub message: String,
}

impl TreeLog {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TreeError {
    pub message: String,
}

impl TreeError {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TreeAccount {
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
}

impl From<AccountSharedData> for AccountSharedDataWrapper {
    fn from(value: AccountSharedData) -> Self {
        AccountSharedDataWrapper {
            lamports: value.lamports(),
            data: value.data().to_vec(),
            owner: *value.owner(),
            executable: value.executable(),
            rent_epoch: value.rent_epoch(),
        }
    }
}

pub enum RootChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke(usize),
}

#[derive(Serialize, Deserialize)]
pub enum RootViewChildren {
    Invoke(TreeRoot<RootViewChildren>),
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
}

pub enum EntrypointChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    FnCall(TreeFnCall<FnCallChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke(usize),
}

#[derive(Serialize, Deserialize)]
pub enum EntrypointViewChildren {
    Invoke(TreeRoot<RootViewChildren>),
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    FnCall(TreeFnCall<FnCallViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
}

pub enum FnCallChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    FnCall(TreeFnCall<FnCallChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke(usize),
}

#[derive(Serialize, Deserialize)]
pub enum FnCallViewChildren {
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    FnCall(TreeFnCall<FnCallViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke(TreeRoot<RootViewChildren>),
}

impl TreeRoot<RootChildren> {
    pub fn clone_into_view(
        source_index: usize,
        source_roots: &Vec<TreeRoot<RootChildren>>,
    ) -> TreeRoot<RootViewChildren> {
        let root = &source_roots[source_index];

        let mut tree_view: TreeRoot<RootViewChildren> = TreeRoot {
            sender: root.sender,
            receiver: root.receiver,
            children: vec![],
        };

        for child in &root.children {
            match child {
                RootChildren::Entrypoint(e) => {
                    tree_view.children.push(RootViewChildren::Entrypoint(
                        TreeEntrypoint::clone_into_view(&e, source_roots),
                    ));
                }
                RootChildren::Invoke(index) => {
                    tree_view
                        .children
                        .push(RootViewChildren::Invoke(TreeRoot::clone_into_view(
                            index.clone(),
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

    pub fn push_err(&mut self, error_message: String) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_err(error_message),
            _ => self
                .children
                .push(RootChildren::Error(TreeError::new(error_message))),
        }
    }

    pub fn push_call_trace(&mut self, instruction: u64, mut call_trace: VecDeque<SourceDie>) {
        let maybe_entrypoint = 'find_entrypoint: {
            while let Some(source_die) = call_trace.pop_front() {
                match source_die.source_type {
                    SourceDieType::Fn => {
                        if let Some(d) = source_die.loc.decl {
                            break 'find_entrypoint Some(TreeEntrypoint {
                                instruction,
                                signature: source_die.loc.signature,
                                loc: d,
                                children: vec![],
                            });
                        }
                    }
                    _ => {}
                }
            }
            None
        };

        if let Some(entrypoint) = maybe_entrypoint {
            self.push_entrypoint(instruction, entrypoint, call_trace);
        }
    }

    fn push_entrypoint(
        &mut self,
        instruction: u64,
        mut entrypoint: TreeEntrypoint<EntrypointChildren>,
        call_trace: VecDeque<SourceDie>,
    ) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => {
                e.push_entrypoint(instruction, entrypoint, call_trace);
            }
            _ => {
                entrypoint.push_call_trace(instruction, call_trace);
                self.children.push(RootChildren::Entrypoint(entrypoint));
            }
        }
    }

    pub fn push_invoke(&mut self, new_tree_index: usize) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_invoke(new_tree_index),
            _ => {
                self.children.push(RootChildren::Invoke(new_tree_index));
            }
        }
    }

    pub fn push_log(&mut self, message: String) {
        match self.children.last_mut() {
            Some(RootChildren::Entrypoint(e)) => e.push_log(message),
            _ => self.children.push(RootChildren::Log(TreeLog::new(message))),
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
                EntrypointChildren::Invoke(index) => {
                    tree_view.children.push(EntrypointViewChildren::Invoke(
                        TreeRoot::clone_into_view(index.clone(), source_roots),
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

    pub fn push_err(&mut self, error_message: String) {
        match self.children.last_mut() {
            Some(EntrypointChildren::FnCall(f)) => f.push_err(error_message),
            _ => self
                .children
                .push(EntrypointChildren::Error(TreeError::new(error_message))),
        }
    }

    pub fn push_entrypoint(
        &mut self,
        instruction: u64,
        mut entrypoint: TreeEntrypoint<EntrypointChildren>,
        call_trace: VecDeque<SourceDie>,
    ) {
        if *self == entrypoint {
            self.push_call_trace(instruction, call_trace);
        } else {
            match self.children.last_mut() {
                Some(EntrypointChildren::Entrypoint(e)) => {
                    e.push_entrypoint(instruction, entrypoint, call_trace)
                }
                Some(EntrypointChildren::FnCall(f)) if f.has_fn_children() => {
                    f.push_entrypoint(instruction, entrypoint, call_trace)
                }
                _ => {
                    entrypoint.push_call_trace(instruction, call_trace);
                    self.children
                        .push(EntrypointChildren::Entrypoint(entrypoint));
                }
            }
        }
    }

    pub fn push_call_trace(&mut self, instruction: u64, mut call_trace: VecDeque<SourceDie>) {
        let mut fn_calls = Vec::new();

        if let Some(source_die) = call_trace.pop_front() {
            match source_die.source_type {
                SourceDieType::Fn => {
                    if let Some(c) = source_die.loc.call {
                        fn_calls.push(TreeFnCall {
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

    pub fn push_invoke(&mut self, new_tree_index: usize) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => e.push_invoke(new_tree_index),
            Some(EntrypointChildren::FnCall(f)) if f.has_fn_children() => {
                f.push_invoke(new_tree_index)
            }
            _ => self
                .children
                .push(EntrypointChildren::Invoke(new_tree_index)),
        }
    }

    pub fn push_log(&mut self, message: String) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => e.push_log(message),
            Some(EntrypointChildren::FnCall(f)) => f.push_log(message),
            _ => self
                .children
                .push(EntrypointChildren::Log(TreeLog::new(message))),
        }
    }

    pub fn push_account_diff(&mut self, data: TreeAccount) {
        match self.children.last_mut() {
            Some(EntrypointChildren::Entrypoint(e)) => e.push_account_diff(data),
            Some(EntrypointChildren::FnCall(f)) => f.push_account_diff(data),
            _ => self.children.push(EntrypointChildren::Account(data)),
        }
    }
}

impl TreeFnCall<FnCallChildren> {
    pub fn clone_into_view(
        &self,
        source_roots: &Vec<TreeRoot<RootChildren>>,
    ) -> TreeFnCall<FnCallViewChildren> {
        let mut tree_view: TreeFnCall<FnCallViewChildren> = TreeFnCall {
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
                FnCallChildren::Invoke(index) => {
                    tree_view
                        .children
                        .push(FnCallViewChildren::Invoke(TreeRoot::clone_into_view(
                            index.clone(),
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

    pub fn push_err(&mut self, error_message: String) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_err(error_message),
            _ => self
                .children
                .push(FnCallChildren::Error(TreeError::new(error_message))),
        }
    }

    pub fn push_entrypoint(
        &mut self,
        instruction: u64,
        mut entrypoint: TreeEntrypoint<EntrypointChildren>,
        call_trace: VecDeque<SourceDie>,
    ) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => {
                e.push_entrypoint(instruction, entrypoint, call_trace)
            }
            Some(FnCallChildren::FnCall(f)) if f.has_fn_children() => {
                f.push_entrypoint(instruction, entrypoint, call_trace)
            }
            _ => {
                entrypoint.push_call_trace(instruction, call_trace);
                self.children.push(FnCallChildren::Entrypoint(entrypoint));
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

    pub fn push_invoke(&mut self, new_tree_index: usize) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_invoke(new_tree_index),
            Some(FnCallChildren::FnCall(f)) if f.has_fn_children() => f.push_invoke(new_tree_index),
            _ => {
                self.children.push(FnCallChildren::Invoke(new_tree_index));
            }
        }
    }

    pub fn push_log(&mut self, message: String) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_log(message),
            Some(FnCallChildren::FnCall(f)) => f.push_log(message),
            _ => self
                .children
                .push(FnCallChildren::Log(TreeLog::new(message))),
        }
    }

    pub fn push_account_diff(&mut self, data: TreeAccount) {
        match self.children.last_mut() {
            Some(FnCallChildren::Entrypoint(e)) => e.push_account_diff(data),
            Some(FnCallChildren::FnCall(f)) => f.push_account_diff(data),
            _ => self.children.push(FnCallChildren::Account(data)),
        }
    }
}
