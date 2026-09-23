#[macro_use]
#[path = "../common/mod.rs"]
mod common;

use std::fs;
use std::sync::Once;

use codama_nodes::NumberFormat;
use common::{codama_idl_golden_dir, seer_test_save_enabled, SEER_TEST_SAVE_ENV};
use seer_core::idl::codama::CodamaIdlLookup;
use seer_core::idl::parsed_arg::ParsedArgValue;
use seer_core::idl::types::{ParsedAccount, ParsedInstruction};
use seer_core::idl::IdlTreeParser;
use seer_core::{init_seer_logger, SeerLogger};
use serde::Serialize;
use serde_json::Value;

static INIT_SEER_LOG: Once = Once::new();

fn ensure_seer_logger() {
    INIT_SEER_LOG.call_once(|| {
        init_seer_logger(SeerLogger::from_env());
    });
}

fn token_lookup() -> CodamaIdlLookup {
    ensure_seer_logger();
    CodamaIdlLookup::from_json_str(include_str!(
        "../../src/program_manager/known_programs/token_program.json"
    ))
    .expect("token program Codama IDL must parse")
}

fn token_2022_json() -> Value {
    serde_json::from_str(include_str!(
        "../../src/program_manager/known_programs/token_2022_program.json"
    ))
    .expect("token-2022 Codama JSON must parse")
}

fn lookup_from_json_value(v: &Value) -> CodamaIdlLookup {
    CodamaIdlLookup::from_json_str(
        &serde_json::to_string(v).expect("serialize mutated Codama JSON"),
    )
    .expect("mutated Codama IDL must parse")
}

fn system_lookup() -> CodamaIdlLookup {
    ensure_seer_logger();
    CodamaIdlLookup::from_json_str(include_str!(
        "../../src/program_manager/known_programs/system_program.json"
    ))
    .expect("system program Codama IDL must parse")
}

fn mytest_lookup() -> CodamaIdlLookup {
    ensure_seer_logger();
    CodamaIdlLookup::from_json_str(include_tests_fixture!("idl/codama/idls/mytest.json"))
        .expect("mytest Codama IDL must parse")
}

fn bad_layout_instruction_lookup() -> CodamaIdlLookup {
    ensure_seer_logger();
    CodamaIdlLookup::from_json_str(include_tests_fixture!(
        "idl/codama/idls/bad_layout_instruction.json"
    ))
    .expect("bad layout Codama IDL must parse")
}

fn two_discriminators_instruction_lookup() -> CodamaIdlLookup {
    ensure_seer_logger();
    CodamaIdlLookup::from_json_str(include_tests_fixture!(
        "idl/codama/idls/two_field_discriminators.json"
    ))
    .expect("two-discriminator Codama IDL must parse")
}

fn zeroable_option_lookup() -> CodamaIdlLookup {
    ensure_seer_logger();
    CodamaIdlLookup::from_json_str(include_tests_fixture!(
        "idl/codama/idls/zeroable_option_instruction.json"
    ))
    .expect("zeroable-option Codama IDL must parse")
}

fn sentinel_lookup() -> CodamaIdlLookup {
    ensure_seer_logger();
    CodamaIdlLookup::from_json_str(include_tests_fixture!(
        "idl/codama/idls/sentinel_instruction.json"
    ))
    .expect("sentinel Codama IDL must parse")
}

// Codama schema / runtime decode diagnostics are emitted via `seer_warn!` (not collected tests).

fn assert_or_update_fixture(name: &str, value: &impl Serialize) {
    let dir = codama_idl_golden_dir();
    let path = dir.join(name);
    let pretty = serde_json::to_string_pretty(value).expect("serialize fixture JSON");

    if seer_test_save_enabled() {
        fs::create_dir_all(&dir).expect("create fixture dir");
        seer_core::artifacts::AtomicFileWriter::new().replace_bytes(&path, pretty.as_bytes());
        return;
    }

    let expected_raw = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read fixture {} (set {} in the environment to generate): {e}",
            path.display(),
            SEER_TEST_SAVE_ENV
        );
    });
    let expected: serde_json::Value =
        serde_json::from_str(&expected_raw).expect("parse expected fixture JSON");
    let actual: serde_json::Value =
        serde_json::from_str(&pretty).expect("parse actual fixture JSON");
    assert_eq!(expected, actual, "fixture mismatch: {}", path.display());
}

fn decode_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0, "hex input must have even length");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i.saturating_add(2)], 16).expect("valid hex byte"))
        .collect()
}

fn token_2022_read_411_bytes() -> Vec<u8> {
    serde_json::from_str(include_tests_fixture!(
        "idl/codama/samples/token_2022_read_411_bytes.json"
    ))
    .expect("token-2022 read sample bytes must parse")
}

fn set_token_2022_mint_extension_offset(root: &mut Value, offset: u64) {
    root["program"]["accounts"][0]["data"]["fields"][5]["type"]["item"]["prefix"][0]["type"]
        ["offset"] = Value::from(offset);
}

#[derive(Serialize)]
struct InstructionFixture {
    instruction: Option<ParsedInstruction>,
}

#[derive(Serialize)]
struct AccountFixture {
    parsed: Option<ParsedAccount>,
}

#[test]
fn test_token_program_instruction_parse() {
    let token_program_idl = token_lookup();

    {
        let invoke_data: Vec<u8> = vec![
            18, 138, 129, 140, 92, 164, 216, 236, 167, 115, 236, 179, 240, 124, 136, 192, 103, 163,
            246, 23, 107, 172, 219, 65, 180, 86, 198, 48, 116, 73, 165, 107, 248,
        ];
        let parsed_ix = token_program_idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "token_ix_initialize_account3.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }

    {
        let invoke_data: Vec<u8> = vec![
            6, 2, 1, 30, 185, 24, 117, 164, 97, 180, 227, 158, 14, 164, 123, 209, 60, 254, 122,
            218, 196, 232, 100, 78, 46, 201, 109, 241, 146, 1, 242, 62, 118, 123, 76,
        ];
        let parsed_ix = token_program_idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "token_ix_set_authority.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }
}

#[test]
fn test_token_program_account_parse() {
    let token_program_idl = token_lookup();

    let data: &[u8] = &vec![
        245, 201, 200, 145, 211, 31, 246, 6, 101, 254, 179, 93, 216, 14, 67, 81, 26, 41, 128, 253,
        201, 185, 213, 175, 39, 121, 103, 218, 161, 224, 199, 232, 138, 129, 140, 92, 164, 216,
        236, 167, 115, 236, 179, 240, 124, 136, 192, 103, 163, 246, 23, 107, 172, 219, 65, 180, 86,
        198, 48, 116, 73, 165, 107, 248, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let parsed_ax = token_program_idl.get_account(data);
    assert_or_update_fixture("token_account.json", &AccountFixture { parsed: parsed_ax });
}

#[test]
fn test_token_2022_account_parse_with_extensions_len_170() {
    ensure_seer_logger();
    let token_2022_idl = lookup_from_json_value(&token_2022_json());
    let account_hex = "ad6bdee6c348bdc0414f11196709a78dcdc502c9d7a9c9d266aac90187c5c5cf987e93e36c955a0bd514f619fdce4a145c673a696141f274f79b10f77a8c559600000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000207000000";
    let data = decode_hex(account_hex);
    assert_eq!(data.len(), 170, "expected extension-sized account payload");

    let parsed_ax = token_2022_idl.get_account(&data);
    assert!(
        parsed_ax.is_some(),
        "Token-2022 account parser should accept 165-byte base layout plus extension bytes",
    );
}

#[test]
fn test_token_2022_len_411_playground_current_idl_fails_parse() {
    ensure_seer_logger();
    let token_2022_idl = lookup_from_json_value(&token_2022_json());
    let data = token_2022_read_411_bytes();
    assert_eq!(
        data.len(),
        411,
        "expected 411-byte captured account payload"
    );

    let parsed_ax = token_2022_idl.get_account(&data);
    assert!(
        parsed_ax.is_none(),
        "current token-2022 known-program IDL should fail this 411-byte sample; use this test as a baseline playground"
    );
}

#[test]
fn test_token_2022_len_411_playground_mutating_mint_extension_offset() {
    ensure_seer_logger();
    let data = token_2022_read_411_bytes();
    assert_eq!(
        data.len(),
        411,
        "expected 411-byte captured account payload"
    );

    let mut root = token_2022_json();
    set_token_2022_mint_extension_offset(&mut root, 165);
    let lookup = lookup_from_json_value(&root);
    let parsed = lookup.get_account(&data);

    // Playground guardrail: this mutation should not panic decode paths and makes it easy to
    // iterate on Token-2022 mint extension assumptions in one place.
    let _ = parsed;
}

#[test]
fn test_system_program_instruction_parse() {
    let system_program_idl = system_lookup();

    {
        let invoke_data: Vec<u8> = vec![
            0, 0, 0, 0, 64, 41, 30, 0, 0, 0, 0, 0, 156, 0, 0, 0, 0, 0, 0, 0, 153, 75, 17, 83, 39,
            24, 5, 235, 133, 88, 112, 6, 41, 84, 148, 131, 245, 33, 12, 251, 175, 174, 227, 18,
            145, 2, 10, 15, 247, 227, 88, 150,
        ];
        let parsed_ix = system_program_idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "system_ix_create_account_1.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }

    {
        let invoke_data: Vec<u8> = vec![
            0, 0, 0, 0, 240, 29, 31, 0, 0, 0, 0, 0, 165, 0, 0, 0, 0, 0, 0, 0, 6, 221, 246, 225,
            215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172, 28, 180, 133, 237, 95, 91,
            55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
        ];
        let parsed_ix = system_program_idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "system_ix_create_account_2.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }
}

#[test]
fn test_instruction_multiple_discriminators_match_conjunctively() {
    let lookup = two_discriminators_instruction_lookup();

    // First discriminator matches (`discriminator == 0`), second does not (`size == 99`).
    assert!(
        lookup.get_instruction(&[0u8]).is_none(),
        "all instruction discriminators must match (AND semantics)"
    );

    // Both discriminators match: first byte is 0, and length is 99.
    let data = vec![0u8; 99];
    let parsed = lookup.get_instruction(&data);
    assert_eq!(
        parsed.as_ref().map(|ix| ix.name.as_str()),
        Some("weirdIx"),
        "instruction should match when all discriminators pass"
    );

    // Size matches but field discriminator fails.
    let mut bad_data = vec![0u8; 99];
    bad_data[0] = 1;
    assert!(
        lookup.get_instruction(&bad_data).is_none(),
        "any failed discriminator should prevent a match"
    );
}

#[test]
fn test_token_2022_instructions_allow_single_and_multiple_discriminators() {
    let mut root = token_2022_json();
    let instructions = root["program"]["instructions"]
        .as_array_mut()
        .expect("token-2022 instructions array");

    let single = instructions
        .iter()
        .find(|ix| {
            ix["discriminators"]
                .as_array()
                .is_some_and(|d| d.len() == 1)
        })
        .and_then(|ix| ix["name"].as_str())
        .expect("token-2022 must contain at least one single-discriminator instruction")
        .to_string();
    let multiple = instructions
        .iter()
        .find(|ix| {
            ix["discriminators"]
                .as_array()
                .is_some_and(|d| d.len() > 1)
        })
        .and_then(|ix| ix["name"].as_str())
        .expect("token-2022 must contain at least one multi-discriminator instruction")
        .to_string();

    assert!(!single.is_empty() && !multiple.is_empty());
    let _lookup = lookup_from_json_value(&root);
}

#[test]
fn test_token_2022_zero_discriminator_allowed_for_single_instruction_program() {
    let mut root = token_2022_json();
    let instructions = root["program"]["instructions"]
        .as_array_mut()
        .expect("token-2022 instructions array");
    let mut first = instructions
        .first()
        .expect("token-2022 must contain at least one instruction")
        .clone();
    let instruction_name = first["name"]
        .as_str()
        .expect("instruction name")
        .to_string();
    first["discriminators"] = Value::Array(vec![]);
    first["arguments"] = Value::Array(vec![]);
    *instructions = vec![first];

    let lookup = lookup_from_json_value(&root);
    assert_eq!(
        lookup
            .get_instruction(&[])
            .as_ref()
            .map(|ix| ix.name.as_str()),
        Some(instruction_name.as_str()),
        "single-instruction/zero-discriminator fallback should select the only instruction"
    );
}

#[test]
fn test_zeroable_option_decodes_none_and_some() {
    let lookup = zeroable_option_lookup();

    let none_data = vec![7u8; 1]
        .into_iter()
        .chain([0u8; 32])
        .collect::<Vec<u8>>();
    let none_ix = lookup
        .get_instruction(&none_data)
        .expect("instruction discriminator should match");
    assert_eq!(none_ix.name, "setAuthority");
    assert_eq!(none_ix.args.len(), 2);
    match &none_ix.args[1].value {
        ParsedArgValue::Option(v) => assert!(
            v.value.is_none(),
            "zeroed public key should decode as None for zeroableOption"
        ),
        other => panic!("expected option argument, got {other:?}"),
    }

    let some_data = vec![7u8; 1]
        .into_iter()
        .chain([1u8; 32])
        .collect::<Vec<u8>>();
    let some_ix = lookup
        .get_instruction(&some_data)
        .expect("instruction discriminator should match");
    match &some_ix.args[1].value {
        ParsedArgValue::Option(v) => assert!(
            v.value.is_some(),
            "non-zero public key should decode as Some for zeroableOption"
        ),
        other => panic!("expected option argument, got {other:?}"),
    }
}

#[test]
fn test_sentinel_decodes_none_and_some() {
    let lookup = sentinel_lookup();

    let none_ix = lookup
        .get_instruction(&[9u8, 255u8])
        .expect("instruction discriminator should match");
    assert_eq!(none_ix.name, "setSentinelValue");
    assert_eq!(none_ix.args.len(), 2);
    match &none_ix.args[1].value {
        ParsedArgValue::Option(v) => {
            assert!(v.value.is_none(), "sentinel value should decode as None")
        }
        other => panic!("expected option argument, got {other:?}"),
    }

    let some_ix = lookup
        .get_instruction(&[9u8, 7u8])
        .expect("instruction discriminator should match");
    match &some_ix.args[1].value {
        ParsedArgValue::Option(v) => assert!(
            v.value.is_some(),
            "non-sentinel value should decode as Some"
        ),
        other => panic!("expected option argument, got {other:?}"),
    }
}

#[test]
fn test_bad_layout_instruction_skipped() {
    let lookup = bad_layout_instruction_lookup();
    // Discriminator u32 = 0 / little-endian — would match `badIx` if it were not skipped.
    let data = vec![0u8, 0u8, 0u8, 0u8];
    assert!(
        lookup.get_instruction(&data).is_none(),
        "skipped instruction must not be matched"
    );
}

#[test]
fn test_truncated_instruction_buffer_no_panic() {
    let lookup = token_lookup();
    // Prefix of `token_ix_set_authority` fixture: matches instruction, then hits short buffer.
    let full = vec![
        6u8, 2, 1, 30, 185, 24, 117, 164, 97, 180, 227, 158, 14, 164, 123, 209, 60, 254, 122, 218,
        196, 232, 100, 78, 46, 201, 109, 241, 146, 1, 242, 62, 118, 123, 76,
    ];
    let _ = lookup.get_instruction(&full[..12]);
}

#[test]
fn test_codama_instruction_stops_on_malformed_arg_and_keeps_prefix_args() {
    let lookup = system_lookup();
    // `createAccount`: discriminator(u32) + lamports(u64) + space(u64) + programAddress(pubkey).
    // Truncate pubkey bytes so first 2 args decode and third fails.
    let invoke_data: Vec<u8> = vec![
        0, 0, 0, 0, 64, 41, 30, 0, 0, 0, 0, 0, 156, 0, 0, 0, 0, 0, 0, 0, 153, 75, 17, 83, 39, 24,
        5, 235, 133, 88, 112, 6,
    ];
    let parsed_ix = lookup
        .get_instruction(&invoke_data)
        .expect("instruction discriminator should match");
    assert_eq!(parsed_ix.name, "createAccount");
    assert_eq!(
        parsed_ix.args.len(),
        2,
        "must keep only decoded prefix args"
    );
    assert_eq!(parsed_ix.args[0].name, "lamports");
    assert_eq!(parsed_ix.args[1].name, "space");
    match &parsed_ix.args[0].value {
        ParsedArgValue::Number(v) => {
            assert_eq!(v.value, "1976640");
            assert_eq!(v.format, NumberFormat::U64);
        }
        other => panic!("expected Number for lamports, got {other:?}"),
    }
    match &parsed_ix.args[1].value {
        ParsedArgValue::Number(v) => {
            assert_eq!(v.value, "156");
            assert_eq!(v.format, NumberFormat::U64);
        }
        other => panic!("expected Number for space, got {other:?}"),
    }
}

#[test]
fn test_mytest_account_parse() {
    let mytest_idl = mytest_lookup();

    let invoke_data: Vec<u8> = vec![
        190, 42, 124, 96, 242, 52, 141, 28, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        193, 123, 163, 87, 192, 151, 1, 23, 244, 21, 1, 99, 162, 77, 243, 94, 162, 178, 173, 235,
        56, 22, 222, 124, 234, 219, 40, 93, 108, 166, 91, 157, 89, 23, 186, 46, 55, 84, 176, 244,
        91, 225, 233, 50, 101, 73, 244, 56, 236, 43, 245, 7, 216, 67, 136, 151, 92, 214, 178, 235,
        230, 166, 180, 187, 166, 93, 14, 55, 20, 230, 83, 19, 214, 214, 72, 201, 196, 154, 127,
        120, 134, 212, 13, 127, 216, 243, 170, 96, 143, 2, 156, 156, 211, 175, 223, 117, 220, 5, 0,
        0, 0, 0, 0, 0, 220, 5, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let parsed_ax = mytest_idl.get_account(&invoke_data);
    assert_or_update_fixture(
        "mytest_account_admin_state.json",
        &AccountFixture { parsed: parsed_ax },
    );
}
