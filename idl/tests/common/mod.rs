//! Shared helpers for idl crate tests.

#![allow(dead_code)]

use std::path::PathBuf;

pub const SEER_TEST_SAVE_ENV: &str = "SEER_TEST_SAVE";

pub fn seer_test_save_enabled() -> bool {
    std::env::var(SEER_TEST_SAVE_ENV).is_ok()
}

#[allow(unused_macros)]
macro_rules! include_tests_fixture {
    ($path:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/",
            $path
        ))
    };
}

pub fn tests_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn codama_idl_golden_dir() -> PathBuf {
    tests_fixtures_dir().join("codama/canonical_result")
}

pub fn anchor_idl_golden_dir() -> PathBuf {
    tests_fixtures_dir().join("anchor/canonical_result")
}
