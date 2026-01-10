use std::collections::HashMap;

use seer_interface::GuestMemory;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::call_trace_lookup::CallTraceLookup;
#[cfg(feature = "step_trace")]
use crate::step_trace::StepTrace;
use crate::trace_tree::node_data::{AccountData, ErrorData, LogData, NodeData};
use crate::trace_tree::TraceTree;

struct ProgramTraceTree {
    pub program_address: Pubkey,
    pub trace_tree: TraceTree,
    // pub last_call_trace: VecDeque<NodeData>,
}

pub struct TraceTreeContext {
    fee_payer: Pubkey,
    trace: Vec<ProgramTraceTree>,
    stepped: bool,
    return_trace_tree: Option<TraceTree>,
    #[cfg(feature = "step_trace")]
    pub step_trace: StepTrace,
}

impl TraceTreeContext {
    pub fn new(fee_payer: Pubkey) -> Self {
        Self {
            fee_payer,
            trace: vec![],
            stepped: false,
            return_trace_tree: None,
            #[cfg(feature = "step_trace")]
            step_trace: StepTrace::new(),
        }
    }

    pub fn start_program(&mut self, program_address: Pubkey) {
        let sender = match self.trace.last() {
            Some(t) => t.program_address,
            None => self.fee_payer,
        };

        let trace_tree = TraceTree::new(sender, program_address);

        self.trace.push(ProgramTraceTree {
            program_address,
            trace_tree,
        });
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) -> bool {
        let mut trace = self
            .trace
            .pop()
            .expect("Ending program with no program trace tree");

        if let Some(msg) = err {
            trace
                .trace_tree
                .push_leaf(NodeData::Error(ErrorData::new(msg.to_string())));
        }

        if let Some(parent_trace) = self.trace.last_mut() {
            parent_trace.trace_tree.attach(trace.trace_tree);
            false
        } else {
            self.return_trace_tree = Some(trace.trace_tree);
            true
        }
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        lookups: &HashMap<Pubkey, CallTraceLookup>,
        i: u64,
        _: &mut M,
        _: &[u64; 12],
    ) {
        let trace = self.trace.last_mut().expect("Stepping before tree exists");

        if let Some(lookup) = lookups.get(&trace.program_address) {
            self.stepped = true;

            #[cfg(feature = "step_trace")]
            self.step_trace.insert(trace.program_address, i);

            let call_trace = lookup.get_call_trace(i.clone());
            trace.trace_tree.push(i, call_trace);
        }
    }

    pub fn log(&mut self, message: &str) {
        let trace = self.trace.last_mut().expect("Logging before tree exists");

        trace
            .trace_tree
            .push_leaf(NodeData::Log(LogData::new(message)));
    }

    pub fn account_diff(&mut self, data: AccountData) {
        let trace = self.trace.last_mut().expect("Account diff before tree exists");

        trace.trace_tree.push_leaf(NodeData::Account(data));
    }
}

impl From<TraceTreeContext> for Option<TraceTree> {
    fn from(value: TraceTreeContext) -> Self {
        if value.stepped {
            #[cfg(feature = "step_trace")]
            value.step_trace.save();

            value.return_trace_tree
        } else {
            None
        }
    }
}
