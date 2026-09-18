pub mod analysis;
pub mod artifacts;
pub mod binary_lookup_tree;
pub mod contexts;
pub mod dwarf;
pub mod entrypoint_lookup;
pub mod errors;
pub mod failure;
pub mod idl;
pub mod logger;
pub mod meta;
pub mod path_resolver;
pub mod program_manager;
pub mod register_trace;
pub mod runbook;
pub mod sources;
pub mod step_mirror;
pub mod target_reader;
pub mod tree;

use std::cell::RefCell;
use std::{env, path::PathBuf};

use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::artifacts::AtomicFileWriter;
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

impl SeerSingleton {
    pub fn new() -> Self {
        Self {
            context: None,
            active: false,
        }
    }

    pub fn set(&mut self, tx: Signature) {
        if let Some(ctx) = &mut self.context {
            ctx.get_context().set_current_tx(tx);
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

pub async fn init(
    authority: Pubkey,
    network_rpc_url: Option<String>,
) -> Result<(), IrrecoverableError> {
    init_seer_logger(SeerLogger::from_env());

    let ctx = SourcesContext::new(authority, network_rpc_url).await?;

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

pub fn set(tx: Signature) {
    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        seer.set(tx);
    })
}

pub fn unset() {
    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        seer.unset();
    })
}

/// Persist a run-scoped internal failure (`seer/failure.json`) if none exists yet.
/// No-op when Seer is not initialized (unit tests hitting the SVM without a workspace).
pub fn write_run_failure_if_missing(failure: &Failure) {
    let inited = SEER.with(|seer| seer.borrow().is_inited());
    if !inited {
        return;
    }
    AtomicFileWriter::new().save_run_failure_if_missing(failure);
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

/// Append a warning to the active tx's `meta.json` notes. No-op if no tx is active.
pub fn push_warning(warning: impl Into<String>) {
    let warning = warning.into();
    get(|ctx| {
        ctx.push_warning(warning.clone());
    });
}

pub fn get_cwd() -> PathBuf {
    env::current_dir().expect("env::curnet_dir failed!")
}

pub fn is_default<T>(value: &T) -> bool
where
    T: Default + PartialEq,
{
    value == &T::default()
}
