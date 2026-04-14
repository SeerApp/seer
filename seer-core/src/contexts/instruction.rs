use seer_interface::GuestMemory;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    analysis::Analysis,
    contexts::tracer::Tracer,
    program_manager::program_manager::ProgramManager,
    tree::nodes::{RootViewChildren, TreeAccount, TreeRoot},
};

pub struct InstructionContext {
    index: u8,
    tracer: Tracer,
    analysis: Option<Analysis>,
}

impl InstructionContext {
    pub fn new(index: u8, fee_payer: Pubkey, sig: Option<Signature>) -> Self {
        Self {
            index,
            tracer: Tracer::new(fee_payer),
            analysis: sig.and_then(|s| {
                std::env::var("SEER_ANALYSIS")
                    .ok()
                    .map(|_| Analysis::new(s.to_string(), index))
            }),
        }
    }

    pub fn log(&mut self, message: &str) {
        self.tracer.log(message);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.log(message);
        }
    }

    pub fn start_program(&mut self, accounts: Vec<Pubkey>, data: Vec<u8>, program_address: Pubkey) {
        self.tracer.start_program(accounts, data, program_address);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.start_program(program_address);
        }
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        self.tracer.end_program(err.clone());

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.end_program(err);
        }
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        program_manager: &ProgramManager,
        i: u64,
        _: &mut M,
        _: &[u64; 12],
    ) {
        self.tracer.step(program_manager, i);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.step(i);
        }
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        self.tracer.account_diff(data.clone());

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.account_diff(data);
        }
    }

    pub fn finalize_tree(&mut self, program_manager: &ProgramManager) {
        self.tracer.finalize_tree(&program_manager);
    }
}

impl From<InstructionContext> for Option<(u8, TreeRoot<RootViewChildren>)> {
    fn from(value: InstructionContext) -> Self {
        Into::<Option<TreeRoot<RootViewChildren>>>::into(value.tracer).map(|tracer| {
            if let Some(analysis) = value.analysis {
                analysis.save();
            }
            (value.index, tracer)
        })
    }
}
