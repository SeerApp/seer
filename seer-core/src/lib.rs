pub mod binary_lookup_tree;
pub mod call_trace_lookup;
pub mod contexts;
pub mod dwarf;
pub mod logger;
pub mod save;
pub mod sources;
pub mod step_mirror;
#[cfg(feature = "step_trace")]
pub mod step_trace;
pub mod tracer;
pub mod tree;

use std::cell::RefCell;
use std::{env, path::PathBuf};

use solana_signature::Signature;

use crate::contexts::seer::SeerContext;
use crate::logger::{init_seer_logger, seer_logger, SeerLogger, SeerLoggerLevel};

pub struct SeerSingleton {
    context: Option<SeerContext>,
    active: bool,
}

impl SeerSingleton {
    pub fn new() -> Self {
        Self {
            context: None,
            active: false,
        }
    }

    pub fn init(&mut self, source_project_root: PathBuf, deploy_folder_root: PathBuf) {
        self.context = Some(SeerContext::new(source_project_root, deploy_folder_root));
    }

    pub fn set(&mut self, tx: Signature) {
        if let Some(ctx) = self.context.as_mut() {
            ctx.set_current_tx(tx);
            self.active = true;
        }
    }

    pub fn unset(&mut self) {
        if self.is_active() {
            if let Some(ctx) = self.context.as_mut() {
                ctx.unset_current_tx();
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

/// `maybe_source_project_root` is the root of the native Solana/Anchor project with files to which we will map.
/// `maybe_deploy_folder_root` is the root of the built programs, debug data, and addresses.
/// This function must be called exactly once in the lifetime of a program, such as at the start of
/// the individual user's Seer RPC.
pub fn init(maybe_source_project_root: Option<PathBuf>, maybe_deploy_folder_root: Option<PathBuf>) {
    let source_project_root = maybe_source_project_root.unwrap_or_else(|| get_cwd());
    let deploy_folder_root =
        maybe_deploy_folder_root.unwrap_or_else(|| get_cwd().join("target").join("deploy"));

    init_seer_logger(SeerLogger::from_env());

    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        seer.init(source_project_root, deploy_folder_root);
    })
}

pub fn get<F>(f: F)
where
    F: FnOnce(&mut SeerContext),
{
    SEER.with(|seer| {
        let mut seer = seer.borrow_mut();
        if seer.is_active() {
            if let Some(ctx) = seer.context.as_mut() {
                f(ctx);
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
