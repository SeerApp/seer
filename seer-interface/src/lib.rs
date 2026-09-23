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

pub fn install_hooks(hooks: SeerVmHooks) {
    let _ = HOOKS.set(hooks);
}

pub fn hooks() -> Option<&'static SeerVmHooks> {
    HOOKS.get()
}
