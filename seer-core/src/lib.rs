pub mod account_read_trace;
pub mod account_reads;
pub mod account_reads_persist;
pub mod analysis;
pub mod binary_lookup_tree;
pub mod contexts;
pub mod dwarf;
pub mod errors;
pub mod entrypoint_lookup;
pub mod idl;
pub mod logger;
pub mod meta;
pub mod path_resolver;
pub mod program_manager;
pub mod register_trace;
pub mod runbook;
pub mod save;
pub mod sources;
pub mod step_mirror;
pub mod target_reader;
pub mod tree;

use std::cell::RefCell;
use std::{env, path::PathBuf};

use solana_pubkey::Pubkey;
use solana_signature::Signature;

use crate::contexts::seer::SeerContext;
use crate::contexts::sources::SourcesContext;
use crate::errors::IrrecoverableError;
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

    pub fn is_active(&self) -> bool {
        self.active
    }
}

thread_local! {
    static SEER: RefCell<SeerSingleton> = RefCell::new(SeerSingleton::new());
}

pub async fn init(authority: Pubkey, network_rpc_url: Option<String>) -> Result<(), IrrecoverableError> {
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

pub fn get_cwd() -> PathBuf {
    env::current_dir().expect("env::curnet_dir failed!")
}

pub fn is_default<T>(value: &T) -> bool
where
    T: Default + PartialEq,
{
    value == &T::default()
}
