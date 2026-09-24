use std::{env, path::PathBuf};

use seer_interface::{GuestAccountBackdoor, GuestMemory};
use solana_account::ReadableAccount;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use storage::Storage;

use crate::{
    contexts::{
        account::global::GlobalAccountContext, register::RegisterContext,
        register::TransactionRegisterContext, transaction::TransactionContext,
    },
    errors::IrrecoverableError,
    get_cwd,
    program_manager::types::GlobalProgramContext,
    seer_debug,
};

pub struct SeerContext {
    storage: *const Storage,
    run_id: i64,
    pub transaction_context: Option<TransactionContext>,
    pub register_context: RegisterContext,
    pub global_program_context: GlobalProgramContext,
    pub global_account_context: GlobalAccountContext,
}

impl SeerContext {
    pub fn new(_authority: Pubkey, storage: &Storage) -> Result<Self, IrrecoverableError> {
        seer_debug!("Activated in directory {}", get_cwd().to_string_lossy());

        let runtime_dir = env::var("SEER_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let dwarf_compile_dir = env::var("SEER_DWARF_COMPILE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| get_cwd());

        let global_program_context = GlobalProgramContext::init(&runtime_dir, &dwarf_compile_dir)?;
        let global_account_context = GlobalAccountContext::new();

        Ok(Self {
            storage,
            run_id: 0,
            transaction_context: None,
            register_context: RegisterContext::new(),
            global_program_context,
            global_account_context,
        })
    }

    fn storage(&self) -> &Storage {
        unsafe { &*self.storage }
    }

    fn persist_reg(&self, ix: u8, program: &Pubkey, chunk: &TransactionRegisterContext) {
        let (start_step, end_step) =
            match (chunk.trace.first_key_value(), chunk.trace.last_key_value()) {
                (Some((&first, _)), Some((&last, _))) => (first, last),
                _ => (chunk.min_order, chunk.min_order),
            };
        let accounts: Vec<(Pubkey, Vec<u8>)> = self
            .global_account_context
            .live_accounts()
            .iter()
            .map(|(k, a)| (*k, a.data().to_vec()))
            .collect();
        let elf = crate::program_elf::program_elf_bytes(&accounts, program);
        let run_id = self.run_id;
        let storage = self.storage();
        let self_hash = storage
            .blob
            .store(&serde_json::to_vec(chunk).expect("serialize register chunk"))
            .expect("store register blob");
        let program_hash = storage.blob.store(&elf).expect("store program blob");
        storage
            .db
            .insert_program(&program_hash)
            .expect("insert program");
        storage
            .db
            .insert_reg(
                run_id,
                i64::from(ix),
                i64::try_from(start_step).expect("start_step fits i64"),
                i64::try_from(end_step).expect("end_step fits i64"),
                &self_hash,
                &program_hash,
                &program.to_bytes(),
            )
            .expect("insert reg");
    }

    pub fn set_current_tx(&mut self, run_id: i64) {
        seer_debug!("New run: {run_id}");

        self.run_id = run_id;
        self.register_context.reset_for_new_transaction();
        self.transaction_context = Some(TransactionContext::new());
    }

    pub fn unset_current_tx(&mut self) {
        if let Some(tx) = self.transaction_context.take() {
            seer_debug!("Run unset: {}", self.run_id);
            let _ = tx;
        }
    }

    pub fn record_execution_failure_if_empty(
        &mut self,
        code: &str,
        message: impl Into<String>,
        component: &str,
    ) {
        if let Some(tx) = self.transaction_context.as_mut() {
            tx.set_execution_failure_if_empty(code, message, component);
        }
    }

    pub fn push_warning(&mut self, warning: impl Into<String>) {
        if let Some(tx) = self.transaction_context.as_mut() {
            tx.meta.push_warning(warning);
        }
    }

    pub fn start_instruction(&mut self, instruction: u8, fee_payer: Pubkey) {
        seer_debug!("New instruction: {:?}", instruction);

        self.transaction_context
            .as_mut()
            .expect("Instruction called before transaction context")
            .start_instruction(instruction, fee_payer);
        let run_id = self.run_id;
        self.storage()
            .db
            .insert_run_ix(run_id, i64::from(instruction))
            .expect("insert run_ix");
    }

    pub fn end_instruction(&mut self) {
        seer_debug!("Ending instruction");

        let (ix, tree) = {
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

            let ix = tx.instruction();
            let tree = tx
                .end_instruction(&self.global_program_context)
                .map(|(_, tree)| serde_json::to_vec(&tree).expect("serialize trace"));
            (ix, tree)
        };
        let hash = tree.map(|bytes| self.storage().blob.store(&bytes).expect("store trace blob"));
        let run_id = self.run_id;
        self.storage()
            .db
            .finish_run_ix(run_id, i64::from(ix), hash.as_ref())
            .expect("finish run_ix");
    }

    pub fn close_account_backdoor(&mut self) {
        seer_debug!("Closing account backdoor");

        self.global_account_context
            .close_account_backdoor_idempotent();
    }

    /// # Safety
    /// `bd` must remain valid until the matching `end_program` closes the account backdoor.
    pub unsafe fn start_program(
        &mut self,
        accounts: Vec<Pubkey>,
        data: Vec<u8>,
        program_address: Pubkey,
        bd: &dyn GuestAccountBackdoor,
    ) {
        seer_debug!("Starting program: {:?}", program_address);

        self.global_account_context
            .open_account_backdoor_idempotent(bd);

        let pending_reg = {
            let tx = self
                .transaction_context
                .as_mut()
                .expect("Starting program before transaction context");
            let pending_reg = if tx.is_cpi() {
                self.register_context
                    .flush_for_roll()
                    .map(|rx| (tx.instruction(), tx.get_current_program_address(), rx))
            } else {
                None
            };
            if tx.is_cpi() {
                self.register_context.push_invocation();
            }
            tx.start_program(accounts, data, program_address);
            pending_reg
        };
        if let Some((ix, program, rx)) = pending_reg {
            self.persist_reg(ix, &program, &rx);
        }
    }

    pub fn end_program(&mut self, program_address: Pubkey, err: Option<InstructionError>) {
        seer_debug!("Ending program: {:?}", program_address);

        let (ix, pending_reg) = {
            let tx = self
                .transaction_context
                .as_mut()
                .expect("Ending program before transaction context exists");

            if let Some(err) = err.clone() {
                tx.set_execution_error(err);
            }

            let ix = tx.instruction();
            let pending_reg = self.register_context.flush_finalize();
            self.register_context.pop_invocation_if_nested();
            tx.end_program(err);
            (ix, pending_reg)
        };
        if let Some(rx) = pending_reg {
            self.persist_reg(ix, &program_address, &rx);
        }
    }

    pub fn step<M: GuestMemory + ?Sized>(&mut self, i: u64, _: &mut M, reg: &[u64; 12]) {
        let pending_reg = {
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

            let pending_reg = self
                .register_context
                .record(tx.step_order, i, reg)
                .map(|rx| (tx.instruction(), tx.get_current_program_address(), rx));

            tx.step(&self.global_program_context, i);
            pending_reg
        };
        if let Some((ix, program, rx)) = pending_reg {
            self.persist_reg(ix, &program, &rx);
        }
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
}
