#![allow(unexpected_cfgs)]
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};
mod state;

entrypoint!(process_instruction);

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum NftMinterInstruction {
    InitializeConfig,
    MintContributorBadge,
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = NftMinterInstruction::try_from_slice(instruction_data)?;
    match instruction {
        NftMinterInstruction::InitializeConfig => initialize_config(program_id, accounts),
        NftMinterInstruction::MintContributorBadge => mint_contributor_badge(program_id, accounts),
    }
}

fn initialize_config(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let admin = next_account_info(account_info_iter)?; // signer
    let config_pda = next_account_info(account_info_iter)?; // writable PDA
    let system_program = next_account_info(account_info_iter)?; // readonly

    if !admin.is_signer {
        msg!("Admin must sign");
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (expected_pda, bump) = Pubkey::find_program_address(&[b"nft_config"], program_id);
    if expected_pda != *config_pda.key {
        msg!("Invalid config PDA");
        return Err(ProgramError::InvalidSeeds);
    }

    let rent = Rent::get()?;
    let size = std::mem::size_of::<state::NftConfig>();
    let lamports = rent.minimum_balance(size);

    let create_ix = system_instruction::create_account(
        admin.key,
        config_pda.key,
        lamports,
        size as u64,
        program_id,
    );

    invoke_signed(
        &create_ix,
        &[admin.clone(), config_pda.clone(), system_program.clone()],
        &[&[b"nft_config", &[bump]]],
    )?;

    let config_data = state::NftConfig {
        admin: *admin.key,
        bump_seed: bump,
    };
    config_data.serialize(&mut &mut config_pda.data.borrow_mut()[..])?;

    msg!("NFT config initialized by {}", admin.key);
    Ok(())
}

fn mint_contributor_badge(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let admin = next_account_info(account_info_iter)?;
    let config_pda = next_account_info(account_info_iter)?;
    let mint_account = next_account_info(account_info_iter)?;
    let mint_authority_pda = next_account_info(account_info_iter)?;
    let recipient = next_account_info(account_info_iter)?;
    let token_account = next_account_info(account_info_iter)?;
    let token_program = next_account_info(account_info_iter)?;
    let rent_sysvar = next_account_info(account_info_iter)?;

    if !admin.is_signer {
        msg!("Admin must sign the mint");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Validate config
    let config_data = state::NftConfig::try_from_slice(&config_pda.data.borrow())?;
    if config_data.admin != *admin.key {
        msg!("Unauthorized admin");
        return Err(ProgramError::IllegalOwner);
    }

    // Derive mint authority PDA
    let (expected_authority, bump) = Pubkey::find_program_address(&[b"mint_authority"], program_id);
    if expected_authority != *mint_authority_pda.key {
        msg!("Invalid mint authority PDA");
        return Err(ProgramError::InvalidSeeds);
    }

    // Create mint (SPL Token)
    let create_mint_ix = spl_token::instruction::initialize_mint(
        &spl_token::ID,
        mint_account.key,
        mint_authority_pda.key,
        None,
        0, // decimals
    )?;

    invoke(
        &create_mint_ix,
        &[
            mint_account.clone(),
            rent_sysvar.clone(),
            token_program.clone(),
        ],
    )?;

    // Mint 1 NFT to recipient
    let mint_to_ix = spl_token::instruction::mint_to(
        &spl_token::ID,
        mint_account.key,
        token_account.key,
        mint_authority_pda.key,
        &[],
        1,
    )?;

    invoke_signed(
        &mint_to_ix,
        &[
            mint_account.clone(),
            token_account.clone(),
            mint_authority_pda.clone(),
            token_program.clone(),
        ],
        &[&[b"mint_authority", &[bump]]],
    )?;

    msg!("Minted contributor badge NFT to {}", recipient.key);
    Ok(())
}
