use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

#[repr(C)]
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct NftConfig {
    pub admin: Pubkey, // Who can mint NFTs
    pub bump_seed: u8, // PDA bump
}
