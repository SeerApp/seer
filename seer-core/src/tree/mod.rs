pub mod nodes;
pub mod loc;
pub mod demangle;

use std::collections::HashMap;

use solana_instruction_error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    call_trace_lookup::CallTraceLookup,
    tree::nodes::{RootChildren, RootViewChildren, TreeAccount, TreeRoot},
};

pub struct InvokeContext {
    live_trace: Vec<usize>,
    trees: Vec<TreeRoot<RootChildren>>,
}

impl InvokeContext {
    pub fn new() -> Self {
        Self {
            live_trace: vec![],
            trees: vec![],
        }
    }

    pub fn start_program(&mut self, sender: Pubkey, receiver: Pubkey) {
        let tree_len = self.trees.len();

        self.trees.push(TreeRoot {
            sender,
            receiver,
            children: vec![],
        });

        if let Some(root) = self.get_current_tree_root() {
            root.push_invoke(tree_len);
        }

        self.live_trace.push(tree_len);
    }

    pub fn end_program(&mut self, maybe_err: Option<InstructionError>) {
        if let Some(err) = maybe_err {
            self.get_current_tree_root()
                .expect("Ending program on empty live trace")
                .push_err(err.to_string());
        }

        self.live_trace.pop();
    }

    pub fn step(
        &mut self,
        lookups: &HashMap<Pubkey, CallTraceLookup>,
        i: u64,
        executed: &mut bool,
    ) {
        let root = self
            .get_current_tree_root()
            .expect("Stepping on empty live trace");

        if let Some(lookup) = lookups.get(&root.receiver) {
            let call_trace = lookup.get_call_trace(i);

            root.push_call_trace(i, call_trace);

            *executed = true;
        }
    }

    pub fn log(&mut self, message: &str) {
        self.get_current_tree_root()
            .expect("Logging on empty live trace")
            .push_log(message.to_string());
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        self.get_current_tree_root()
            .expect("Account diff on empty live trace")
            .push_account_diff(data);
    }

    fn get_current_tree_root(&mut self) -> Option<&mut TreeRoot<RootChildren>> {
        if let Some(last_trace_index) = self.live_trace.last() {
            Some(&mut self.trees[*last_trace_index])
        } else {
            None
        }
    }

    pub fn get_last_receiver(&self) -> Pubkey {
        self.trees[*self
            .live_trace
            .last()
            .expect("Invoke context lacks last receiver")]
        .receiver
    }
}

impl From<InvokeContext> for TreeRoot<RootViewChildren> {
    fn from(value: InvokeContext) -> Self {
        TreeRoot::clone_into_view(0, &value.trees)
    }
}
