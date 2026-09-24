use idl::IdlTreeParser;
use idl::{get_known_programs, SYSTEM_PROGRAM_ADDRESS};
use seer_core::{init_seer_logger, SeerLogger};
use solana_pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Once;

static INIT_SEER_LOG: Once = Once::new();

fn ensure_seer_logger() {
    INIT_SEER_LOG.call_once(|| {
        init_seer_logger(SeerLogger::from_env());
    });
}

#[test]
fn known_program_idls_can_instantiate_idl_lookup() {
    ensure_seer_logger();
    let known_programs = get_known_programs();
    let expected_program_ids = [
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        SYSTEM_PROGRAM_ADDRESS,
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
        "Stake11111111111111111111111111111111111111",
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
        "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
        "AddressLookupTab1e1111111111111111111111111",
        "ComputeBudget111111111111111111111111111111",
    ]
    .into_iter()
    .map(|id| Pubkey::from_str(id).expect("known program id must be valid pubkey"))
    .collect::<Vec<_>>();

    let actual_program_ids = known_programs
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    assert_eq!(
        actual_program_ids, expected_program_ids,
        "known programs and embedded IDLs should stay in sync",
    );
}

fn decode_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "hex input must have even length");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i.saturating_add(2)], 16).expect("valid hex byte"))
        .collect()
}

#[test]
fn token_2022_account_parse_supports_extended_account_size() {
    ensure_seer_logger();
    let known_programs = get_known_programs();
    let token_2022_program_id =
        Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").expect("valid pubkey");
    let token_2022_idl = known_programs
        .into_iter()
        .find(|(id, _)| *id == token_2022_program_id)
        .map(|(_, idl)| idl)
        .expect("Token-2022 IDL should exist in known programs");

    let account_hex = "ad6bdee6c348bdc0414f11196709a78dcdc502c9d7a9c9d266aac90187c5c5cf987e93e36c955a0bd514f619fdce4a145c673a696141f274f79b10f77a8c559600000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000207000000";
    let account_data = decode_hex(account_hex);
    assert_eq!(account_data.len(), 170);

    // Token-2022 accounts can include extension bytes beyond the base 165-byte
    // token-account layout. The parser should still match the account kind.
    let parsed_full = token_2022_idl.get_account(&account_data);
    println!("{:?}", parsed_full);
    assert!(
        parsed_full.is_some(),
        "Expected parse to succeed for 170-byte Token-2022 account payload"
    );

    // Control: legacy base-size token account should continue parsing too.
    let parsed_base = token_2022_idl.get_account(&account_data[..165]);
    println!("{:?}", parsed_base);
    assert!(
        parsed_base.is_some(),
        "Expected 165-byte base payload to match token account discriminator"
    );
}

#[test]
fn token_program_rejects_zeroed_170_bytes_payload() {
    ensure_seer_logger();
    let known_programs = get_known_programs();
    let token_program_id =
        Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").expect("valid pubkey");
    let token_idl = known_programs
        .into_iter()
        .find(|(id, _)| *id == token_program_id)
        .map(|(_, idl)| idl)
        .expect("Token program IDL should exist in known programs");

    // 170-byte account buffer with a trailing little-endian u32 marker (= 7).
    let mut account_data = vec![0u8; 170];
    account_data[166..170].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(account_data.len(), 170);

    let parsed = token_idl.get_account(&account_data);
    println!();
    assert!(
        parsed.is_none(),
        "legacy token program IDL should reject 170-byte payloads (base token size is 165)",
    );
}
