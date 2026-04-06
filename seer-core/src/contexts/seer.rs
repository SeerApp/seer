use std::{env, path::PathBuf};

use seer_interface::{GuestMemory, GuestStepMirror};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    contexts::transaction::TransactionContext,
    errors::{IrrecoverableError, Warning},
    get_cwd,
    program_manager::program_manager::ProgramManager,
    runbook::{generate_runbooks, save_runbooks},
    save::{save_meta, save_trace_tree},
    seer_debug,
};

pub struct SeerContext {
    pub program_manager: ProgramManager,
    pub transaction_context: Option<TransactionContext>,
    pub warnings: Vec<Warning>,
}

impl SeerContext {
    pub fn new(authority: Pubkey) -> Result<Self, IrrecoverableError> {
        seer_debug!("Activated in directory {}", get_cwd().to_string_lossy());

        let runtime_dir = env::var("SEER_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let dwarf_compile_dir = env::var("SEER_DWARF_COMPILE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let (txtx, main) = generate_runbooks(authority, &runtime_dir);
        save_runbooks(&runtime_dir, txtx, main);
        let (program_manager, warnings) = ProgramManager::init(&runtime_dir, &dwarf_compile_dir)?;

        Ok(Self {
            program_manager,
            transaction_context: None,
            warnings,
        })
    }

    pub fn set_current_tx(&mut self, tx: Signature) {
        seer_debug!("New tx: {:?}", tx);
        self.transaction_context = Some(TransactionContext::new(tx));
    }

    pub fn unset_current_tx(&mut self) {
        if let Some(txc) = &self.transaction_context.take() {
            seer_debug!("Tx unset: {:?}", txc.signature);

            if *txc.executed() {
                save_meta(&txc.signature.to_string(), &txc.meta);
            }
        }
    }

    pub fn start_instruction(&mut self, instruction: u8, fee_payer: Pubkey) {
        seer_debug!("New instruction: {:?}", instruction);
        self.transaction_context
            .as_mut()
            .expect("Instruction called before transaction context")
            .start_instruction(instruction, fee_payer);
    }

    pub fn end_instruction(&mut self) {
        seer_debug!("Ending instruction");
        let txc = self
            .transaction_context
            .as_mut()
            .expect("Instruction ended before transaction context exists");

        if let Some((instruction, trace_tree)) = txc.end_instruction(&self.program_manager) {
            save_trace_tree(&txc.signature.to_string(), instruction, trace_tree);
        }
    }

    pub unsafe fn end_transaction_context(&mut self) {
        if let Some(txc) = self.transaction_context.as_mut() {
            if let Some(mut step_mirror) = txc.step_mirror.take() {
                step_mirror.clear();
            }
        }
    }

    pub unsafe fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
        step_mirror: &dyn GuestStepMirror,
    ) {
        seer_debug!("Starting program: {:?}", program_address);
        self.transaction_context
            .as_mut()
            .expect("Starting program before transaction context")
            .start_program(accounts, data, program_address, step_mirror);
    }

    pub fn end_program(&mut self, program_address: Pubkey, err: Option<InstructionError>) {
        seer_debug!("Ending program: {:?}", program_address);
        self.transaction_context
            .as_mut()
            .expect("Ending program before transaction context exists")
            .end_program(err)
    }

    pub fn step<M: GuestMemory>(&mut self, i: u64, mem: &mut M, reg: &[u64; 12]) {
        self.transaction_context
            .as_mut()
            .expect("Stepping before transaction context exists")
            .step(&self.program_manager, i, mem, reg);
    }

    pub fn log(&mut self, message: &str) {
        seer_debug!("Log: {:?}", message);
        self.transaction_context
            .as_mut()
            .expect("Logging before transaction context exists")
            .log(message);
    }
}
