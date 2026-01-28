use std::collections::VecDeque;

use crate::{
    dwarf::source_die::{SourceDie, SourceDieType},
    tree::{
        invoke::InvokeNode,
        view::{AccountData, EntrypointData, ErrorData, FnCallData, LogData},
        Tree, TreeContext, TreeNode,
    },
};

#[derive(Clone, PartialEq, Debug)]
pub enum EntrypointNode {
    Root,
    FnCall(FnCallData),
    Log(LogData),
    Error(ErrorData),
    Account(AccountData),
    Entrypoint(usize),
}

impl TreeNode for EntrypointNode {
    fn root() -> Self {
        Self::Root
    }

    fn index(index: usize) -> Self {
        Self::Entrypoint(index)
    }

    fn is_leaf(&self) -> bool {
        matches!(self, Self::Log(_) | Self::Error(_) | Self::Account(_))
    }

    fn is_fn_call(&self) -> bool {
        matches!(self, Self::FnCall(_))
    }

    fn can_push_to(&self, tree: &super::Tree<Self>) -> bool {
        if tree.node.is_leaf() {
            false
        } else {
            match self {
                Self::Root => panic!("Cannot push root to a tree"),
                _ => true,
            }
        }
    }
}

#[derive(Debug)]
pub struct EntrypointHolder {
    context: TreeContext<EntrypointData, EntrypointNode>,
    cursor: Option<EntrypointNode>,
}

impl EntrypointHolder {
    pub fn maybe_new(i: u64, source_die_trace: VecDeque<SourceDie>) -> Option<Self> {
        let mut entrypoint_holder: Self = Self {
            context: TreeContext {
                trace: vec![],
                subtrees: vec![],
            },
            cursor: None,
        };

        entrypoint_holder.push_call_trace(i, source_die_trace);

        if entrypoint_holder.context.trace.len() > 0 {
            Some(entrypoint_holder)
        } else {
            None
        }
    }

    pub fn push_call_trace(&mut self, i: u64, mut source_die_trace: VecDeque<SourceDie>) {
        let maybe_entrypoint = 'find_entrypoint: {
            while let Some(source_die) = source_die_trace.pop_front() {
                match source_die.source_type {
                    SourceDieType::Fn => {
                        if let Some(d) = source_die.loc.decl {
                            break 'find_entrypoint Some(EntrypointData {
                                signature: source_die.loc.signature,
                                loc: d,
                            });
                        }
                    }
                    _ => {}
                }
            }
            None
        };

        if let Some(entrypoint) = maybe_entrypoint {
            let mut call_trace = VecDeque::new();
            while let Some(source_die) = source_die_trace.pop_front() {
                match source_die.source_type {
                    SourceDieType::Fn => {
                        if let Some(c) = source_die.loc.call {
                            call_trace.push_back(EntrypointNode::FnCall(FnCallData {
                                signature: source_die.loc.signature,
                                loc: c,
                            }));
                        }
                    }
                    _ => {}
                }
            }

            let found_index = self
                .context
                .trace
                .iter()
                .enumerate()
                .find_map(|(index, trace)| {
                    let subtree_index = trace.subtree_index;
                    self.context
                        .subtrees
                        .get(subtree_index)
                        .and_then(|subtree| {
                            if subtree.id == entrypoint {
                                Some(index)
                            } else {
                                None
                            }
                        })
                });

            if let Some(index) = found_index {
                self.context.trace.truncate(index + 1);
            } else {
                self.context.push_subtree(entrypoint, ());
            }

            self.cursor = call_trace.iter().last().cloned();

            self.context
                .get_current_unique_tree()
                .tree
                .push_branch(i, call_trace);
        }
    }

    pub fn push_leaf(&mut self, node: EntrypointNode) {
        self.context
            .get_current_unique_tree()
            .tree
            .push_leaf(node, self.cursor.as_ref());
    }
}

impl From<EntrypointHolder> for Tree<InvokeNode> {
    fn from(value: EntrypointHolder) -> Self {
        value.context.into()
    }
}

impl From<&Tree<InvokeNode>> for Option<TreeContext<EntrypointData, EntrypointNode>> {
    fn from(value: &Tree<InvokeNode>) -> Self {
        let mut last_branch = value.colone_last_branch();

        println!("Got last branch {:?}", last_branch);

        let mut new_context: TreeContext<EntrypointData, EntrypointNode> = TreeContext {
            trace: vec![],
            subtrees: vec![],
        };
        let mut source_tree = &mut last_branch;

        loop {
            match &source_tree.node {
                InvokeNode::Entrypoint(id) => {
                    new_context.push_subtree(id.clone(), ());
                }
                InvokeNode::FnCall(data) => {
                    new_context
                        .get_current_unique_tree()
                        .tree
                        .get_absolute_last_child()
                        .children
                        .push(Tree {
                            instruction: source_tree.instruction,
                            node: EntrypointNode::FnCall(data.clone()),
                            children: vec![],
                        });
                }
                _ => {}
            }

            if let Some(last_child) = source_tree.children.last_mut() {
                source_tree = last_child;
            } else {
                break;
            }
        }

        if new_context.trace.is_empty() {
            None
        } else {
            Some(new_context)
        }
    }
}

impl From<&Tree<InvokeNode>> for Option<EntrypointHolder> {
    fn from(value: &Tree<InvokeNode>) -> Self {
        let maybe_context: Option<TreeContext<EntrypointData, EntrypointNode>> = value.into();

        if let Some(mut context) = maybe_context {
            let cursor = match context
                .get_current_unique_tree()
                .tree
                .get_absolute_last_child()
                .node
                .clone()
            {
                EntrypointNode::Root => None,
                node => Some(node),
            };

            Some(EntrypointHolder { context, cursor })
        } else {
            None
        }
    }
}
