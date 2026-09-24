use borsh::BorshSerialize;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

#[derive(BorshSerialize)]
pub enum TreasuryInstruction {
    CreateVault,
    DepositFunds { amount: u64 },
}

pub fn create_vault_ix(
    program_id: Pubkey,
    payer: Pubkey,
    campaign: Pubkey,
    vault: Pubkey,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(campaign, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
        data: borsh::to_vec(&TreasuryInstruction::CreateVault).unwrap(),
    }
}

pub fn deposit_funds_ix(
    program_id: Pubkey,
    contributor: Pubkey,
    vault: Pubkey,
    campaign: Pubkey,
    amount: u64,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(contributor, true),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(campaign, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
        data: borsh::to_vec(&TreasuryInstruction::DepositFunds { amount }).unwrap(),
    }
}
