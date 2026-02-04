use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

#[derive(BorshSerialize, BorshDeserialize)]
pub enum NftMinterInstruction {
    InitializeConfig,
    MintContributorBadge,
}

pub fn mint_badge_ix(
    program_id: Pubkey,
    admin: Pubkey,
    config_pda: Pubkey,
    mint_account: Pubkey,
    mint_authority_pda: Pubkey,
    recipient: Pubkey,
    token_account: Pubkey,
    token_program: Pubkey,
    system_program: Pubkey,
    rent_sysvar: Pubkey,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(admin, true),
            AccountMeta::new_readonly(config_pda, false),
            AccountMeta::new(mint_account, false),
            AccountMeta::new(mint_authority_pda, false),
            AccountMeta::new(recipient, false),
            AccountMeta::new(token_account, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(rent_sysvar, false),
        ],
        data: borsh::to_vec(&NftMinterInstruction::MintContributorBadge).unwrap(),
    }
}
