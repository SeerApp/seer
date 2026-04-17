use seer_interface::GuestMemory;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    analysis::Analysis,
    contexts::tracer::Tracer,
    program_manager::program_manager::ProgramManager,
    register_trace::{RegisterTraceCollector, REGISTER_COUNT},
    save::save_register_trace_chunk,
    tree::nodes::{RootViewChildren, TreeAccount, TreeRoot},
};

pub struct InstructionContext {
    index: u8,
    signature: Option<String>,
    tracer: Tracer,
    analysis: Option<Analysis>,
    register_trace: Option<RegisterTraceCollector>,
    /// Last register snapshot from the most recent VM step (used to seed CPI register chunks).
    last_regs: Option<[u64; REGISTER_COUNT]>,
}

impl InstructionContext {
    pub fn new(index: u8, fee_payer: Pubkey, sig: Option<Signature>) -> Self {
        let signature = sig.map(|signature| signature.to_string());
        Self {
            index,
            signature: signature.clone(),
            tracer: Tracer::new(fee_payer),
            analysis: signature.as_ref().and_then(|s| {
                std::env::var("SEER_ANALYSIS")
                    .ok()
                    .map(|_| Analysis::new(s.clone(), index))
            }),
            register_trace: signature.map(|_| RegisterTraceCollector::new()),
            last_regs: None,
        }
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
        tree_uid: u64,
        order: u64,
    ) {
        let is_cpi = self.tracer.has_invoke_context();
        let flushed_chunk = if let Some(rt) = self.register_trace.as_mut() {
            let flushed = rt.flush_on_invocation_boundary();
            if is_cpi {
                if let Some(regs) = self.last_regs {
                    rt.open_chunk_eager(tree_uid, order, &regs);
                }
            }
            flushed
        } else {
            None
        };
        if let Some(chunk) = flushed_chunk {
            self.save_register_trace_chunk(chunk);
        }
        self.tracer
            .start_program(accounts, data, program_address, tree_uid, order);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.start_program(program_address);
        }
    }

    pub fn end_program(&mut self, err: Option<InstructionError>, order: u64) {
        if let Some(rt) = self.register_trace.as_mut() {
            if let Some(chunk) = rt.flush_on_invocation_boundary() {
                self.save_register_trace_chunk(chunk);
            }
        }
        self.tracer.end_program(err.clone(), order);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.end_program(err);
        }
    }

    pub fn step<M: GuestMemory>(
        &mut self,
        program_manager: &ProgramManager,
        order: u64,
        i: u64,
        _: &mut M,
        reg: &[u64; 12],
    ) {
        self.tracer.step(program_manager, i, order);

        if let Some(analysis) = self.analysis.as_mut() {
            analysis.step(i);
        }

        self.last_regs = Some(*reg);

        if let Some(register_trace) = self.register_trace.as_mut() {
            let tree_uid = self.tracer.current_tree_uid();
            if let Some(completed_chunk) = register_trace.record(order, i, reg, tree_uid) {
                self.save_register_trace_chunk(completed_chunk);
            }
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

    fn save_register_trace_chunk(
        &self,
        chunk: crate::register_trace::PersistedRegisterTraceChunk,
    ) {
        let signature = self
            .signature
            .as_ref()
            .expect("Register trace save requires a signature");

        save_register_trace_chunk(
            signature,
            self.index,
            chunk.tree_uid,
            chunk.min_order,
            chunk.max_order,
            &chunk.chunk,
        );
    }
}

impl From<InstructionContext> for Option<(u8, TreeRoot<RootViewChildren>)> {
    fn from(value: InstructionContext) -> Self {
        let InstructionContext {
            index,
            signature,
            tracer,
            analysis,
            mut register_trace,
            last_regs: _,
        } = value;

        Into::<Option<TreeRoot<RootViewChildren>>>::into(tracer).map(|tracer| {
            if let Some(chunk) = register_trace
                .as_mut()
                .and_then(RegisterTraceCollector::finalize)
            {
                let signature = signature
                    .as_ref()
                    .expect("Register trace save requires a signature");

                save_register_trace_chunk(
                    signature,
                    index,
                    chunk.tree_uid,
                    chunk.min_order,
                    chunk.max_order,
                    &chunk.chunk,
                );
            }

            if let Some(analysis) = analysis {
                analysis.save();
            }
            (index, tracer)
        })
    }
}
