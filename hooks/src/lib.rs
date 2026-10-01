use solana_account::AccountSharedData;
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;
use std::sync::OnceLock;

pub trait GuestMemory {
    fn read(&mut self, addr: u64, len: u64) -> Vec<u8>;
}

pub type TransactionAccount = (Pubkey, AccountSharedData);

pub trait GuestAccountBackdoor {
    fn get_account_at_index(&self, index: usize) -> Option<AccountSharedData>;

    fn get_account_keys(&self) -> Vec<Pubkey>;

    fn get_accounts(&self) -> Vec<(Pubkey, AccountSharedData)>;
}

pub struct SeerVmHooks {
    pub start_instruction: fn(u8, Pubkey),
    pub end_instruction: fn(),
    pub start_program: unsafe fn(Vec<Pubkey>, Vec<u8>, Pubkey, &dyn GuestAccountBackdoor),
    pub end_program: fn(Pubkey, Option<InstructionError>),
    pub close_account_backdoor: fn(),
    pub log: fn(&str),
    pub step: fn(u64, &mut dyn GuestMemory, &[u64; 12]),
}

static HOOKS: OnceLock<SeerVmHooks> = OnceLock::new();

pub fn install(hooks: SeerVmHooks) {
    if let Err(incoming) = HOOKS.set(hooks) {
        let installed = HOOKS.get().expect("hook table is installed");
        if !same_table(installed, &incoming) {
            panic!("second hook table differs from the installed one");
        }
    }
}

fn same_table(a: &SeerVmHooks, b: &SeerVmHooks) -> bool {
    std::ptr::fn_addr_eq(a.start_instruction, b.start_instruction)
        && std::ptr::fn_addr_eq(a.end_instruction, b.end_instruction)
        && std::ptr::fn_addr_eq(a.start_program, b.start_program)
        && std::ptr::fn_addr_eq(a.end_program, b.end_program)
        && std::ptr::fn_addr_eq(a.close_account_backdoor, b.close_account_backdoor)
        && std::ptr::fn_addr_eq(a.log, b.log)
        && std::ptr::fn_addr_eq(a.step, b.step)
}

pub fn get() -> Option<&'static SeerVmHooks> {
    HOOKS.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nop_ix(_: u8, _: Pubkey) {}
    fn nop_end_ix() {}
    unsafe fn nop_start_program(
        _: Vec<Pubkey>,
        _: Vec<u8>,
        _: Pubkey,
        _: &dyn GuestAccountBackdoor,
    ) {
    }
    fn nop_end_program(_: Pubkey, _: Option<InstructionError>) {}
    fn nop_close() {}
    fn nop_log(_: &str) {}
    fn nop_step(_: u64, _: &mut dyn GuestMemory, _: &[u64; 12]) {}
    fn other_step(_: u64, _: &mut dyn GuestMemory, _: &[u64; 12]) {
        let _ = std::hint::black_box(1u64);
    }

    fn table(step: fn(u64, &mut dyn GuestMemory, &[u64; 12])) -> SeerVmHooks {
        SeerVmHooks {
            start_instruction: nop_ix,
            end_instruction: nop_end_ix,
            start_program: nop_start_program,
            end_program: nop_end_program,
            close_account_backdoor: nop_close,
            log: nop_log,
            step,
        }
    }

    #[test]
    fn second_install_keeps_the_first_table_unless_it_differs() {
        install(table(nop_step));
        install(table(nop_step));
        let step: fn(u64, &mut dyn GuestMemory, &[u64; 12]) = nop_step;
        assert!(std::ptr::fn_addr_eq(get().unwrap().step, step));
        let panicked = std::panic::catch_unwind(|| install(table(other_step)));
        assert!(panicked.is_err());
        assert!(std::ptr::fn_addr_eq(get().unwrap().step, step));
    }
}
