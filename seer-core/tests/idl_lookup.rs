use std::{collections::HashMap, str::FromStr};

use seer_core::program_manager::known_programs::{build_idl_lookups, get_known_programs};
use solana_pubkey::Pubkey;

const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
const MYTEST_PROGRAM_ID: &str = "BKPmYX2xSgis75tMCm5fqpWdQyoJxhEW2EQ7EBnrngDj";

const TEST_KNOWN_PROGRAMS: [(&str, &str); 1] = [(
    MYTEST_PROGRAM_ID,
    include_str!("fixtures/idl/mytest.json"),
)];

fn build_lookup_registry() -> HashMap<Pubkey, seer_core::idl::lookup::IdlLookup> {
    let mut registry: HashMap<Pubkey, seer_core::idl::lookup::IdlLookup> =
        get_known_programs().into_iter().collect();

    for (key, lookup) in build_idl_lookups(&TEST_KNOWN_PROGRAMS) {
        registry.insert(key, lookup);
    }

    registry
}

#[test]
fn test_token_program_instruction_parse() {
    let registry = build_lookup_registry();
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let token_program_idl_lookup = registry.get(&token_program).unwrap();

    {
        let accounts = vec![
            Pubkey::from_str("Gqdd8HC3FW5dBR2F6aNZagZrGbbUiHiMBq7oA7m8CfU").unwrap(),
            Pubkey::from_str("HYTLyA85bXocKVYnQuNPFqCtF4xXfFSvC17awsonMfV9").unwrap(),
        ];
        let invoke_data: Vec<u8> = vec![
            18, 138, 129, 140, 92, 164, 216, 236, 167, 115, 236, 179, 240, 124, 136, 192, 103,
            163, 246, 23, 107, 172, 219, 65, 180, 86, 198, 48, 116, 73, 165, 107, 248,
        ];

        let data: &[u8] = &invoke_data[..];
        let parsed_ix = token_program_idl_lookup.get_instruction(&accounts, data);

        println!("{:?}", parsed_ix);
    }

    {
        let accounts = vec![
            Pubkey::from_str("Gqdd8HC3FW5dBR2F6aNZagZrGbbUiHiMBq7oA7m8CfU").unwrap(),
            Pubkey::from_str("AKfqTU9gGTCXoAjiZEpKt5x9fBwD3ZU4D3wCJ87KZqTu").unwrap(),
        ];
        let invoke_data: Vec<u8> = vec![
            6, 2, 1, 30, 185, 24, 117, 164, 97, 180, 227, 158, 14, 164, 123, 209, 60, 254, 122,
            218, 196, 232, 100, 78, 46, 201, 109, 241, 146, 1, 242, 62, 118, 123, 76,
        ];

        let data: &[u8] = &invoke_data[..];
        let parsed_ix = token_program_idl_lookup.get_instruction(&accounts, data);

        println!("{:?}", parsed_ix);
    }
}

#[test]
fn test_token_program_account_parse() {
    let registry = build_lookup_registry();
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let token_program_idl_lookup = registry.get(&token_program).unwrap();

    let invoke_data: Vec<u8> = vec![
        245, 201, 200, 145, 211, 31, 246, 6, 101, 254, 179, 93, 216, 14, 67, 81, 26, 41, 128,
        253, 201, 185, 213, 175, 39, 121, 103, 218, 161, 224, 199, 232, 138, 129, 140, 92, 164,
        216, 236, 167, 115, 236, 179, 240, 124, 136, 192, 103, 163, 246, 23, 107, 172, 219, 65,
        180, 86, 198, 48, 116, 73, 165, 107, 248, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let data: &[u8] = &invoke_data[..];
    let parsed_ax = token_program_idl_lookup.get_account(data);

    println!("{:?}", parsed_ax);
}

#[test]
fn test_system_program_instruction_parse() {
    let registry = build_lookup_registry();
    let system_program = Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap();
    let system_program_idl_lookup = registry.get(&system_program).unwrap();

    {
        let accounts = vec![
            Pubkey::from_str("AKfqTU9gGTCXoAjiZEpKt5x9fBwD3ZU4D3wCJ87KZqTu").unwrap(),
            Pubkey::from_str("2JuyPjExgYvATEGhDqLCDG6ygvp1FDYLT7a4yR91h9UX").unwrap(),
        ];
        let invoke_data: Vec<u8> = vec![
            0, 0, 0, 0, 64, 41, 30, 0, 0, 0, 0, 0, 156, 0, 0, 0, 0, 0, 0, 0, 153, 75, 17, 83, 39,
            24, 5, 235, 133, 88, 112, 6, 41, 84, 148, 131, 245, 33, 12, 251, 175, 174, 227, 18,
            145, 2, 10, 15, 247, 227, 88, 150,
        ];

        let data: &[u8] = &invoke_data[..];
        let parsed_ix = system_program_idl_lookup.get_instruction(&accounts, data);

        println!("{:?}", parsed_ix);
    }

    {
        let accounts = vec![
            Pubkey::from_str("AKfqTU9gGTCXoAjiZEpKt5x9fBwD3ZU4D3wCJ87KZqTu").unwrap(),
            Pubkey::from_str("Gqdd8HC3FW5dBR2F6aNZagZrGbbUiHiMBq7oA7m8CfU").unwrap(),
        ];
        let invoke_data: Vec<u8> = vec![
            0, 0, 0, 0, 240, 29, 31, 0, 0, 0, 0, 0, 165, 0, 0, 0, 0, 0, 0, 0, 6, 221, 246, 225,
            215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172, 28, 180, 133, 237, 95, 91,
            55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
        ];

        let data: &[u8] = &invoke_data[..];
        let parsed_ix = system_program_idl_lookup.get_instruction(&accounts, data);

        println!("{:?}", parsed_ix);
    }
}

#[test]
fn test_mytest_account_parse() {
    let registry = build_lookup_registry();
    let mytest_program = Pubkey::from_str(MYTEST_PROGRAM_ID).unwrap();
    let mytest_idl_lookup = registry.get(&mytest_program).unwrap();

    let invoke_data: Vec<u8> = vec![
        190, 42, 124, 96, 242, 52, 141, 28, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        193, 123, 163, 87, 192, 151, 1, 23, 244, 21, 1, 99, 162, 77, 243, 94, 162, 178, 173, 235,
        56, 22, 222, 124, 234, 219, 40, 93, 108, 166, 91, 157, 89, 23, 186, 46, 55, 84, 176, 244,
        91, 225, 233, 50, 101, 73, 244, 56, 236, 43, 245, 7, 216, 67, 136, 151, 92, 214, 178, 235,
        230, 166, 180, 187, 166, 93, 14, 55, 20, 230, 83, 19, 214, 214, 72, 201, 196, 154, 127,
        120, 134, 212, 13, 127, 216, 243, 170, 96, 143, 2, 156, 156, 211, 175, 223, 117, 220, 5,
        0, 0, 0, 0, 0, 0, 220, 5, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let data: &[u8] = &invoke_data[..];
    let parsed_ax = mytest_idl_lookup.get_account(data);

    println!(
        "{}",
        serde_json::to_string_pretty(&parsed_ax)
            .expect("Failed to serialize parsed account as JSON")
    );
}
