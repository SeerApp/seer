use std::collections::HashMap;

use seer_interface::GuestMemory;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    call_trace_lookup::CallTraceLookup,
    tracer::Tracer,
    tree::{
        view::{AccountData, ViewNode},
        Tree,
    },
};

pub struct InstructionContext {
    index: u8,
    tracer: Tracer,
}

impl InstructionContext {
    pub fn new(index: u8, fee_payer: Pubkey) -> Self {
        Self {
            index,
            tracer: Tracer::new(fee_payer),
        }
    }

    pub fn log(&mut self, message: &str) {
        self.tracer.log(message);
    }

    pub fn start_program(&mut self, program_address: Pubkey) {
        self.tracer.start_program(program_address);
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        self.tracer.end_program(err)
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        lookups: &HashMap<Pubkey, CallTraceLookup>,
        i: u64,
        mem: &mut M,
        reg: &[u64; 12],
    ) {
        self.tracer.step(lookups, i, mem, reg);
    }

    pub fn account_diff(&mut self, data: AccountData) {
        self.tracer.account_diff(data);
    }
}

impl From<InstructionContext> for Option<(u8, Tree<ViewNode>)> {
    fn from(value: InstructionContext) -> Self {
        Into::<Option<Tree<ViewNode>>>::into(value.tracer).map(|tracer| (value.index, tracer))
    }
}
