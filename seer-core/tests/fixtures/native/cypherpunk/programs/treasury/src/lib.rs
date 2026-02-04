#![allow(unexpected_cfgs)]
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    system_instruction,
    sysvar::rent::Rent,
};
mod state;

entrypoint!(process_instruction);

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum Instruction {
    CreateVault,
    DepositFunds { amount: u64 },
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = Instruction::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    match instruction {
        Instruction::CreateVault => process_create_vault(program_id, accounts),
        Instruction::DepositFunds { amount } => process_deposit_funds(program_id, accounts, amount),
    }
}

fn process_create_vault(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    // Expected accounts
    let payer = next_account_info(account_info_iter)?; // [signer] user or campaign creator
    let campaign_account = next_account_info(account_info_iter)?; // [readonly] campaign pubkey
    let vault_account = next_account_info(account_info_iter)?; // [writable] PDA
    let system_program = next_account_info(account_info_iter)?; // [] system program

    // Derive expected PDA
    let (expected_pda, bump) =
        Pubkey::find_program_address(&[b"vault", campaign_account.key.as_ref()], program_id);

    if expected_pda != *vault_account.key {
        msg!("Error: Invalid vault PDA provided");
        return Err(ProgramError::InvalidSeeds);
    }

    // Allocate space for Vault struct
    let vault_size = std::mem::size_of::<state::Vault>();

    // Create account instruction
    let create_ix = system_instruction::create_account(
        payer.key,
        vault_account.key,
        1.max(Rent::default().minimum_balance(vault_size)), // rent exempt
        vault_size as u64,
        program_id,
    );

    // Create PDA account signed by program
    invoke_signed(
        &create_ix,
        &[payer.clone(), vault_account.clone(), system_program.clone()],
        &[&[b"vault", campaign_account.key.as_ref(), &[bump]]],
    )?;

    // Initialize vault data
    let vault_data = state::Vault {
        campaign: *campaign_account.key,
        balance: 0,
        bump_seed: bump,
    };

    vault_data.serialize(&mut &mut vault_account.data.borrow_mut()[..])?;

    msg!("Vault created for campaign: {}", campaign_account.key);
    Ok(())
}

fn process_deposit_funds(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    let contributor = next_account_info(account_info_iter)?; // signer
    let vault_account = next_account_info(account_info_iter)?; // writable PDA
    let campaign_account = next_account_info(account_info_iter)?; // readonly
    let system_program = next_account_info(account_info_iter)?; // readonly

    // Validate signer
    if !contributor.is_signer {
        msg!("Error: Contributor must sign the transaction");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Validate vault PDA
    let (expected_vault_pda, _bump) =
        Pubkey::find_program_address(&[b"vault", campaign_account.key.as_ref()], program_id);

    if expected_vault_pda != *vault_account.key {
        msg!("Error: Invalid vault PDA");
        return Err(ProgramError::InvalidSeeds);
    }

    // Perform transfer of lamports
    msg!(
        "Transferring {} lamports from contributor to vault...",
        amount
    );

    let transfer_ix = system_instruction::transfer(contributor.key, vault_account.key, amount);

    invoke(
        &transfer_ix,
        &[
            contributor.clone(),
            vault_account.clone(),
            system_program.clone(),
        ],
    )?;

    // Update vault balance in account data
    let mut vault_account_data = vault_account.data.borrow_mut();
    let mut vault_data = state::Vault::try_from_slice(&vault_account_data)?;

    vault_data.balance = vault_data
        .balance
        .checked_add(amount)
        .ok_or(ProgramError::InvalidInstructionData)?;

    vault_data.serialize(&mut &mut vault_account_data[..])?;

    msg!(
        "Deposit successful. New vault balance: {} lamports",
        vault_data.balance
    );

    Ok(())
}
