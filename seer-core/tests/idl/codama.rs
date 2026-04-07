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
use seer_core::idl::{IdlIssue, IdlTreeParser};
use seer_core::{init_seer_logger, SeerLogger};
use serde::Serialize;

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

// IdlIssue coverage (Codama IDL): integration tests below exercise these variants at least once:
// - `InstructionNonUnaryDiscriminator` — `test_instruction_non_unary_discriminator_records_and_skips`
// - `InvalidLayoutBytesOrStringWithoutLength` — `test_bad_layout_instruction_skipped_and_issue_recorded`
// - `InsufficientBytes` — `test_truncated_instruction_buffer_no_panic`
// Dedup / non-unary logging — `idl::issues` unit tests (`note_*`, `note_generic_dedupes`).

fn assert_or_update_fixture(name: &str, value: &impl Serialize) {
    let dir = codama_idl_golden_dir();
    let path = dir.join(name);
    let pretty = serde_json::to_string_pretty(value).expect("serialize fixture JSON");

    if seer_test_save_enabled() {
        fs::create_dir_all(&dir).expect("create fixture dir");
        fs::write(&path, pretty).unwrap_or_else(|e| {
            panic!("write fixture {}: {e}", path.display());
        });
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
fn test_instruction_non_unary_discriminator_records_and_skips() {
    let lookup = two_discriminators_instruction_lookup();
    let issues = lookup.sorted_idl_issues();
    assert!(
        issues.iter().any(|i| matches!(
            i,
            IdlIssue::InstructionNonUnaryDiscriminator { instruction, .. }
                if instruction == "weirdIx"
        )),
        "expected InstructionNonUnaryDiscriminator for weirdIx, got {issues:?}"
    );
    assert!(
        lookup.get_instruction(&[0u8]).is_none(),
        "instruction with multiple discriminators must be skipped permanently"
    );
}

#[test]
fn test_bad_layout_instruction_skipped_and_issue_recorded() {
    let lookup = bad_layout_instruction_lookup();
    let issues = lookup.sorted_idl_issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IdlIssue::InvalidLayoutBytesOrStringWithoutLength { .. })),
        "expected invalid bytes/string layout issue, got {issues:?}"
    );
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
    let issues = lookup.sorted_idl_issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IdlIssue::InsufficientBytes { .. })),
        "truncated parse should record InsufficientBytes, got {issues:?}"
    );
}

#[test]
fn test_codama_instruction_stops_on_malformed_arg_and_keeps_prefix_args() {
    let lookup = system_lookup();
    // `createAccount`: discriminator(u32) + lamports(u64) + space(u64) + programAddress(pubkey).
    // Truncate pubkey bytes so first 2 args decode and third fails.
    let invoke_data: Vec<u8> = vec![
        0, 0, 0, 0, 64, 41, 30, 0, 0, 0, 0, 0, 156, 0, 0, 0, 0, 0, 0, 0, 153, 75, 17, 83, 39,
        24, 5, 235, 133, 88, 112, 6,
    ];
    let parsed_ix = lookup
        .get_instruction(&invoke_data)
        .expect("instruction discriminator should match");
    assert_eq!(parsed_ix.name, "createAccount");
    assert_eq!(parsed_ix.args.len(), 2, "must keep only decoded prefix args");
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

    let issues = lookup.sorted_idl_issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IdlIssue::InsufficientBytes { .. })),
        "expected InsufficientBytes, got {issues:?}"
    );
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
