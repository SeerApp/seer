use std::collections::{hash_map::Entry, HashMap};

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_instruction_error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    idl::{types::ParsedInstruction, IdlTreeParser},
    program_manager::known_programs::SYSTEM_PROGRAM_PUBKEY,
    tree::nodes::{
        account::TreeAccount,
        entrypoint::{EntrypointChildren, EntrypointViewChildren, TreeEntrypoint},
        error::TreeError,
        fn_call::{FnCallChildren, FnCallViewChildren},
        log::TreeLog,
    },
};

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct TreeRoot<C> {
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

impl TreeRoot<RootChildren> {
    pub fn clone_into_view(
        source_index: usize,
        source_roots: &Vec<TreeRoot<RootChildren>>,
    ) -> TreeRoot<RootViewChildren> {
        let root = &source_roots[source_index];

        let mut tree_view: TreeRoot<RootViewChildren> = TreeRoot {
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

    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        self.parsed = parser.get_instruction(&self.data);
        for c in &mut self.children {
            match c {
                RootChildren::Entrypoint(e) => {
                    e.parse(parser);
                }
                RootChildren::Account(a) => {
                    a.parse(parser);
                }
                RootChildren::Error(e) => {
                    e.parse(parser);
                }
                _ => continue,
            }
        }
    }
}

impl TreeRoot<RootViewChildren> {
    fn insert_account_receiver(
        map: &mut HashMap<Pubkey, (Pubkey, u32)>,
        account_key: Pubkey,
        receiver: Pubkey,
        depth: u32,
    ) {
        if receiver == *SYSTEM_PROGRAM_PUBKEY {
            return;
        }
        match map.entry(account_key) {
            Entry::Vacant(v) => {
                v.insert((receiver, depth));
            }
            Entry::Occupied(mut o) => {
                let (r_old, d_old) = *o.get();
                if r_old == *SYSTEM_PROGRAM_PUBKEY && receiver != *SYSTEM_PROGRAM_PUBKEY {
                    o.insert((receiver, depth));
                    return;
                }
                if depth > d_old {
                    o.insert((receiver, depth));
                }
            }
        }
    }

    fn walk_root_view_children(
        children: &[RootViewChildren],
        receiver: Pubkey,
        depth: u32,
        map: &mut HashMap<Pubkey, (Pubkey, u32)>,
    ) {
        for child in children {
            match child {
                RootViewChildren::Account(a) => {
                    Self::insert_account_receiver(map, a.key, receiver, depth);
                }
                RootViewChildren::Invoke(inner) => Self::walk_root_view_children(
                    &inner.children,
                    inner.receiver,
                    depth.saturating_add(1),
                    map,
                ),
                RootViewChildren::Entrypoint(e) => {
                    Self::walk_entrypoint_view_children(&e.children, receiver, depth, map);
                }
                RootViewChildren::Log(_) | RootViewChildren::Error(_) => {}
            }
        }
    }

    fn walk_entrypoint_view_children(
        children: &[EntrypointViewChildren],
        receiver: Pubkey,
        depth: u32,
        map: &mut HashMap<Pubkey, (Pubkey, u32)>,
    ) {
        for child in children {
            match child {
                EntrypointViewChildren::Account(a) => {
                    Self::insert_account_receiver(map, a.key, receiver, depth);
                }
                EntrypointViewChildren::Invoke(inner) => Self::walk_root_view_children(
                    &inner.children,
                    inner.receiver,
                    depth.saturating_add(1),
                    map,
                ),
                EntrypointViewChildren::Entrypoint(e) => {
                    Self::walk_entrypoint_view_children(&e.children, receiver, depth, map);
                }
                EntrypointViewChildren::FnCall(f) => {
                    Self::walk_fn_call_view_children(&f.children, receiver, depth, map);
                }
                EntrypointViewChildren::Log(_) | EntrypointViewChildren::Error(_) => {}
            }
        }
    }

    fn walk_fn_call_view_children(
        children: &[FnCallViewChildren],
        receiver: Pubkey,
        depth: u32,
        map: &mut HashMap<Pubkey, (Pubkey, u32)>,
    ) {
        for child in children {
            match child {
                FnCallViewChildren::Account(a) => {
                    Self::insert_account_receiver(map, a.key, receiver, depth);
                }
                FnCallViewChildren::Invoke(inner) => Self::walk_root_view_children(
                    &inner.children,
                    inner.receiver,
                    depth.saturating_add(1),
                    map,
                ),
                FnCallViewChildren::Entrypoint(e) => {
                    Self::walk_entrypoint_view_children(&e.children, receiver, depth, map);
                }
                FnCallViewChildren::FnCall(f) => {
                    Self::walk_fn_call_view_children(&f.children, receiver, depth, map);
                }
                FnCallViewChildren::Log(_) | FnCallViewChildren::Error(_) => {}
            }
        }
    }

    /// Maps each account pubkey seen under a non-system program `receiver` in the finalized view
    /// tree to the IDL owner (`receiver`) for that subtree. Deeper nested `Invoke` receivers win on
    /// collision.
    pub fn account_pubkey_to_idl_receiver_map(&self) -> HashMap<Pubkey, Pubkey> {
        let mut with_depth: HashMap<Pubkey, (Pubkey, u32)> = HashMap::new();
        Self::walk_root_view_children(&self.children, self.receiver, 0, &mut with_depth);
        with_depth
            .into_iter()
            .map(|(k, (recv, _))| (k, recv))
            .collect()
    }
}

#[cfg(test)]
mod account_pubkey_to_idl_receiver_map_tests {
    use solana_account::AccountSharedData;
    use solana_pubkey::Pubkey;

    use crate::{
        program_manager::known_programs::SYSTEM_PROGRAM_PUBKEY,
        tree::nodes::account::AccountSharedDataWrapper,
    };

    use super::{RootViewChildren, TreeAccount, TreeRoot};

    fn dummy_account(key: Pubkey) -> TreeAccount {
        let empty = AccountSharedData::default();
        TreeAccount {
            step_order: 0,
            key,
            before: AccountSharedDataWrapper::from(empty.clone()),
            after: AccountSharedDataWrapper::from(empty),
        }
    }

    #[test]
    fn skips_system_program_receiver() {
        let k = Pubkey::new_unique();
        let tree = TreeRoot {
            step_order: 0,
            sender: Pubkey::new_unique(),
            receiver: *SYSTEM_PROGRAM_PUBKEY,
            accounts: vec![],
            data: vec![],
            children: vec![RootViewChildren::Account(dummy_account(k))],
            parsed: None,
        };
        let m = tree.account_pubkey_to_idl_receiver_map();
        assert!(!m.contains_key(&k));
    }

    #[test]
    fn nested_invoke_prefers_inner_receiver() {
        let outer = Pubkey::new_unique();
        let inner = Pubkey::new_unique();
        let acc_k = Pubkey::new_unique();

        let inner_tree = TreeRoot {
            step_order: 1,
            sender: Pubkey::new_unique(),
            receiver: inner,
            accounts: vec![],
            data: vec![],
            children: vec![RootViewChildren::Account(dummy_account(acc_k))],
            parsed: None,
        };

        let tree = TreeRoot {
            step_order: 0,
            sender: Pubkey::new_unique(),
            receiver: outer,
            accounts: vec![],
            data: vec![],
            children: vec![RootViewChildren::Invoke(inner_tree)],
            parsed: None,
        };

        let m = tree.account_pubkey_to_idl_receiver_map();
        assert_eq!(m.get(&acc_k), Some(&inner));
    }

    #[test]
    fn outer_account_maps_before_inner_on_same_key_last_write_wins_by_depth() {
        let outer = Pubkey::new_unique();
        let inner = Pubkey::new_unique();
        let acc_k = Pubkey::new_unique();

        let inner_tree = TreeRoot {
            step_order: 2,
            sender: Pubkey::new_unique(),
            receiver: inner,
            accounts: vec![],
            data: vec![],
            children: vec![RootViewChildren::Account(dummy_account(acc_k))],
            parsed: None,
        };

        let tree = TreeRoot {
            step_order: 0,
            sender: Pubkey::new_unique(),
            receiver: outer,
            accounts: vec![],
            data: vec![],
            children: vec![
                RootViewChildren::Account(dummy_account(acc_k)),
                RootViewChildren::Invoke(inner_tree),
            ],
            parsed: None,
        };

        let m = tree.account_pubkey_to_idl_receiver_map();
        assert_eq!(m.get(&acc_k), Some(&inner));
    }
}
