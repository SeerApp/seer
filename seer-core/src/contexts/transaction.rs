use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    artifacts::AtomicFileWriter,
    contexts::instruction::InstructionContext,
    failure::Failure,
    idl::IdlTreeParser,
    meta::TxMetadata,
    program_manager::types::GlobalProgramContext,
    tree::nodes::{
        account::TreeAccount,
        root::{RootViewChildren, TreeRoot},
    },
};

pub struct TransactionContext {
    pub meta: TxMetadata,
    pub failure: Option<Failure>,
    pub signature: Signature,
    instruction_context: Option<InstructionContext>,
    pub step_order: u64,
}

impl TransactionContext {
    pub fn new(signature: Signature) -> Self {
        Self {
            meta: TxMetadata::default(),
            failure: None,
            signature,
            instruction_context: None,
            step_order: 0,
        }
    }

    /// Keep the first observed program error as the canonical tx failure.
    pub fn set_execution_error(
        &mut self,
        err: InstructionError,
        idl: Option<&dyn IdlTreeParser>,
    ) {
        if self.failure.is_some() {
            return;
        }
        let message = idl
            .map(|parser| parser.get_error(err.clone()))
            .unwrap_or_else(|| err.to_string());
        self.failure = Some(Failure::execution(
            "instruction_error",
            message,
            "debugger",
        ));
    }

    pub fn set_execution_failure_if_empty(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        component: impl Into<String>,
    ) {
        if self.failure.is_some() {
            return;
        }
        self.failure = Some(Failure::execution(code, message, component));
    }

    pub fn get_current_program_address(&self) -> Pubkey {
        self.instruction_context
            .as_ref()
            .expect("Getting current program before instruction context")
            .get_current_program_address()
    }

    pub fn is_cpi(&self) -> bool {
        if let Some(ix) = &self.instruction_context {
            ix.is_cpi()
        } else {
            false
        }
    }

    pub fn instruction(&self) -> u8 {
        self.instruction_context
            .as_ref()
            .expect("Requesting instruction index before ix")
            .index
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
        global_program_context: &GlobalProgramContext,
        file_writer: &AtomicFileWriter,
    ) -> Option<(u8, TreeRoot<RootViewChildren>)> {
        let mut ix = self
            .instruction_context
            .take()
            .expect("Ending instruction before it exists");

        ix.finalize_tree(global_program_context);
        ix.into_trace_tree(file_writer)
    }

    pub unsafe fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
    ) {
        self.instruction_context
            .as_mut()
            .expect("Starting program before instruction context exists")
            .start_program(
                accounts,
                data,
                program_address,
                self.step_order.saturating_sub(1),
            );
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        self.instruction_context
            .as_mut()
            .expect("Ending program before instruction context exists")
            .end_program(err, self.step_order.saturating_sub(1));
    }

    pub fn log(&mut self, message: &str) {
        let order = self.step_order.saturating_sub(1);
        self.instruction_context
            .as_mut()
            .expect("Logging before transaction context exists")
            .log(message, order);
    }

    pub fn step(&mut self, global_program_context: &GlobalProgramContext, i: u64) {
        let order = self.step_order;

        let ix = self
            .instruction_context
            .as_mut()
            .expect("Stepping before instruction context exists");

        ix.step(global_program_context, order, i);

        self.step_order += 1;
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        let ix = self
            .instruction_context
            .as_mut()
            .expect("Account diff before ix exists");
        ix.account_diff(data);
    }
}
