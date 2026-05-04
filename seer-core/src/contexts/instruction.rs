use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    analysis::Analysis,
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
    analysis: Option<Analysis>,
}

impl InstructionContext {
    pub fn new(index: u8, fee_payer: Pubkey, sig: Option<Signature>) -> Self {
        let signature = sig.map(|signature| signature.to_string());
        Self {
            index,
            tracer: Tracer::new(fee_payer),
            analysis: signature.as_ref().and_then(|s| {
                std::env::var("SEER_ANALYSIS")
                    .ok()
                    .map(|_| Analysis::new(s.clone(), index))
            }),
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

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.log(message);
        }
    }

    pub fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
        order: u64,
    ) {
        // let is_cpi = self.tracer.has_invoke_context();
        // let flushed_chunk = if let Some(rt) = self.register_trace.as_mut() {
        //     let flushed = rt.flush_on_invocation_boundary(false);
        //     if is_cpi {
        //         if let Some(regs) = self.last_regs {
        //             rt.open_chunk_eager(tree_uid, order, &regs);
        //         }
        //     }
        //     flushed
        // } else {
        //     None
        // };
        // if let Some(chunk) = flushed_chunk {
        //     self.save_register_trace_chunk(chunk);
        // }
        self.tracer
            .start_program(accounts, data, program_address, order);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.start_program(program_address);
        }
    }

    pub fn end_program(&mut self, err: Option<InstructionError>, order: u64) {
        // if let Some(rt) = self.register_trace.as_mut() {
        //     if let Some(chunk) = rt.flush_on_invocation_boundary(true) {
        //         self.save_register_trace_chunk(chunk);
        //     }
        // }
        self.tracer.end_program(err.clone(), order);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.end_program(err);
        }
    }

    pub fn step(
        &mut self,
        global_program_context: &GlobalProgramContext,
        order: u64,
        i: u64,
    ) {
        self.tracer.step(global_program_context, i, order);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.step(i);
        }

        // self.last_regs = Some(*reg);

        // if let Some(register_trace) = self.register_trace.as_mut() {
        //     let tree_uid = self.tracer.current_tree_uid();
        //     if let Some(completed_chunk) = register_trace.record(order, i, reg, tree_uid) {
        //         self.save_register_trace_chunk(completed_chunk);
        //     }
        // }
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        self.tracer.account_diff(data.clone());

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.account_diff(data);
        }
    }

    pub fn finalize_tree(&mut self, global_program_context: &GlobalProgramContext) {
        self.tracer.finalize_tree(global_program_context);
    }
}

impl From<InstructionContext> for Option<(u8, TreeRoot<RootViewChildren>)> {
    fn from(value: InstructionContext) -> Self {
        let InstructionContext {
            index,
            tracer,
            analysis,
        } = value;

        Into::<Option<TreeRoot<RootViewChildren>>>::into(tracer).map(|tracer| {
            if let Some(analysis) = analysis {
                analysis.save();
            }
            (index, tracer)
        })
    }
}
