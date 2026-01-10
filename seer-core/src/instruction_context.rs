use std::collections::HashMap;

use seer_interface::GuestMemory;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    call_trace_lookup::CallTraceLookup,
    trace_tree::{TraceTree, context::TraceTreeContext, node_data::AccountData},
};

pub struct InstructionContext {
    index: u8,
    trace_tree_context: TraceTreeContext,
}

impl InstructionContext {
    pub fn new(index: u8, fee_payer: Pubkey) -> Self {
        Self {
            index,
            trace_tree_context: TraceTreeContext::new(fee_payer),
        }
    }

    pub fn log(&mut self, message: &str) {
        self.trace_tree_context.log(message);
    }

    pub fn start_program(&mut self, program_address: Pubkey) {
        self.trace_tree_context.start_program(program_address);
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) -> bool {
        self.trace_tree_context.end_program(err)
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        lookups: &HashMap<Pubkey, CallTraceLookup>,
        i: u64,
        mem: &mut M,
        reg: &[u64; 12],
    ) {
        self.trace_tree_context.step(lookups, i, mem, reg);
    }

    pub fn account_diff(&mut self, data: AccountData) {
        self.trace_tree_context.account_diff(data);
    }
}

impl From<InstructionContext> for Option<(u8, TraceTree)> {
    fn from(value: InstructionContext) -> Self {
        match value.trace_tree_context.into() {
            Some(trace_tree) => {
                Some((value.index, trace_tree))
            },
            None => None
        }
    }
}