use std::collections::HashMap;

use seer_interface::{GuestMemory, GuestStepMirror};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    call_trace_lookup::CallTraceLookup, contexts::instruction::InstructionContext, step_mirror::StepMirror, tree::{Tree, view::ViewNode}
};

pub struct TransactionContext {
    pub signature: Signature,
    instruction_context: Option<InstructionContext>,
    pub step_mirror: Option<StepMirror>,
}

impl<'a> TransactionContext {
    pub fn new(signature: Signature) -> Self {
        Self {
            signature,
            instruction_context: None,
            step_mirror: None,
        }
    }

    pub fn start_instruction(&mut self, instruction: u8, fee_payer: Pubkey) {
        self.instruction_context = Some(InstructionContext::new(instruction, fee_payer));
    }

    pub fn end_instruction(&mut self) -> Option<(u8, Tree<ViewNode>)> {
        self.instruction_context
            .take()
            .expect("Ending instruction before it exists")
            .into()
    }

    pub unsafe fn start_program(&mut self, program_address: Pubkey, mirror: &dyn GuestStepMirror) {
        if self.step_mirror.is_none() {
            self.step_mirror = Some(StepMirror::new(mirror));
        }

        self.instruction_context
            .as_mut()
            .expect("Starting program before instruction context exists")
            .start_program(program_address);
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        self
            .instruction_context
            .as_mut()
            .expect("Ending program before instruction context exists")
            .end_program(err);
    }

    pub fn log(&mut self, message: &str) {
        self.instruction_context
            .as_mut()
            .expect("Logging before instruction context exists")
            .log(message);
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        lookups: &HashMap<Pubkey, CallTraceLookup>,
        i: u64,
        mem: &mut M,
        reg: &[u64; 12],
    ) {
        let icx = self
            .instruction_context
            .as_mut()
            .expect("Stepping before instruction context exists");

        icx.step(lookups, i, mem, reg);

        if let Some(step_mirror) = &mut self.step_mirror {
            for acc in step_mirror.check_diffs() {
                icx.account_diff(acc);
            }
        }
    }
}
