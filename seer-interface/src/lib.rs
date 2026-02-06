use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

pub trait GuestMemory {
    fn read(&mut self, addr: u64, len: u64) -> Vec<u8>;
}

pub type TransactionAccount = (Pubkey, AccountSharedData);

pub trait GuestStepMirror {
    fn get_account_at_index(&self, index: usize) -> Option<AccountSharedData>;

    fn get_account_keys(&self) -> Vec<Pubkey>;

    fn get_accounts(&self) -> Vec<(Pubkey, AccountSharedData)>;
}
