use std::{collections::HashMap, env, path::PathBuf};

use seer_interface::{GuestMemory, GuestStepMirror};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    contexts::transaction::TransactionContext,
    dwarf::{manager::DwarfManager, source_die::SourceDieTrace},
    entrypoint_lookup::EntrypointLookup,
    get_cwd,
    path_resolver::PathResolver,
    runbook::{generate_runbooks, save_runbooks},
    save::{save, save_meta, save_trace_tree},
    seer_debug, seer_trace,
    sources::Sources,
};

pub fn get_lookups(
    runtime_dir: &PathBuf,
    dwarf_compile_dir: &PathBuf,
) -> HashMap<Pubkey, EntrypointLookup> {
    seer_debug!(
        "Resolving paths\n\t{:?}\n\t{:?}",
        runtime_dir,
        dwarf_compile_dir
    );
    let path_resolver = PathResolver::new(dwarf_compile_dir.clone(), runtime_dir.clone());
    seer_debug!("Assembling dwarf manager");
    let dwarf_manager = DwarfManager::new(&runtime_dir.clone().join("target/deploy"));
    seer_debug!("Fetching source files");
    let source_files = dwarf_manager.get_all_source_files(&path_resolver);
    seer_debug!("Found source files\n\t{:?}", source_files);
    let sources = Sources::new(path_resolver, source_files);

    seer_debug!("About to search for {} DWARF source(s)...", sources.len());

    let mut lookups: HashMap<Pubkey, EntrypointLookup> = HashMap::new();

    for program_address in dwarf_manager.get_pubkeys() {
        seer_debug!("Building lookup for {:?}", program_address);
        let dwarf = dwarf_manager.get_dwarf(program_address).unwrap();
        let source_die_trace = SourceDieTrace::new(&dwarf, &sources);

        let sizes = source_die_trace.sizes();

        seer_debug!("Assembled Source Die Trace for program {:?} with {} traces {} parents and {} die ranges", program_address, sizes.0, sizes.1, sizes.2);

        if std::env::var("SEER_SOURCE_TRACE").ok().is_some() {
            seer_debug!("Saving source trace for program {}", program_address);
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

    seer_debug!("Successfully collected {} program source(s)", lookups.len());

    lookups
}

pub struct SeerContext {
    lookups: HashMap<Pubkey, EntrypointLookup>,
    pub transaction_context: Option<TransactionContext>,
}

impl SeerContext {
    pub fn new(authority: Pubkey) -> Self {
        seer_debug!("Activated in directory {}", get_cwd().to_string_lossy());

        let runtime_dir = env::var("SEER_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let target_deploy_dir = env::var("SEER_DWARF_COMPILE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let (txtx, main) = generate_runbooks(authority, &runtime_dir);
        save_runbooks(&runtime_dir, txtx, main);

        Self {
            lookups: get_lookups(&runtime_dir, &target_deploy_dir),
            transaction_context: None,
        }
    }

    pub fn add_lookups(&mut self, runtime_dir: &PathBuf, target_deploy_dir: &PathBuf) {
        for (key, value) in get_lookups(runtime_dir, target_deploy_dir) {
            if self.lookups.contains_key(&key) {
                panic!("Collision detected for key: {:?}", key);
            }
            self.lookups.insert(key, value);
        }
    }

    pub fn set_current_tx(&mut self, tx: Signature) {
        seer_trace!("New tx: {:?}", tx);
        self.transaction_context = Some(TransactionContext::new(tx));
    }

    pub fn unset_current_tx(&mut self) {
        if let Some(txc) = &self.transaction_context.take() {
            seer_trace!("Tx unset: {:?}", txc.signature);

            if *txc.executed() {
                save_meta(&txc.signature.to_string(), &txc.meta);
            }
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
            if let Some(mut step_mirror) = txc.step_mirror.take() {
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
