#[macro_use]
#[path = "../common/mod.rs"]
mod common;

use std::fs;
use std::sync::Once;

use codama_nodes::NumberFormat;
use common::{anchor_idl_golden_dir, seer_test_save_enabled, SEER_TEST_SAVE_ENV};
use seer_core::idl::anchor::AnchorIdlLookup;
use seer_core::idl::parsed_arg::ParsedArgValue;
use seer_core::idl::types::{ParsedAccount, ParsedInstruction};
use seer_core::idl::{IdlIssue, IdlTreeParser};
use seer_core::{init_seer_logger, SeerLogger};
use serde::Serialize;
use solana_instruction_error::InstructionError;

static INIT_SEER_LOG: Once = Once::new();

fn ensure_seer_logger() {
    INIT_SEER_LOG.call_once(|| {
        init_seer_logger(SeerLogger::from_env());
    });
}

fn mytest_lookup() -> AnchorIdlLookup {
    ensure_seer_logger();
    AnchorIdlLookup::from_json_str(include_tests_fixture!("idl/anchor/idls/mytest.json"))
        .expect("mytest Anchor IDL must parse")
}

// IdlIssue coverage (Anchor IDL): see `test_truncated_anchor_instruction_records_insufficient_bytes`
// (`InsufficientBytes`) and `idl::issues` unit tests for dedup / non-unary helpers.

fn assert_or_update_fixture(name: &str, value: &impl Serialize) {
    let dir = anchor_idl_golden_dir();
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

#[derive(Serialize)]
struct ErrorFixture {
    message: String,
}

#[test]
fn test_truncated_anchor_instruction_records_insufficient_bytes() {
    ensure_seer_logger();
    let idl = AnchorIdlLookup::from_json_str(include_tests_fixture!("idl/anchor/idls/mytest.json"))
        .expect("mytest Anchor IDL must parse");
    // `approve`: 8-byte discriminator + u64 `milestone_idx`; only 4 bytes of payload after disc.
    let invoke_data: Vec<u8> = vec![
        69, 74, 217, 36, 115, 117, 97, 76, //
        0, 0, 0, 0,
    ];
    let _ = idl.get_instruction(&invoke_data);
    let issues = idl.sorted_idl_issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IdlIssue::InsufficientBytes { .. })),
        "expected InsufficientBytes, got {issues:?}"
    );
}

#[test]
fn test_anchor_instruction_stops_on_malformed_arg_and_keeps_prefix_args() {
    let idl = mytest_lookup();
    // `initialize`: discriminator + random_seed(u64) + initializer_amount([u64; 5]).
    // Truncate the final array element bytes so `random_seed` decodes and
    // `initializer_amount` fails.
    let invoke_data: Vec<u8> = vec![
        175, 175, 109, 31, 13, 152, 155, 237, 161, 134, 1, 0, 0, 0, 0, 0, 100, 0, 0, 0, 0, 0,
        0, 0, 200, 0, 0, 0, 0, 0, 0, 0, 0, 44, 1, 0, 0, 0, 0, 0, 0, 144, 1, 0, 0, 0, 0, 0, 0,
        244, 1, 0, 0,
    ];

    let parsed_ix = idl
        .get_instruction(&invoke_data)
        .expect("instruction discriminator should still match");
    assert_eq!(parsed_ix.name, "initialize");
    assert_eq!(parsed_ix.args.len(), 1, "must keep only decoded prefix args");
    assert_eq!(parsed_ix.args[0].name, "random_seed");
    match &parsed_ix.args[0].value {
        ParsedArgValue::Number(v) => {
            assert_eq!(v.value, "100001");
            assert_eq!(v.format, NumberFormat::U64);
        }
        other => panic!("expected Number for random_seed, got {other:?}"),
    }

    let issues = idl.sorted_idl_issues();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, IdlIssue::InsufficientBytes { .. })),
        "expected InsufficientBytes, got {issues:?}"
    );
}

#[test]
fn test_mytest_instruction_parse() {
    let idl = mytest_lookup();

    {
        let invoke_data: Vec<u8> = vec![216, 92, 128, 146, 202, 85, 135, 73];
        let parsed_ix = idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "mytest_ix_dispute.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }

    {
        let invoke_data: Vec<u8> =
            vec![246, 150, 236, 206, 108, 63, 58, 10, 10, 0, 0, 0, 0, 0, 0, 0];
        let parsed_ix = idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "mytest_ix_resolve.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }

    {
        let invoke_data: Vec<u8> = vec![193, 151, 203, 161, 200, 202, 32, 146];
        let parsed_ix = idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "mytest_ix_change_admin.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }

    {
        let invoke_data: Vec<u8> = vec![69, 74, 217, 36, 115, 117, 97, 76, 76, 0, 0, 0, 0, 0, 0, 0];
        let parsed_ix = idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "mytest_ix_approve.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }

    {
        let invoke_data: Vec<u8> = vec![
            175, 175, 109, 31, 13, 152, 155, 237, 161, 134, 1, 0, 0, 0, 0, 0, 100, 0, 0, 0, 0, 0,
            0, 0, 200, 0, 0, 0, 0, 0, 0, 0, 0, 44, 1, 0, 0, 0, 0, 0, 0, 144, 1, 0, 0, 0, 0, 0, 0,
            244, 1, 0, 0, 0, 0, 0, 0,
        ];
        let parsed_ix = idl.get_instruction(&invoke_data);
        assert_or_update_fixture(
            "mytest_ix_initialize.json",
            &InstructionFixture {
                instruction: parsed_ix,
            },
        );
    }
}

#[test]
fn test_mytest_account_parse() {
    let idl = mytest_lookup();

    {
        let data: Vec<u8> = vec![
            190, 42, 124, 96, 242, 52, 141, 28, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 193, 123, 163, 87, 192, 151, 1, 23, 244, 21, 1, 99, 162, 77, 243, 94, 162, 178, 173,
            235, 56, 22, 222, 124, 234, 219, 40, 93, 108, 166, 91, 157, 89, 23, 186, 46, 55, 84,
            176, 244, 91, 225, 233, 50, 101, 73, 244, 56, 236, 43, 245, 7, 216, 67, 200, 136, 151,
            92, 214, 178, 235, 230, 166, 180, 187, 166, 93, 14, 55, 20, 230, 83, 19, 214, 214, 72,
            201, 196, 154, 127, 120, 134, 212, 13, 127, 216, 243, 170, 96, 143, 2, 156, 156, 211,
            175, 223, 117, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let parsed_ax = idl.get_account(&data);
        assert_or_update_fixture(
            "mytest_account_admin_state_min.json",
            &AccountFixture { parsed: parsed_ax },
        );
    }

    {
        let data: Vec<u8> = vec![
            190, 42, 124, 96, 242, 52, 141, 28, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 193, 123, 163, 87, 192, 151, 1, 23, 244, 21, 1, 99, 162, 77, 243, 94, 162, 178, 173,
            235, 56, 22, 222, 124, 234, 219, 40, 93, 108, 166, 91, 157, 89, 23, 186, 46, 55, 84,
            176, 244, 91, 225, 233, 50, 101, 73, 244, 56, 236, 43, 245, 7, 216, 67, 136, 151, 92,
            214, 178, 235, 230, 166, 180, 187, 166, 93, 14, 55, 20, 230, 83, 19, 214, 214, 72, 201,
            196, 154, 127, 120, 134, 212, 13, 127, 216, 243, 170, 96, 143, 2, 156, 156, 211, 175,
            223, 117, 220, 5, 0, 0, 0, 0, 0, 0, 220, 5, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let parsed_ax = idl.get_account(&data);
        assert_or_update_fixture(
            "mytest_account_admin_state.json",
            &AccountFixture { parsed: parsed_ax },
        );
    }

    {
        let data: Vec<u8> = vec![
            19, 90, 148, 111, 55, 130, 229, 108, 161, 134, 1, 0, 0, 0, 0, 0, 138, 129, 140, 92,
            164, 216, 236, 167, 115, 236, 179, 240, 124, 136, 192, 103, 163, 246, 23, 107, 172,
            219, 65, 180, 86, 198, 48, 116, 73, 165, 107, 248, 177, 156, 238, 79, 138, 89, 202,
            185, 128, 44, 139, 193, 208, 199, 186, 79, 33, 24, 251, 216, 250, 220, 84, 20, 128,
            192, 179, 201, 164, 176, 254, 222, 100, 0, 0, 0, 0, 0, 0, 0, 200, 0, 0, 0, 0, 0, 0, 0,
            44, 1, 0, 0, 0, 0, 0, 0, 144, 1, 0, 0, 0, 0, 0, 0, 244, 1, 0, 0, 0, 0, 0, 0, 0, 0, 245,
            201, 200, 145, 211, 31, 246, 6, 101, 254, 179, 93, 216, 14, 67, 81, 26, 41, 128, 253,
            201, 185, 213, 175, 39, 121, 103, 218, 161, 224, 199, 232, 255, 253,
        ];
        let parsed_ax = idl.get_account(&data);
        assert_or_update_fixture(
            "mytest_account_escrow_state.json",
            &AccountFixture { parsed: parsed_ax },
        );
    }
}

#[test]
fn test_mytest_anchor_framework_error_message() {
    let idl = mytest_lookup();
    // 0x7d3 == 2003 == anchor_lang::error::ErrorCode::ConstraintRaw
    let message = idl.get_error(InstructionError::Custom(0x7d3));
    assert_or_update_fixture(
        "mytest_err_anchor_framework.json",
        &ErrorFixture { message },
    );
}
