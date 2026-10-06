use std::collections::HashMap;
use std::sync::Arc;
use std::{env, path::PathBuf};

use hooks::{GuestAccountBackdoor, GuestMemory};
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
    seer_debug, seer_info,
};

pub struct SeerContext {
    storage: Arc<Storage>,
    run_id: i64,
    state_accounts: Vec<(Pubkey, Vec<u8>)>,
    /// program id → content hash of unwrapped ELF. One store per program per run.
    program_hash: HashMap<Pubkey, [u8; 32]>,
    pub transaction_context: Option<TransactionContext>,
    pub register_context: RegisterContext,
    pub global_program_context: GlobalProgramContext,
    pub global_account_context: GlobalAccountContext,
}

impl SeerContext {
    pub fn new(_authority: Pubkey, storage: Arc<Storage>) -> Result<Self, IrrecoverableError> {
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
            state_accounts: Vec::new(),
            program_hash: HashMap::new(),
            transaction_context: None,
            register_context: RegisterContext::new(),
            global_program_context,
            global_account_context,
        })
    }

    fn persist_reg(&mut self, ix: u8, program: &Pubkey, chunk: &TransactionRegisterContext) {
        let (start_step, end_step) =
            match (chunk.trace.first_key_value(), chunk.trace.last_key_value()) {
                (Some((&first, _)), Some((&last, _))) => (first, last),
                _ => (chunk.min_order, chunk.min_order),
            };
        let program_hash = self.program_blob_hash(program);
        let run_id = self.run_id;
        let storage = &self.storage;
        let self_hash = storage
            .blob
            .store(&serde_json::to_vec(chunk).expect("serialize register chunk"))
            .expect("store register blob");
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

    fn program_blob_hash(&mut self, program: &Pubkey) -> [u8; 32] {
        if let Some(&h) = self.program_hash.get(program) {
            return h;
        }
        let elf = crate::program_elf::program_elf_bytes(&self.state_accounts, program);
        assert!(
            !elf.is_empty(),
            "no ELF for {program} in run {} state",
            self.run_id
        );
        let storage = &self.storage;
        let h = storage.blob.store(&elf).expect("store program blob");
        storage.db.insert_program(&h).expect("insert program");
        self.program_hash.insert(*program, h);
        h
    }

    pub fn set_current_tx(&mut self, run_id: i64) {
        seer_info!("New run: {run_id}");

        self.run_id = run_id;
        self.state_accounts = crate::program_elf::run_state_accounts(&self.storage, run_id);
        self.program_hash.clear();
        self.register_context.reset_for_new_transaction();
        self.transaction_context = Some(TransactionContext::new());
    }

    pub fn unset_current_tx(&mut self) {
        if self.transaction_context.take().is_some() {
            seer_info!("Run unset: {}", self.run_id);
        }
        self.state_accounts.clear();
        self.program_hash.clear();
    }

    pub fn start_instruction(&mut self, instruction: u8, fee_payer: Pubkey) {
        seer_info!("New instruction: {:?}", instruction);

        self.transaction_context
            .as_mut()
            .expect("Instruction called before transaction context")
            .start_instruction(instruction, fee_payer);
        let run_id = self.run_id;
        self.storage
            .db
            .insert_run_ix(run_id, i64::from(instruction))
            .expect("insert run_ix");
    }

    pub fn end_instruction(&mut self) {
        seer_info!("Ending instruction");

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
        let hash = tree.map(|bytes| self.storage.blob.store(&bytes).expect("store trace blob"));
        let run_id = self.run_id;
        self.storage
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
        seer_info!("Starting program: {:?}", program_address);

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
        seer_info!("Ending program: {:?}", program_address);

        let (ix, pending_reg) = {
            let tx = self
                .transaction_context
                .as_mut()
                .expect("Ending program before transaction context exists");

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
