use std::vec;

use solana_instruction_error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    program_manager::types::GlobalProgramContext, tree::{
        edge_cases::delayed_log::DelayedLogEdgeCase,
        nodes::{
            account::TreeAccount,
            entrypoint::{EntrypointChildren, TreeEntrypoint},
            log::TreeLog,
            root::{RootChildren, RootViewChildren, TreeRoot},
        },
    }
};

enum Pushable {
    Log(TreeLog),
    Account(TreeAccount),
}

struct LiveTrace {
    tree_index: usize,
    last_known_entrypoint: Option<TreeEntrypoint<EntrypointChildren>>,
    pushables: Vec<Pushable>,

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

    pub fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        sender: Pubkey,
        receiver: Pubkey,
        step_order: u64,
    ) {
        let tree_len = self.trees.len();

        if let Some(live_trace) = self.live_trace.last_mut() {
            let root = &mut self.trees[live_trace.tree_index];

            if let Some(mut lke) = live_trace.last_known_entrypoint.take() {
                for p in live_trace.pushables.iter() {
                    match p {
                        Pushable::Log(l) => {
                            lke.push_log(l.message.clone(), l.step_order);
                        }
                        Pushable::Account(a) => {
                            lke.push_account_diff(a.clone());
                        }
                    }
                }

                lke.push_invoke(tree_len, step_order);
                root.push_entrypoint(lke);
            } else {
                root.push_invoke_root(tree_len, step_order);
            }

            live_trace.pushables = vec![];
        }

        self.trees.push(TreeRoot {
            step_order,
            sender,
            receiver,
            accounts,
            data,
            children: vec![],
            parsed: None,
        });

        self.live_trace.push(LiveTrace {
            tree_index: tree_len,
            last_known_entrypoint: None,
            pushables: vec![],
            delayed_log_edge_case: DelayedLogEdgeCase::new(),
        });
    }

    pub fn end_program(&mut self, maybe_err: Option<InstructionError>, step_order: u64) {
        let live_trace = self
            .live_trace
            .last_mut()
            .expect("Ending program on empty live trace");
        let root = &mut self.trees[live_trace.tree_index];

        if let Some(err) = maybe_err {
            root.push_err(err, step_order);
        }

        for p in &live_trace.pushables {
            match p {
                Pushable::Log(l) => root.children.push(RootChildren::Log(l.clone())),
                Pushable::Account(a) => root.children.push(RootChildren::Account(a.clone())),
            }
        }

        self.live_trace.pop();
    }

    pub fn step(&mut self, global_program_context: &GlobalProgramContext, i: u64, step_order: u64) {
        let live_trace = self
            .live_trace
            .last_mut()
            .expect("Stepping on empty live trace");
        let root = &mut self.trees[live_trace.tree_index];

        if let Some(mut lke) = live_trace.last_known_entrypoint.clone() {
            for p in live_trace.pushables.iter() {
                match p {
                    Pushable::Log(l) => {
                        lke.push_log(l.message.clone(), l.step_order);
                    }
                    Pushable::Account(a) => {
                        lke.push_account_diff(a.clone());
                    }
                }
            }
            live_trace.pushables = vec![];
            live_trace.delayed_log_edge_case.met_hook(&lke);
            root.push_entrypoint(lke);
        }

        let current_program = &self.trees[live_trace.tree_index].receiver;

        if let Some(entrypoint_lookup) = global_program_context.get_entrypoint_lookup(current_program) {
            if let Some(entrypoint) = entrypoint_lookup.get_entrypoint(i, step_order) {
                live_trace.delayed_log_edge_case.lke_hook(&entrypoint);
                live_trace.last_known_entrypoint = Some(entrypoint);
            }
        }
    }

    pub fn log(&mut self, message: &str, step_order: u64) {
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
                .push(Pushable::Log(TreeLog::new(message.to_string(), step_order)));
        } else {
            root.push_log(message.to_string(), step_order);
        }
    }

    /// `TreeAccount::step_order` must be set by the producer (e.g. `UnsafeAccountBackdoor::check_diffs`).
    pub fn account_diff(&mut self, data: TreeAccount) {
        let tree_index = self
            .live_trace
            .last()
            .map(|trace| trace.tree_index)
            .unwrap_or(0);

        let root = &mut self.trees[tree_index];

        if let Some(live_trace) = self.live_trace.last_mut() {
            if live_trace.last_known_entrypoint.is_some() {
                live_trace.pushables.push(Pushable::Account(data));
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

    pub fn trees_iter_mut(&mut self) -> impl Iterator<Item = &mut TreeRoot<RootChildren>> {
        self.trees.iter_mut()
    }

    pub fn flatten_account_diffs(&mut self) {
        for tree in self.trees.iter_mut() {
            tree.flatten_account_diffs();
        }
    }

    // pub fn finalize_account_read_aggregates(
    //     &mut self,
    //     global_program_context: Option<&GlobalProgramContext>,
    // ) -> Vec<TaggedAccountLoadAggregated> {
    //     nodes::finalize_invoke_account_read_aggregates(
    //         &self.trees[..],
    //         &self.account_reads,
    //         global_program_context,
    //     )
    // }
}

impl From<InvokeContext> for TreeRoot<RootViewChildren> {
    fn from(value: InvokeContext) -> Self {
        TreeRoot::clone_into_view(0, &value.trees)
    }
}
