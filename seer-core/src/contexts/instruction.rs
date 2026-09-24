use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    contexts::tracer::Tracer,
    program_manager::types::GlobalProgramContext,
    tree::nodes::{
        account::TreeAccount,
        root::{RootViewChildren, TreeRoot},
    },
};

pub struct InstructionContext {
    pub index: u8,
    tracer: Tracer,
}

impl InstructionContext {
    pub fn new(index: u8, fee_payer: Pubkey) -> Self {
        Self {
            index,
            tracer: Tracer::new(fee_payer),
        }
    }

    pub fn get_current_program_address(&self) -> Pubkey {
        self.tracer.get_current_program_address()
    }

    pub fn is_cpi(&self) -> bool {
        self.tracer.has_invoke_context()
    }

    pub fn log(&mut self, message: &str, order: u64) {
        self.tracer.log(message, order);
    }

    pub fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
        order: u64,
    ) {
        self.tracer
            .start_program(accounts, data, program_address, order);
    }

    pub fn end_program(&mut self, err: Option<InstructionError>, order: u64) {
        self.tracer.end_program(err, order);
    }

    pub fn step(&mut self, global_program_context: &GlobalProgramContext, order: u64, i: u64) {
        self.tracer.step(global_program_context, i, order);
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        self.tracer.account_diff(data);
    }

    pub fn finalize_tree(&mut self, global_program_context: &GlobalProgramContext) {
        self.tracer.finalize_tree(global_program_context);
    }

    pub fn into_trace_tree(self) -> Option<(u8, TreeRoot<RootViewChildren>)> {
        let Self { index, tracer } = self;
        Into::<Option<TreeRoot<RootViewChildren>>>::into(tracer).map(|tree| (index, tree))
    }
}
