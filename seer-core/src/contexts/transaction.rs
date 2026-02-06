use std::collections::HashMap;

use seer_interface::{GuestMemory, GuestStepMirror};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    entrypoint_lookup::EntrypointLookup,
    contexts::instruction::InstructionContext,
    tree::nodes::{RootViewChildren, TreeRoot},
    step_mirror::StepMirror,
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
        self.instruction_context = Some(InstructionContext::new(
            instruction,
            fee_payer,
            Some(self.signature),
        ));
    }

    pub fn end_instruction(&mut self) -> Option<(u8, TreeRoot<RootViewChildren>)> {
        let mut icx = self.instruction_context
            .take()
            .expect("Ending instruction before it exists");

        if icx.executed() {
            if let Some(step_mirror) = &mut self.step_mirror {
                for acc in step_mirror.check_diffs() {
                    icx.account_diff(acc);
                }
            }
        }

        icx.into()
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
        self.instruction_context
            .as_mut()
            .expect("Ending program before instruction context exists")
            .end_program(err);
    }

    pub fn log(&mut self, message: &str) {
        let icx = self.instruction_context
            .as_mut()
            .expect("Logging before instruction context exists");

        icx.log(message);
            
        if icx.executed() {
            if let Some(step_mirror) = &mut self.step_mirror {
                for acc in step_mirror.check_diffs() {
                    icx.account_diff(acc);
                }
            }
        }
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        lookups: &HashMap<Pubkey, EntrypointLookup>,
        i: u64,
        mem: &mut M,
        reg: &[u64; 12],
    ) {
        let icx = self
            .instruction_context
            .as_mut()
            .expect("Stepping before instruction context exists");

        if let Some(step_mirror) = &mut self.step_mirror {
            for acc in step_mirror.check_diffs() {
                icx.account_diff(acc);
            }
        }

        icx.step(lookups, i, mem, reg);
    }
}
