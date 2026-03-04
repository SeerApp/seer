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
            TreeLog, TreeRoot,
        },
    },
};

struct LiveTrace {
    tree_index: usize,
    last_known_entrypoint: Option<TreeEntrypoint<EntrypointChildren>>,
    pushables: Vec<EntrypointChildren>,

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

            if let Some(mut lke) = live_trace.last_known_entrypoint.take() {
                for p in live_trace.pushables.iter() {
                    match p {
                        EntrypointChildren::Log(l) => lke.push_log(l.message.clone()),
                        EntrypointChildren::Account(a) => lke.push_account_diff(a.clone()),
                        _ => panic!("Unexpected value in pushables!"),
                    }
                }

                lke.push_invoke(tree_len);
                root.push_entrypoint(lke);
            } else {
                root.push_invoke_root(tree_len);
            }

            live_trace.pushables = vec![];
        }

        self.trees.push(TreeRoot {
            sender,
            receiver,
            children: vec![],
        });

        self.live_trace.push(LiveTrace {
            tree_index: tree_len,
            last_known_entrypoint: None,
            pushables: vec![],
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

        for p in &live_trace.pushables {
            root.children.push(p.into());
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

        if let Some(mut lke) = live_trace.last_known_entrypoint.clone() {
            for p in live_trace.pushables.iter() {
                match p {
                    EntrypointChildren::Log(l) => lke.push_log(l.message.clone()),
                    EntrypointChildren::Account(a) => lke.push_account_diff(a.clone()),
                    _ => panic!("Unexpected value in pushables!"),
                }
            }
            live_trace.pushables = vec![];
            live_trace.delayed_log_edge_case.met_hook(&lke);
            root.push_entrypoint(lke);
        }

        if let Some(lookup) = lookups.get(&self.trees[live_trace.tree_index].receiver) {
            if let Some(entrypoint) = lookup.get_entrypoint(i) {
                live_trace.delayed_log_edge_case.lke_hook(&entrypoint);
                live_trace.last_known_entrypoint = Some(entrypoint);
            }
            *executed = true;
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
            .log_hook(&mut live_trace.last_known_entrypoint);

        if live_trace.last_known_entrypoint.is_some() {
            live_trace
                .pushables
                .push(EntrypointChildren::Log(TreeLog::new(message.to_string())));
        } else {
            root.push_log(message.to_string());
        }
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        let tree_index = self
            .live_trace
            .last()
            .map(|trace| trace.tree_index)
            .unwrap_or(0);

        let root = &mut self.trees[tree_index];

        if let Some(live_trace) = self.live_trace.last_mut() {
            if live_trace.last_known_entrypoint.is_some() {
                live_trace.pushables.push(EntrypointChildren::Account(data));
                return;
            }
        }

        root.push_account_diff(data);
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
