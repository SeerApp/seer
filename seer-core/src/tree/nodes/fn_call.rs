use serde::{Deserialize, Serialize};
use solana_instruction_error::InstructionError;

use crate::{
    idl::IdlTreeParser,
    tree::{
        loc::Loc,
        nodes::{
            account::TreeAccount,
            entrypoint::{EntrypointChildren, EntrypointViewChildren, TreeEntrypoint},
            error::TreeError,
            log::TreeLog,
            root::{RootChildren, RootViewChildren, TreeRoot},
        },
    },
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TreeFnCall<C> {
    #[serde(default)]
    pub step_order: u64,
    pub instruction: u64,
    pub signature: String,
    pub loc: Loc,
    pub children: Vec<C>,
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

impl PartialEq for TreeFnCall<FnCallChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
    }
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

#[derive(Serialize, Deserialize, PartialEq)]
pub enum FnCallViewChildren {
    Entrypoint(TreeEntrypoint<EntrypointViewChildren>),
    FnCall(TreeFnCall<FnCallViewChildren>),
    Log(TreeLog),
    Error(TreeError),
    Account(TreeAccount),
    Invoke(TreeRoot<RootViewChildren>),
}

impl PartialEq for TreeFnCall<FnCallViewChildren> {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.loc == other.loc
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

    pub fn has_fn_children(&self) -> bool {
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

    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        for c in &mut self.children {
            match c {
                FnCallChildren::Entrypoint(e) => {
                    e.parse(parser);
                }
                FnCallChildren::Account(a) => {
                    a.parse(parser);
                }
                FnCallChildren::Error(e) => {
                    e.parse(parser);
                }
                FnCallChildren::FnCall(f) => {
                    f.parse(parser);
                }
                _ => continue,
            }
        }
    }
}
