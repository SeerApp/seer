pub mod binary_lookup_tree;
pub mod contexts;
pub mod dwarf;
pub mod entrypoint_lookup;
pub mod errors;
pub mod logger;
pub mod path_resolver;
pub mod program_elf;
pub mod program_manager;
pub mod sources;
pub mod step_mirror;
pub mod target_reader;
pub mod tree;

use std::cell::RefCell;
use std::sync::Arc;
use std::{env, path::PathBuf};

use solana_pubkey::Pubkey;
use storage::Storage;

use crate::contexts::seer::SeerContext;
use crate::errors::IrrecoverableError;
pub use crate::logger::{
    init_seer_logger, seer_logger, SeerLogFormat, SeerLogger, SeerLoggerLevel,
};

pub struct SeerSingleton {
    context: Option<SeerContext>,
    active: bool,
    dropped_hooks: u64,
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
            dropped_hooks: 0,
        }
    }

    pub fn set(&mut self, run_id: i64) {
        if let Some(ctx) = &mut self.context {
            ctx.set_current_tx(run_id);
            self.active = true;
        }
    }

    pub fn unset(&mut self) {
        if self.is_active() {
            if let Some(ctx) = &mut self.context {
                ctx.unset_current_tx();
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

pub fn init(authority: [u8; 32], storage: Arc<Storage>) -> Result<(), IrrecoverableError> {
    init_seer_logger(SeerLogger::from_env());

    let ctx = SeerContext::new(Pubkey::new_from_array(authority), storage)?;

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
                f(ctx);
                return;
            }
        }
        seer.dropped_hooks = seer.dropped_hooks.saturating_add(1);
    });
}

pub fn dropped_hooks() -> u64 {
    SEER.with(|seer| seer.borrow().dropped_hooks)
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

#[cfg(test)]
mod tests {
    use super::{dropped_hooks, get};

    #[test]
    fn inactive_get_counts_a_drop() {
        let before = dropped_hooks();
        let mut ran = false;
        get(|_| ran = true);
        assert!(!ran);
        assert_eq!(dropped_hooks(), before.saturating_add(1));
    }
}
