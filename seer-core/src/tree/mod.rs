pub mod demangle;
pub mod edge_cases;
pub mod loc;
pub mod nodes;

use std::{collections::HashMap, vec};

use solana_instruction_error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    entrypoint_lookup::EntrypointLookup,
    tree::{
        edge_cases::delayed_log::DelayedLogEdgeCase,
        nodes::{
            EntrypointChildren, RootChildren, RootViewChildren, TreeAccount, TreeEntrypoint,
            TreeRoot,
        },
    },
};

struct LiveTrace {
    tree_index: usize,
    last_known_entrypoint: Option<TreeEntrypoint<EntrypointChildren>>,
    barrel: Option<TreeEntrypoint<EntrypointChildren>>,

    // edge cases
    delayed_log_edge_case: DelayedLogEdgeCase,
}

pub struct InvokeContext {
    live_trace: Vec<LiveTrace>,
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

        if let Some(live_trace) = self.live_trace.last_mut() {
            let root = &mut self.trees[live_trace.tree_index];

            match live_trace
                .barrel
                .take()
                .or_else(|| live_trace.last_known_entrypoint.take())
            {
                Some(mut barrel) => {
                    barrel.push_invoke(tree_len);
                    root.push_entrypoint(barrel);
                }
                None => {
                    root.push_invoke_root(tree_len);
                }
            }
        }

        self.trees.push(TreeRoot {
            sender,
            receiver,
            children: vec![],
        });

        self.live_trace.push(LiveTrace {
            tree_index: tree_len,
            last_known_entrypoint: None,
            barrel: None,
            delayed_log_edge_case: DelayedLogEdgeCase::new(),
        });
    }

    pub fn end_program(&mut self, maybe_err: Option<InstructionError>) {
        let live_trace = self
            .live_trace
            .last_mut()
            .expect("Ending program on empty live trace");
        let root = &mut self.trees[live_trace.tree_index];

        if let Some(err) = maybe_err {
            root.push_err(err.to_string());
        }

        match live_trace
            .barrel
            .take()
            .or_else(|| live_trace.last_known_entrypoint.clone())
        {
            Some(barrel) => {
                root.push_entrypoint(barrel);
            }
            _ => {}
        }

        self.live_trace.pop();
    }

    pub fn step(
        &mut self,
        lookups: &HashMap<Pubkey, EntrypointLookup>,
        i: u64,
        executed: &mut bool,
    ) {
        let live_trace = self
            .live_trace
            .last_mut()
            .expect("Stepping on empty live trace");
        let root = &mut self.trees[live_trace.tree_index];

        if let Some(barrel) = live_trace
            .barrel
            .take()
            .or_else(|| live_trace.last_known_entrypoint.clone())
        {
            live_trace.delayed_log_edge_case.met_hook(&barrel);
            root.push_entrypoint(barrel);
        }

        if let Some(lookup) = lookups.get(&self.trees[live_trace.tree_index].receiver) {
            if let Some(entrypoint) = lookup.get_entrypoint(i) {
                live_trace.delayed_log_edge_case.lke_hook(&entrypoint);
                live_trace.last_known_entrypoint = Some(entrypoint);
                *executed = true;
            }
        }
    }

    pub fn log(&mut self, message: &str) {
        let live_trace = self
            .live_trace
            .last_mut()
            .expect("Logging on empty live trace");
        let root = &mut self.trees[live_trace.tree_index];

        live_trace
            .delayed_log_edge_case
            .log_hook(&mut live_trace.barrel);

        if let Some(barrel) = live_trace.barrel.as_mut() {
            barrel.push_log(message.to_string());
        } else if let Some(last_known_entrypoint) = &live_trace.last_known_entrypoint {
            let mut barrel = last_known_entrypoint.clone();
            barrel.push_log(message.to_string());
            live_trace.barrel = Some(barrel);
        } else {
            root.push_log(message.to_string());
        }
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        if let Some(live_trace) = self.live_trace.last_mut() {
            let root = &mut self.trees[live_trace.tree_index];

            if let Some(barrel) = live_trace.barrel.as_mut() {
                barrel.push_account_diff(data);
            } else if let Some(last_known_entrypoint) = &live_trace.last_known_entrypoint {
                let mut barrel = last_known_entrypoint.clone();
                barrel.push_account_diff(data);
                live_trace.barrel = Some(barrel);
            } else {
                root.push_account_diff(data);
            }
        } else {
            let root = &mut self.trees[0];
            root.push_account_diff(data);
        }
    }

    pub fn get_last_receiver(&self) -> Pubkey {
        self.trees[self
            .live_trace
            .last()
            .expect("Invoke context lacks last receiver")
            .tree_index]
            .receiver
    }
}

impl From<InvokeContext> for TreeRoot<RootViewChildren> {
    fn from(value: InvokeContext) -> Self {
        TreeRoot::clone_into_view(0, &value.trees)
    }
}
