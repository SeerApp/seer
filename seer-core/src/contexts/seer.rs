use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use seer_interface::{GuestAccountBackdoor, GuestMemory};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::{
    account_reads::{
        refresh_parsed_reads::refresh_account_reads_parsed_for_instruction,
        scanner::AccountVmLayout,
        utils::save_view_account_reads_chunks,
    },
    artifacts::AtomicFileWriter,
    contexts::{
        account::global::GlobalAccountContext, register::RegisterContext,
        transaction::TransactionContext,
    },
    errors::IrrecoverableError,
    get_cwd,
    program_manager::types::GlobalProgramContext,
    runbook::generate_runbooks,
    seer_debug,
};

pub struct SeerContext {
    pub file_writer: Arc<Mutex<AtomicFileWriter>>,
    pub transaction_context: Option<TransactionContext>,
    pub register_context: RegisterContext,
    pub global_program_context: GlobalProgramContext,
    pub global_account_context: GlobalAccountContext,
}

impl SeerContext {
    pub fn new(
        authority: Pubkey,
        network_rpc_url: Option<String>,
    ) -> Result<Self, IrrecoverableError> {
        seer_debug!("Activated in directory {}", get_cwd().to_string_lossy());

        let runtime_dir = env::var("SEER_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let dwarf_compile_dir = env::var("SEER_DWARF_COMPILE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let file_writer = AtomicFileWriter::new();
        if let Some((txtx, main)) = generate_runbooks(authority, &runtime_dir) {
            file_writer.save_runbooks(&runtime_dir, txtx, main);
        } else {
            seer_debug!("Starting without target.");
        }

        let file_writer = Arc::new(Mutex::new(file_writer));
        let global_program_context = GlobalProgramContext::init(
            &runtime_dir,
            &dwarf_compile_dir,
            network_rpc_url,
            file_writer.clone(),
        )?;
        let global_account_context = GlobalAccountContext::new();

        Ok(Self {
            file_writer,
            transaction_context: None,
            register_context: RegisterContext::new(),
            global_program_context,
            global_account_context,
        })
    }

    fn lock_file_writer(
        writer: &Arc<Mutex<AtomicFileWriter>>,
    ) -> MutexGuard<'_, AtomicFileWriter> {
        writer.lock().expect("file writer lock should not be poisoned")
    }

    pub fn set_current_tx(&mut self, tx: Signature) {
        seer_debug!("New tx: {:?}", tx);

        self.register_context.reset_for_new_transaction();
        self.transaction_context = Some(TransactionContext::new(tx));
    }

    pub fn unset_current_tx(&mut self) {
        if let Some(tx) = self.transaction_context.take() {
            seer_debug!("Tx unset: {:?}", tx.signature);

            Self::lock_file_writer(&self.file_writer)
                .save_meta(&tx.signature.to_string(), &tx.meta);
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

        let tx = self
            .transaction_context
            .as_mut()
            .expect("Instruction ended before transaction context exists");

        for acc in self
            .global_account_context
            .get_changed_accounts(tx.step_order)
        {
            tx.account_diff(acc);
        }

        let w = Self::lock_file_writer(&self.file_writer);
        if let Some((instruction, trace_tree)) =
            tx.end_instruction(&self.global_program_context, &w)
        {
            let signature = tx.signature.to_string();
            let receiver_by_account = trace_tree.account_pubkey_to_idl_receiver_map();
            w.save_trace_tree(&signature, instruction, trace_tree);
            refresh_account_reads_parsed_for_instruction(
                &receiver_by_account,
                &signature,
                instruction,
                &self.global_program_context,
                &w,
            );
        }
    }

    pub fn close_account_backdoor(&mut self) {
        seer_debug!("Closing account backdoor");

        self.global_account_context
            .close_account_backdoor_idempotent();
    }

    pub unsafe fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
        bd: &dyn GuestAccountBackdoor,
    ) {
        seer_debug!("Starting program: {:?}", program_address);

        let tx = self
            .transaction_context
            .as_mut()
            .expect("Starting program before transaction context");

        self.global_program_context
            .queue_disasm_if_needed(program_address);

        self.global_account_context
            .open_account_backdoor_idempotent(bd);

        if tx.is_cpi() {
            let w = Self::lock_file_writer(&self.file_writer);
            let view_reads = self.global_account_context.drain_parsed_view_accounts(
                &self.global_program_context,
                tx.get_current_program_address(),
            );
            save_view_account_reads_chunks(
                &view_reads,
                &tx.signature.to_string(),
                tx.instruction(),
                &tx.get_current_program_address(),
                &w,
            );

            if let Some(rx) = self.register_context.flush_for_roll() {
                w.save_register_trace_chunk(
                    &tx.signature.to_string(),
                    tx.instruction(),
                    &tx.get_current_program_address(),
                    &rx,
                );
            }
            self.register_context.push_invocation();
        }

        tx.start_program(accounts, data, program_address);
    }

    pub fn end_program(&mut self, program_address: Pubkey, err: Option<InstructionError>) {
        seer_debug!("Ending program: {:?}", program_address);

        let tx = self
            .transaction_context
            .as_mut()
            .expect("Ending program before transaction context exists");
        let global_program_context = &self.global_program_context;

        if let Some(err) = err.clone() {
            let idl_lookup = global_program_context.get_idl_lookup(&program_address);
            let idl = idl_lookup
                .as_deref()
                .map(|l| l as &dyn crate::idl::IdlTreeParser);
            tx.meta.set_error(err, idl);
        }

        let w = Self::lock_file_writer(&self.file_writer);
        let view_reads = self
            .global_account_context
            .drain_parsed_view_accounts(global_program_context, program_address);

        if !view_reads.is_empty() {
            save_view_account_reads_chunks(
                &view_reads,
                &tx.signature.to_string(),
                tx.instruction(),
                &program_address,
                &w,
            );
        }

        if let Some(rx) = self.register_context.flush_finalize() {
            w.save_register_trace_chunk(
                &tx.signature.to_string(),
                tx.instruction(),
                &program_address,
                &rx,
            );
        }
        self.register_context.pop_invocation_if_nested();

        tx.end_program(err);
    }

    pub fn step<M: GuestMemory>(&mut self, i: u64, _: &mut M, reg: &[u64; 12]) {
        let tx = self
            .transaction_context
            .as_mut()
            .expect("Stepping before transaction context exists");

        for acc in self
            .global_account_context
            .get_changed_accounts(tx.step_order)
        {
            tx.account_diff(acc);
        }

        if let Some(rx) = self.register_context.record(tx.step_order, i, reg) {
            Self::lock_file_writer(&self.file_writer).save_register_trace_chunk(
                &tx.signature.to_string(),
                tx.instruction(),
                &tx.get_current_program_address(),
                &rx,
            );
        }

        tx.step(&self.global_program_context, i);
    }

    pub fn log(&mut self, message: &str) {
        seer_debug!("Log: {:?}", message);

        let tx = self
            .transaction_context
            .as_mut()
            .expect("Logging before transaction context exists");
        tx.log(message);

        for acc in self
            .global_account_context
            .get_changed_accounts(tx.step_order)
        {
            tx.account_diff(acc);
        }
    }

    pub fn capture_vm_layout(
        &mut self,
        layouts: &[AccountVmLayout],
        keys: Vec<Pubkey>,
        data_growth: u64,
    ) {
        self.global_account_context
            .capture_vm_layout(layouts, keys, data_growth);
    }

    pub fn capture_account_read(&mut self, vm_addr: u64, width: u64) {
        let tx = self
            .transaction_context
            .as_mut()
            .expect("Capturing account read before transaction context exists");

        self.global_account_context
            .capture_account_read(tx.step_order.saturating_sub(1), vm_addr, width);
    }
}
