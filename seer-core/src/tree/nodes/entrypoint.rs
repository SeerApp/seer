use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use solana_instruction_error::InstructionError;

use crate::{
    dwarf::source_die::{SourceDie, SourceDieType},
    idl::IdlTreeParser,
    tree::{
        loc::Loc,
        nodes::{
            account::TreeAccount,
            error::TreeError,
            fn_call::{FnCallChildren, FnCallViewChildren, TreeFnCall},
            log::TreeLog,
            root::{RootChildren, RootViewChildren, TreeRoot},
        },
    },
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TreeEntrypoint<C> {
    #[serde(default)]
    pub step_order: u64,
    pub instruction: u64,
    pub signature: String,
    pub loc: Loc,
    pub children: Vec<C>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum EntrypointChildren {
    Entrypoint(TreeEntrypoint<EntrypointChildren>),
    FnCall(TreeFnCall<FnCallChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke { tree_index: usize, step_order: u64 },
}

impl PartialEq for TreeEntrypoint<EntrypointChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum EntrypointViewChildren {
    Invoke(TreeRoot<RootViewChildren>),
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    FnCall(TreeFnCall<FnCallViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
}

impl PartialEq for TreeEntrypoint<EntrypointViewChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
    }
}

impl TreeEntrypoint<EntrypointChildren> {
    pub fn clone_into_view(
        &self,
        source_roots: &[TreeRoot<RootChildren>],
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
                        TreeEntrypoint::clone_into_view(e, source_roots),
                    ));
                }
                EntrypointChildren::FnCall(f) => tree_view.children.push(
                    EntrypointViewChildren::FnCall(TreeFnCall::clone_into_view(f, source_roots)),
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
            if let SourceDieType::Fn = source_die.source_type {
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
        }

        self.grow(&fn_calls, 0);
    }

    fn grow(&mut self, call_trace: &[TreeFnCall<FnCallChildren>], counter: usize) {
        let Some(mut tree_fn_call) = call_trace.get(counter).cloned() else {
            return;
        };

        match self.children.last_mut() {
            Some(EntrypointChildren::FnCall(f)) if *f == tree_fn_call => {
                f.grow(call_trace, counter.saturating_add(1));
            }
            _ => {
                tree_fn_call.grow(call_trace, counter.saturating_add(1));
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

    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        for c in &mut self.children {
            match c {
                EntrypointChildren::Entrypoint(e) => {
                    e.parse(parser);
                }
                EntrypointChildren::Account(a) => {
                    a.parse(parser);
                }
                EntrypointChildren::Error(e) => {
                    e.parse(parser);
                }
                EntrypointChildren::FnCall(f) => {
                    f.parse(parser);
                }
                _ => continue,
            }
        }
    }
}
