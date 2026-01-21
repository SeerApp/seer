use std::collections::VecDeque;

use crate::{
    dwarf::source_die::{SourceDie, SourceDieType},
    tree::view::{AccountData, EntrypointData, ErrorData, FnCallData, LogData},
    tree::{TreeContext, TreeNode},
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

    fn is_leaf(&self) -> bool {
        matches!(self, Self::Log(_) | Self::Error(_) | Self::Account(_))
    }

    fn is_fn_call(&self) -> bool {
        matches!(self, Self::FnCall(_))
    }

    fn can_push_to(&self, tree: &super::Tree<Self>) -> bool {
        match self {
            Self::Root => panic!("Cannot push root to a tree"),
            Self::Entrypoint(_) => !tree.node.is_leaf(),
            Self::FnCall(_) => !tree.node.is_leaf(),
            Self::Log(_) => !tree.node.is_leaf(),
            Self::Error(_) => !tree.node.is_leaf(),
            Self::Account(_) => !tree.node.is_leaf(),
        }
    }
}

impl TreeContext<EntrypointData, EntrypointNode> {
    pub fn maybe_new_entrypoint_context(
        i: u64,
        source_die_trace: VecDeque<SourceDie>,
    ) -> Option<Self> {
        let mut context = Self {
            trace: vec![],
            subtrees: vec![],
        };

        context.push_call_trace(i, source_die_trace);

        if context.trace.len() > 0 {
            Some(context)
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

            println!("call {:?} {:?}", entrypoint, call_trace);

            let found_index = self.trace.iter().enumerate().find_map(|(index, trace)| {
                let subtree_index = trace.subtree_index;
                self.subtrees.get(subtree_index).and_then(|subtree| {
                    if subtree.id == entrypoint {
                        Some(index)
                    } else {
                        None
                    }
                })
            });

            if let Some(index) = found_index {
                self.trace.truncate(index + 1);
            } else {
                self.push_subtree(entrypoint, ());
            }

            self.get_current_unique_tree()
                .tree
                .push_branch(i, call_trace);
        }
    }
}
