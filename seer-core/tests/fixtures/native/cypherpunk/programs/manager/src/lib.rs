#![allow(unexpected_cfgs)]
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction,
};

mod nft_instruction;
mod state;
mod treasury_instruction;

entrypoint!(process_instruction);

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum Instruction {
    CreateCampaign { goal: u64 },
    Contribute { amount: u64 },
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = Instruction::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    match instruction {
        Instruction::CreateCampaign { goal } => create_campaign(program_id, accounts, goal),
        Instruction::Contribute { amount } => contribute(program_id, accounts, amount),
    }
}

fn create_campaign(program_id: &Pubkey, accounts: &[AccountInfo], goal: u64) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let creator = next_account_info(account_info_iter)?; // signer
    let campaign_account = next_account_info(account_info_iter)?; // writable
    let vault_account = next_account_info(account_info_iter)?; // PDA from Treasury
    let treasury_program = next_account_info(account_info_iter)?; // Treasury program
    let system_program = next_account_info(account_info_iter)?; // System program

    if !creator.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Create Campaign Account
    let campaign_size = std::mem::size_of::<state::Campaign>();
    let rent_lamports = solana_program::rent::Rent::default().minimum_balance(campaign_size);

    let create_ix = system_instruction::create_account(
        creator.key,
        campaign_account.key,
        rent_lamports,
        campaign_size as u64,
        program_id,
    );

    invoke(
        &create_ix,
        &[
            creator.clone(),
            campaign_account.clone(),
            system_program.clone(),
        ],
    )?;

    // CPI: call Treasury program to create vault
    let treasury_ix = treasury_instruction::create_vault_ix(
        *treasury_program.key,
        *creator.key,
        *campaign_account.key,
        *vault_account.key,
    );

    invoke(
        &treasury_ix,
        &[
            creator.clone(),
            campaign_account.clone(),
            vault_account.clone(),
            treasury_program.clone(),
            system_program.clone(),
        ],
    )?;

    // Initialize Campaign
    let campaign = state::Campaign {
        owner: *creator.key,
        goal,
        total_raised: 0,
        vault: *vault_account.key,
        is_complete: false,
    };

    campaign.serialize(&mut &mut campaign_account.data.borrow_mut()[..])?;
    msg!("Campaign created with goal {} lamports", goal);

    Ok(())
}

fn contribute(_: &Pubkey, accounts: &[AccountInfo], amount: u64) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let contributor = next_account_info(account_info_iter)?; // signer
    let campaign_account = next_account_info(account_info_iter)?; // writable
    let vault_account = next_account_info(account_info_iter)?; // Treasury vault PDA
    let treasury_program = next_account_info(account_info_iter)?; // Treasury program
    let nft_program = next_account_info(account_info_iter)?; // NFT Minter program
    let nft_config = next_account_info(account_info_iter)?; // NFT config PDA
    let mint_account = next_account_info(account_info_iter)?; // new mint
    let mint_authority = next_account_info(account_info_iter)?; // mint authority PDA
    let recipient_token_account = next_account_info(account_info_iter)?; // contributor ATA
    let token_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let rent_sysvar = next_account_info(account_info_iter)?;

    if !contributor.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // CPI: deposit funds to Treasury
    let deposit_ix = treasury_instruction::deposit_funds_ix(
        *treasury_program.key,
        *contributor.key,
        *vault_account.key,
        *campaign_account.key,
        amount,
    );

    invoke(
        &deposit_ix,
        &[
            contributor.clone(),
            vault_account.clone(),
            campaign_account.clone(),
            treasury_program.clone(),
            system_program.clone(),
        ],
    )?;

    // CPI: mint NFT immediately
    let mint_ix = nft_instruction::mint_badge_ix(
        *nft_program.key,
        *contributor.key, // or campaign owner/admin
        *nft_config.key,
        *mint_account.key,
        *mint_authority.key,
        *contributor.key,
        *recipient_token_account.key,
        *token_program.key,
        *system_program.key,
        *rent_sysvar.key,
    );

    invoke(
        &mint_ix,
        &[
            contributor.clone(),
            nft_config.clone(),
            mint_account.clone(),
            mint_authority.clone(),
            contributor.clone(), // recipient
            recipient_token_account.clone(),
            token_program.clone(),
            system_program.clone(),
            rent_sysvar.clone(),
            nft_program.clone(),
        ],
    )?;

    // Update campaign state
    let mut campaign = state::Campaign::try_from_slice(&campaign_account.data.borrow())?;
    campaign.total_raised += amount;
    campaign.serialize(&mut &mut campaign_account.data.borrow_mut()[..])?;

    msg!(
        "Contribution {} lamports received from {}. NFT minted.",
        amount,
        contributor.key
    );

    Ok(())
}
