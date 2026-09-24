pub mod binary_lookup_tree;
pub mod contexts;
pub mod dwarf;
pub mod entrypoint_lookup;
pub mod errors;
pub mod failure;
pub mod logger;
pub mod meta;
pub mod path_resolver;
pub mod program_elf;
pub mod program_manager;
pub mod register_trace;
pub mod sources;
pub mod step_mirror;
pub mod target_reader;
pub mod tree;

use std::cell::RefCell;
use std::{env, path::PathBuf};

use solana_pubkey::Pubkey;
use storage::Storage;

use crate::contexts::seer::SeerContext;
use crate::contexts::sources::SourcesContext;
use crate::errors::IrrecoverableError;
pub use crate::failure::{Failure, FailureKind};
pub use crate::logger::{
    init_seer_logger, seer_logger, SeerLogFormat, SeerLogger, SeerLoggerLevel,
};

pub struct SeerSingleton {
    context: Option<SourcesContext>,
    active: bool,
}

impl Default for SeerSingleton {
    fn default() -> Self {
        Self::new()
    }
}

impl SeerSingleton {
    pub fn new() -> Self {
        Self {
            context: None,
            active: false,
        }
    }

    pub fn set(&mut self, run_id: i64) {
        if let Some(ctx) = &mut self.context {
            ctx.get_context().set_current_tx(run_id);
            self.active = true;
        }
    }

    pub fn unset(&mut self) {
        if self.is_active() {
            if let Some(ctx) = &mut self.context {
                ctx.get_context().unset_current_tx();
                self.active = false;
            }
        }
    }

    pub fn is_inited(&self) -> bool {
        self.context.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

thread_local! {
    static SEER: RefCell<SeerSingleton> = RefCell::new(SeerSingleton::new());
}

pub fn init(authority: [u8; 32], storage: &Storage) -> Result<(), IrrecoverableError> {
    init_seer_logger(SeerLogger::from_env());

    let ctx = SourcesContext::new(Pubkey::new_from_array(authority), storage)?;

    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        seer.context = Some(ctx);
    });

    Ok(())
}

pub fn get<F>(f: F)
where
    F: FnOnce(&mut SeerContext),
{
    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        if seer.is_active() {
            if let Some(ctx) = &mut seer.context {
                f(ctx.get_context());
            }
        }
    });
}

pub fn set(run_id: i64) {
    install_vm_hooks();
    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        seer.set(run_id);
    })
}

pub fn unset() {
    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        seer.unset();
    })
}

/// Record a tx-scoped execution failure if the active tx does not already have one.
pub fn record_execution_failure_if_empty(
    code: impl Into<String>,
    message: impl Into<String>,
    component: impl Into<String>,
) {
    let code = code.into();
    let message = message.into();
    let component = component.into();
    get(|ctx| {
        ctx.record_execution_failure_if_empty(&code, message.clone(), &component);
    });
}

/// Append a warning to the active tx's in-memory notes. No-op if no tx is active.
pub fn push_warning(warning: impl Into<String>) {
    let warning = warning.into();
    get(|ctx| {
        ctx.push_warning(warning.clone());
    });
}

pub fn get_cwd() -> PathBuf {
    env::current_dir().expect("env::curnet_dir failed!")
}

pub fn install_vm_hooks() {
    hooks::install(hooks::SeerVmHooks {
        start_instruction: |ix, fee| get(|s| s.start_instruction(ix, fee)),
        end_instruction: || get(|s| s.end_instruction()),
        start_program: |accounts, data, program, bd| {
            get(|s| unsafe { s.start_program(accounts, data, program, bd) })
        },
        end_program: |program, err| get(|s| s.end_program(program, err)),
        close_account_backdoor: || get(|s| s.close_account_backdoor()),
        log: |msg| get(|s| s.log(msg)),
        step: |pc, mem, reg| get(|s| s.step(pc, mem, reg)),
    });
}
