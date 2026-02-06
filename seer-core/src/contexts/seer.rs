use std::{collections::HashMap, path::PathBuf};

use seer_interface::{GuestMemory, GuestStepMirror};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    contexts::transaction::TransactionContext,
    dwarf::{manager::DwarfManager, source_die::SourceDieTrace},
    entrypoint_lookup::EntrypointLookup,
    save::{save, save_trace_tree},
    seer_trace,
    sources::Sources,
};

pub struct SeerContext {
    lookups: HashMap<Pubkey, EntrypointLookup>,
    pub transaction_context: Option<TransactionContext>,
}

impl SeerContext {
    pub fn new(source_project_root: PathBuf, deploy_folder_root: PathBuf) -> Self {
        let dwarf_manager = DwarfManager::new(deploy_folder_root);
        let sources = Sources::new(
            source_project_root.clone(),
            dwarf_manager.get_all_source_files(&source_project_root, &source_project_root),
        );

        let mut lookups: HashMap<Pubkey, EntrypointLookup> = HashMap::new();

        for program_address in dwarf_manager.get_pubkeys() {
            let dwarf = dwarf_manager.get_dwarf(program_address).unwrap();
            let source_die_trace = SourceDieTrace::new(&dwarf, &sources);

            if std::env::var("SEER_SOURCE_TRACE").ok().is_some() {
                let _ = save(
                    serde_json::to_string_pretty(&source_die_trace)
                        .ok()
                        .unwrap(),
                    format!("{}", program_address),
                    "json",
                    false,
                );
            }

            let entrypoint_lookup: EntrypointLookup = source_die_trace.into();

            lookups.insert(program_address.clone(), entrypoint_lookup);
        }

        Self {
            lookups,
            transaction_context: None,
        }
    }

    pub fn set_current_tx(&mut self, tx: Signature) {
        seer_trace!("New tx: {:?}", tx);
        self.transaction_context = Some(TransactionContext::new(tx));
    }

    pub fn unset_current_tx(&mut self) {
        if let Some(txc) = &self.transaction_context.take() {
            seer_trace!("Tx unset: {:?}", txc.signature);
        }
    }

    pub fn start_instruction(&mut self, instruction: u8, fee_payer: Pubkey) {
        seer_trace!("New instruction: {:?}", instruction);
        self.transaction_context
            .as_mut()
            .expect("Instruction called before transaction context")
            .start_instruction(instruction, fee_payer);
    }

    pub fn end_instruction(&mut self) {
        seer_trace!("Ending instruction");
        let txc = self
            .transaction_context
            .as_mut()
            .expect("Instruction ended before transaction context exists");

        if let Some((instruction, trace_tree)) = txc.end_instruction() {
            save_trace_tree(&txc.signature.to_string(), instruction, trace_tree);
        }
    }

    pub unsafe fn end_transaction_context(&mut self) {
        if let Some(txc) = self.transaction_context.as_mut() {
            if let Some(step_mirror) = txc.step_mirror.take().as_mut() {
                step_mirror.clear();
            }
        }
    }

    pub unsafe fn start_program(
        &mut self,
        program_address: Pubkey,
        step_mirror: &dyn GuestStepMirror,
    ) {
        seer_trace!("Starting program: {:?}", program_address);
        self.transaction_context
            .as_mut()
            .expect("Starting program before transaction context")
            .start_program(program_address, step_mirror);
    }

    pub fn end_program(&mut self, program_address: Pubkey, err: Option<InstructionError>) {
        seer_trace!("Ending program: {:?}", program_address);
        self.transaction_context
            .as_mut()
            .expect("Ending program before transaction context exists")
            .end_program(err)
    }

    pub fn step<M: GuestMemory>(&mut self, i: u64, mem: &mut M, reg: &[u64; 12]) {
        self.transaction_context
            .as_mut()
            .expect("Stepping before transaction context exists")
            .step(&self.lookups, i, mem, reg);
    }

    pub fn log(&mut self, message: &str) {
        seer_trace!("Log: {:?}", message);
        self.transaction_context
            .as_mut()
            .expect("Logging before transaction context exists")
            .log(message);
    }
}
