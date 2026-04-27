use seer_interface::{GuestMemory, GuestStepMirror};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    contexts::instruction::InstructionContext,
    meta::TxMetadata,
    program_manager::program_manager::ProgramManager,
    step_mirror::StepMirror,
    tree::nodes::{RootViewChildren, TreeRoot},
};

pub struct TransactionContext {
    pub signature: Signature,
    instruction_context: Option<InstructionContext>,
    pub step_mirror: Option<StepMirror>,
    pub meta: TxMetadata,
    step_order: u64,
    step_executed: bool,
    next_tree_root_uid: u64,
}

impl TransactionContext {
    pub fn new(signature: Signature) -> Self {
        Self {
            signature,
            instruction_context: None,
            step_mirror: None,
            meta: TxMetadata::default(),
            step_order: 0,
            step_executed: false,
            next_tree_root_uid: 0,
        }
    }

    pub fn allocate_tree_root_uid(&mut self) -> u64 {
        let uid = self.next_tree_root_uid;
        self.next_tree_root_uid += 1;
        uid
    }

    pub fn start_instruction(&mut self, instruction: u8, fee_payer: Pubkey) {
        self.instruction_context = Some(InstructionContext::new(
            instruction,
            fee_payer,
            Some(self.signature),
        ));
    }

    pub fn end_instruction(
        &mut self,
        program_manager: &ProgramManager,
    ) -> Option<(u8, TreeRoot<RootViewChildren>)> {
        let mut icx = self
            .instruction_context
            .take()
            .expect("Ending instruction before it exists");

        if let Some(step_mirror) = &mut self.step_mirror {
            for acc in step_mirror.check_diffs(self.step_order) {
                icx.account_diff(acc);
            }
        }

        icx.finalize_tree(program_manager);
        icx.into()
    }

    pub unsafe fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
        mirror: &dyn GuestStepMirror,
    ) {
        if self.step_mirror.is_none() {
            self.step_mirror = Some(StepMirror::new(mirror));
        }

        let tree_uid = self.allocate_tree_root_uid();
        let order = self.step_order;

        self.instruction_context
            .as_mut()
            .expect("Starting program before instruction context exists")
            .start_program(accounts, data, program_address, tree_uid, order);
    }

    pub fn end_program(
        &mut self,
        program_manager: &ProgramManager,
        program_address: Pubkey,
        err: Option<InstructionError>,
    ) {
        let idl_lookup = program_manager.get_idl_lookup(&program_address);
        let idl = idl_lookup
            .as_deref()
            .map(|l| l as &dyn crate::idl::IdlTreeParser);
        self.meta.set_output(err.clone(), idl);
        let order = self.step_order;
        self.instruction_context
            .as_mut()
            .expect("Ending program before instruction context exists")
            .end_program(err, order);
    }

    pub fn log(&mut self, message: &str) {
        let order = self.step_order;
        let icx = self
            .instruction_context
            .as_mut()
            .expect("Logging before transaction context exists");

        icx.log(message, order);
        if let Some(step_mirror) = &mut self.step_mirror {
            for acc in step_mirror.check_diffs(order) {
                icx.account_diff(acc);
            }
        }
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        program_manager: &ProgramManager,
        i: u64,
        mem: &mut M,
        reg: &[u64; 12],
    ) {
        if self.step_executed {
            self.step_order += 1;
        }
        let order = self.step_order;
        crate::account_read_trace::set_current_trace_order(order);

        let icx = self
            .instruction_context
            .as_mut()
            .expect("Stepping before instruction context exists");

        if let Some(step_mirror) = &mut self.step_mirror {
            for acc in step_mirror.check_diffs(order) {
                icx.account_diff(acc);
            }
        }

        icx.step(program_manager, order, i, mem, reg);

        if !self.step_executed {
            self.step_executed = true;
        }
    }

    pub fn raw_account_load(&mut self, data: crate::account_reads::types::TreeAccountLoad) {
        let icx = self
            .instruction_context
            .as_mut()
            .expect("Account load before instruction context exists");
        icx.raw_account_load(data);
    }
}
