use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

// Represents a vault tied to a specific campaign
#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Vault {
    pub campaign: Pubkey, // Campaign this vault belongs to
    pub balance: u64,     // Current lamport balance
    pub bump_seed: u8,    // PDA bump
}
