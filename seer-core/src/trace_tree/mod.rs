pub mod loc;
pub mod node_data;
pub mod context;
pub mod demangle;

use std::{collections::VecDeque, vec};

use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;

use crate::prefix::is_prefix;
use crate::trace_tree::node_data::{InvokeData, NodeData};

#[derive(Serialize, Deserialize)]
pub struct TraceTree {
    instruction: Option<u64>,
    node: NodeData,
    children: Vec<TraceTree>,
}

impl TraceTree {
    pub fn new(sender: Pubkey, receiver: Pubkey) -> Self {
        Self {
            instruction: None,
            node: NodeData::Invoke(InvokeData::new(sender, receiver)),
            children: vec![],
        }
    }

    pub fn attach(&mut self, tree: TraceTree) {
        self.get_last_invoke_parent_mut().children.push(tree);
    }

    pub fn push(&mut self, i: u64, mut call_trace: VecDeque<NodeData>) {
        if !call_trace.is_empty() {
            let last_call_trace = self.get_last_call_trace();

            // Always force some entrypoint
            if !last_call_trace.is_empty() && !matches!(call_trace[0], NodeData::Entrypoint(_)) {
                call_trace.push_front(last_call_trace[0].clone());
            }

            if !is_prefix::<NodeData>(&call_trace, &last_call_trace) {
                self.grow(i, &call_trace, 0);
            }
        }
    }

    pub fn push_leaf(&mut self, node: NodeData) {
        if node.is_leaf() {
            self.push_node(node);
        } else {
            panic!("Pushing branch as leaf");
        }
    }

    fn push_node(&mut self, node: NodeData) {
        if self.node.is_leaf() {
            panic!("Pushing to leaf");
        }

        let trace = self.get_last_branch_mut();

        trace.children.push(TraceTree {
            instruction: trace.instruction,
            node,
            children: vec![],
        });
    }

    fn get_last_invoke_parent_mut(&mut self) -> &mut TraceTree {
        let should_recurse = self
            .children
            .last()
            .map(|c| c.node.is_invoke_parent()) 
            .unwrap_or(false);

        if should_recurse {
            self.children.last_mut().unwrap().get_last_invoke_parent_mut()
        } else {
            self
        }
    }

    fn get_last_branch_mut(&mut self) -> &mut TraceTree {
        let should_recurse = self
            .children
            .last()
            .map(|c| c.node.is_branch())
            .unwrap_or(false);

        if should_recurse {
            self.children.last_mut().unwrap().get_last_branch_mut()
        } else {
            self
        }
    }

    fn get_last_call_trace(&self) -> VecDeque<NodeData> {
        let mut call_trace = VecDeque::new();
        let mut current_node = self;

        loop {
            call_trace.push_back(current_node.node.clone());

            let length = current_node.children.len();

            if length == 0 {
                break;
            }

            let last_child = &current_node.children[length - 1];

            current_node = last_child;
        }

        call_trace
    }

    fn grow(&mut self, i: u64, call_trace: &VecDeque<NodeData>, counter: usize) {
        let Some(ct) = call_trace.get(counter) else {
            return;
        };

        let needs_new_child = self.children.last().map_or(true, |last| &last.node != ct);

        if needs_new_child {
            self.children.push(TraceTree {
                instruction: Some(i),
                node: ct.clone(),
                children: vec![],
            });
        }

        self.children
            .last_mut()
            .unwrap()
            .grow(i, call_trace, counter + 1);
    }
}
