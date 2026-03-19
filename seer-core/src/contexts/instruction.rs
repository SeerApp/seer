use std::collections::HashMap;

use seer_interface::GuestMemory;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    analysis::Analysis, entrypoint_lookup::EntrypointLookup, seer_debug, tracer::Tracer, tree::nodes::{RootViewChildren, TreeAccount, TreeRoot}
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

    pub fn start_program(&mut self, program_address: Pubkey) {
        self.tracer.start_program(program_address);

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
        lookups: &HashMap<Pubkey, EntrypointLookup>,
        i: u64,
        _: &mut M,
        _: &[u64; 12],
    ) {
        self.tracer.step(lookups, i);

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

    pub fn executed(&self) -> bool {
        self.tracer.executed()
    }
}

impl From<InstructionContext> for Option<(u8, TreeRoot<RootViewChildren>)> {
    fn from(value: InstructionContext) -> Self {
        seer_debug!("Program executed: {:?}", value.tracer.executed());
        Into::<Option<TreeRoot<RootViewChildren>>>::into(value.tracer).map(|tracer| {
            if let Some(analysis) = value.analysis {
                analysis.save();
            }
            (value.index, tracer)
        })
    }
}
