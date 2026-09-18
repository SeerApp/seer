use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use bincode::serialized_size;
use rusqlite::Connection;
use seer_interface::{GuestAccountBackdoor, GuestMemory};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_instruction::error::InstructionError;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;

use crate::{
    artifacts::{layout::register_trace_steps_on_disk, AtomicFileWriter},
    contexts::{
        account::global::GlobalAccountContext, register::RegisterContext,
        register::TransactionRegisterContext, transaction::TransactionContext,
    },
    errors::IrrecoverableError,
    get_cwd,
    program_manager::types::GlobalProgramContext,
    runbook::generate_runbooks,
    seer_debug,
};

pub struct SeerContext {
    conn: *const Connection,
    run_id: i64,
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
        conn: &Connection,
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
            conn,
            run_id: 0,
            file_writer,
            transaction_context: None,
            register_context: RegisterContext::new(),
            global_program_context,
            global_account_context,
        })
    }

    fn conn(&self) -> &Connection {
        unsafe { &*self.conn }
    }

    fn lock_file_writer(writer: &Arc<Mutex<AtomicFileWriter>>) -> MutexGuard<'_, AtomicFileWriter> {
        writer
            .lock()
            .expect("file writer lock should not be poisoned")
    }

    fn persist_reg(&self, ix: u8, program: &Pubkey, chunk: &TransactionRegisterContext) {
        let (start_step, end_step) = register_trace_steps_on_disk(chunk);
        let self_hash = storage::blobs::store_blob(
            &serde_json::to_vec(chunk).expect("serialize register chunk"),
        )
        .expect("store register blob");
        let program_hash = storage::blobs::store_blob(&elf_bytes(
            &self.global_account_context.live_accounts(),
            program,
        ))
        .expect("store program blob");
        storage::db::insert_program(self.conn(), &program_hash).expect("insert program");
        storage::db::insert_reg(
            self.conn(),
            self.run_id,
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
        storage::db::insert_run_ix(self.conn(), self.run_id, i64::from(instruction))
            .expect("insert run_ix");
    }

    pub fn end_instruction(&mut self) {
        seer_debug!("Ending instruction");

        let (ix, hash) = {
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
            let ix = tx.instruction();
            let hash = tx
                .end_instruction(&self.global_program_context, &w)
                .map(|(_, tree)| {
                    storage::blobs::store_blob(
                        &serde_json::to_vec(&tree).expect("serialize trace"),
                    )
                    .expect("store trace blob")
                });
            (ix, hash)
        };
        storage::db::finish_run_ix(self.conn(), self.run_id, i64::from(ix), hash.as_ref())
            .expect("finish run_ix");
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

        self.global_program_context
            .queue_disasm_if_needed(program_address);

        self.global_account_context
            .open_account_backdoor_idempotent(bd);

        let pending_reg = {
            let tx = self
                .transaction_context
                .as_mut()
                .expect("Starting program before transaction context");
            let pending_reg = if tx.is_cpi() {
                self.register_context.flush_for_roll().map(|rx| {
                    (
                        tx.instruction(),
                        tx.get_current_program_address(),
                        rx,
                    )
                })
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
            let global_program_context = &self.global_program_context;

            if let Some(err) = err.clone() {
                let idl_lookup = global_program_context.get_idl_lookup(&program_address);
                let idl = idl_lookup
                    .as_deref()
                    .map(|l| l as &dyn crate::idl::IdlTreeParser);
                tx.set_execution_error(err, idl);
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

    pub fn step<M: GuestMemory>(&mut self, i: u64, _: &mut M, reg: &[u64; 12]) {
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

            let pending_reg = self.register_context.record(tx.step_order, i, reg).map(|rx| {
                (
                    tx.instruction(),
                    tx.get_current_program_address(),
                    rx,
                )
            });

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

fn elf_bytes(accounts: &[(Pubkey, AccountSharedData)], program_id: &Pubkey) -> Vec<u8> {
    const ELF_MAGIC: &[u8; 4] = b"\x7FELF";
    let Some((_, program)) = accounts.iter().find(|(k, _)| k == program_id) else {
        return Vec::new();
    };
    let data = program.data();
    if data.starts_with(ELF_MAGIC) {
        return data.to_vec();
    }
    let Ok(UpgradeableLoaderState::Program {
        programdata_address,
    }) = bincode::deserialize(data)
    else {
        return data.to_vec();
    };
    let programdata_address = Pubkey::new_from_array(programdata_address.to_bytes());
    let Some((_, programdata)) = accounts.iter().find(|(k, _)| k == &programdata_address) else {
        return Vec::new();
    };
    let bytes = programdata.data();
    let offset = match bincode::deserialize(bytes) {
        Ok(UpgradeableLoaderState::ProgramData {
            upgrade_authority_address,
            ..
        }) => {
            if upgrade_authority_address.is_some() {
                UpgradeableLoaderState::size_of_programdata_metadata()
            } else {
                UpgradeableLoaderState::size_of_programdata_metadata()
                    .saturating_sub(serialized_size(&Pubkey::default()).unwrap_or(0) as usize)
            }
        }
        _ => return Vec::new(),
    };
    bytes.get(offset..).unwrap_or(&[]).to_vec()
}
