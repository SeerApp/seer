use std::collections::HashMap;

use seer_interface::GuestMemory;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    call_trace_lookup::CallTraceLookup,
    tree::{
        entrypoint::EntrypointNode,
        is_non_dep_branch,
        view::{AccountData, EntrypointData, ErrorData, FnCallData, InvokeData, LogData, ViewNode},
        Tree, TreeContext, TreeNode, UniqueTree,
    },
};

#[derive(Clone, PartialEq, Debug)]
pub enum InvokeNode {
    Root,
    Entrypoint(EntrypointData),
    FnCall(FnCallData),
    Log(LogData),
    Error(ErrorData),
    Account(AccountData),
    Invoke(usize),
}

impl TreeNode for InvokeNode {
    fn root() -> Self {
        Self::Root
    }

    fn is_leaf(&self) -> bool {
        matches!(self, Self::Log(_) | Self::Error(_) | Self::Account(_))
    }

    fn is_fn_call(&self) -> bool {
        matches!(self, Self::FnCall(_))
    }

    fn can_push_to(&self, tree: &super::Tree<Self>) -> bool {
        match self {
            Self::Root => panic!("Cannot push root to a tree"),
            Self::Invoke(_) => is_non_dep_branch(tree),
            Self::Entrypoint(_) => !tree.node.is_leaf(),
            Self::FnCall(_) => !tree.node.is_leaf(),
            Self::Log(_) => !tree.node.is_leaf(),
            Self::Error(_) => !tree.node.is_leaf(),
            Self::Account(_) => !tree.node.is_leaf(),
        }
    }
}

impl TreeContext<InvokeData, InvokeNode, Option<TreeContext<EntrypointData, EntrypointNode>>> {
    pub fn new_invoke_context(sender: Pubkey, receiver: Pubkey) -> Self {
        let mut context = Self {
            trace: vec![],
            subtrees: vec![],
        };

        context.push_subtree(InvokeData::new(sender, receiver), None);

        context
    }

    pub fn start_program(&mut self, sender: Pubkey, receiver: Pubkey) {
        if let Some(entrypoint_context) = self.take_last_entrypoint_context() {
            self.materialize(entrypoint_context.into());
        }

        self.push_subtree(InvokeData::new(sender, receiver), None);
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        if let Some(entrypoint_context) = self.take_last_entrypoint_context() {
            self.materialize(entrypoint_context.into());
        }

        if let Some(msg) = err {
            self.get_current_unique_tree()
                .tree
                .push_leaf(InvokeNode::Error(ErrorData::new(msg.to_string())));
        }

        self.trace.pop();
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        lookups: &HashMap<Pubkey, CallTraceLookup>,
        i: u64,
        executed: &mut bool,
    ) {
        if let Some(lookup) = lookups.get(&self.get_last_receiver()) {
            let call_trace = lookup.get_call_trace(i);

            let current_trace = self
                .trace
                .last_mut()
                .expect("Stepping on empty invoke context");

            if let Some(entrypoint_context) = current_trace.additional_data.as_mut() {
                entrypoint_context.push_call_trace(i, call_trace);
            } else {
                current_trace.additional_data =
                    TreeContext::maybe_new_entrypoint_context(i, call_trace)
            }

            *executed = true;
        }
    }

    pub fn log(&mut self, message: &str) {
        if let Some(entrypoint_context) = self.get_last_entrypoint_context() {
            entrypoint_context
                .get_current_unique_tree()
                .tree
                .push_leaf(EntrypointNode::Log(LogData::new(message)));
        } else {
            self.get_current_unique_tree()
                .tree
                .push_leaf(InvokeNode::Log(LogData::new(message)));
        }
    }

    pub fn account_diff(&mut self, data: AccountData) {
        if let Some(entrypoint_context) = self.get_last_entrypoint_context() {
            entrypoint_context
                .get_current_unique_tree()
                .tree
                .push_leaf(EntrypointNode::Account(data));
        } else {
            self.get_current_unique_tree()
                .tree
                .push_leaf(InvokeNode::Account(data));
        }
    }

    fn get_last_entrypoint_context(
        &mut self,
    ) -> Option<&mut TreeContext<EntrypointData, EntrypointNode>> {
        self.trace
            .last_mut()
            .expect("Empty invoke context for entrypoint context")
            .additional_data
            .as_mut()
    }

    fn take_last_entrypoint_context(
        &mut self,
    ) -> Option<TreeContext<EntrypointData, EntrypointNode>> {
        self.trace
            .last_mut()
            .expect("Empty invoke context for entrypoint context")
            .additional_data
            .take()
    }

    pub fn get_last_receiver(&self) -> Pubkey {
        self.subtrees[self
            .trace
            .last()
            .expect("Invoke context lacks last receiver")
            .subtree_index]
            .id
            .receiver
    }

    pub fn materialize(&mut self, materialized_tree: Tree<InvokeNode>) {
        println!("mat tree {:?}", materialized_tree);
        self.get_current_unique_tree()
            .tree
            .get_pushable_subtree(&materialized_tree.node)
            .children
            .push(materialized_tree);
    }
}

impl From<TreeContext<EntrypointData, EntrypointNode>> for Tree<InvokeNode> {
    fn from(value: TreeContext<EntrypointData, EntrypointNode>) -> Self {
        if let Some(root_tree) = value.subtrees.get(0) {
            if root_tree.tree.node != EntrypointNode::Root {
                panic!("Root of EntrypointNode tree is not Root");
            }

            fn recurse(
                subtrees: &Vec<UniqueTree<EntrypointData, EntrypointNode>>,
                source_id: &EntrypointData,
                source_tree: &Tree<EntrypointNode>,
            ) -> Tree<InvokeNode> {
                let mut children: Vec<Tree<InvokeNode>> = Vec::new();

                for child in &source_tree.children {
                    children.push(recurse(subtrees, source_id, &child));
                }

                match source_tree.node.clone() {
                    EntrypointNode::Root => Tree {
                        instruction: source_tree.instruction,
                        node: InvokeNode::Entrypoint(source_id.clone()),
                        children,
                    },
                    EntrypointNode::Entrypoint(index) => {
                        let next_subtree = &subtrees[index];
                        recurse(subtrees, &next_subtree.id, &next_subtree.tree)
                    }
                    EntrypointNode::Log(data) => Tree {
                        instruction: source_tree.instruction,
                        node: InvokeNode::Log(data),
                        children,
                    },
                    EntrypointNode::Account(data) => Tree {
                        instruction: source_tree.instruction,
                        node: InvokeNode::Account(data),
                        children,
                    },
                    EntrypointNode::Error(data) => Tree {
                        instruction: source_tree.instruction,
                        node: InvokeNode::Error(data),
                        children,
                    },
                    EntrypointNode::FnCall(data) => Tree {
                        instruction: source_tree.instruction,
                        node: InvokeNode::FnCall(data),
                        children,
                    },
                }
            }

            recurse(&value.subtrees, &root_tree.id, &root_tree.tree)
        } else {
            panic!("Converting empty entrypoint context into invoke tree");
        }
    }
}

impl From<TreeContext<InvokeData, InvokeNode, Option<TreeContext<EntrypointData, EntrypointNode>>>>
    for Option<Tree<ViewNode>>
{
    fn from(
        value: TreeContext<
            InvokeData,
            InvokeNode,
            Option<TreeContext<EntrypointData, EntrypointNode>>,
        >,
    ) -> Self {
        if let Some(root_tree) = value.subtrees.get(0) {
            if root_tree.tree.node != InvokeNode::Root {
                panic!("Root of InvokeNode tree is not Root");
            }

            fn recurse(
                subtrees: &Vec<UniqueTree<InvokeData, InvokeNode>>,
                source_id: &InvokeData,
                source_tree: &Tree<InvokeNode>,
            ) -> Tree<ViewNode> {
                let mut children: Vec<Tree<ViewNode>> = Vec::new();

                for child in &source_tree.children {
                    children.push(recurse(subtrees, source_id, &child));
                }

                match source_tree.node.clone() {
                    InvokeNode::Root => Tree {
                        instruction: source_tree.instruction,
                        node: ViewNode::Invoke(source_id.clone()),
                        children,
                    },
                    InvokeNode::Invoke(index) => {
                        let next_subtree = &subtrees[index];
                        recurse(subtrees, &next_subtree.id, &next_subtree.tree)
                    }
                    InvokeNode::Entrypoint(data) => Tree {
                        instruction: source_tree.instruction,
                        node: ViewNode::Entrypoint(data),
                        children,
                    },
                    InvokeNode::Log(data) => Tree {
                        instruction: source_tree.instruction,
                        node: ViewNode::Log(data),
                        children,
                    },
                    InvokeNode::Account(data) => Tree {
                        instruction: source_tree.instruction,
                        node: ViewNode::Account(data),
                        children,
                    },
                    InvokeNode::Error(data) => Tree {
                        instruction: source_tree.instruction,
                        node: ViewNode::Error(data),
                        children,
                    },
                    InvokeNode::FnCall(data) => Tree {
                        instruction: source_tree.instruction,
                        node: ViewNode::FnCall(data),
                        children,
                    },
                }
            }

            Some(recurse(&value.subtrees, &root_tree.id, &root_tree.tree))
        } else {
            None
        }
    }
}
