use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Campaign {
    pub owner: Pubkey,
    pub goal: u64,
    pub total_raised: u64,
    pub vault: Pubkey,
    pub is_complete: bool,
}
