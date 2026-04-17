//! Shared integration-test helpers.
//!
//! Each integration test crate uses a different subset of this module; unused items are expected.
//!
//! ## Golden fixtures (`SEER_TEST_SAVE`)
//!
//! When the environment variable [`SEER_TEST_SAVE_ENV`] is set (to any value), tests that
//! compare against checked-in JSON **write** updated files under [`tests_fixtures_dir`] instead
//! of asserting. Review the diff, then commit.
//!
//! Layout:
//! - **`tests/fixtures/idl/codama/idls/`** — Codama IDL JSON inputs (`codama_idl` test).
//! - **`tests/fixtures/idl/codama/canonical_result/`** — Codama parse goldens (`codama_idl` test).
//! - **`tests/fixtures/idl/anchor/idls/`** — Anchor IDL JSON inputs (`anchor_idl` test).
//! - **`tests/fixtures/idl/anchor/canonical_result/`** — Anchor parse goldens (`anchor_idl` test).
//! - **`tests/fixtures/<scenario>/canonical_result/`** — trace tree goldens (`puppets`, `cypherpunk`, …).

#![allow(dead_code)]

use std::path::PathBuf;

use seer_core::{
    analysis::{Analysis, ExecutionEvent},
    contexts::tracer::Tracer,
    program_manager::program_manager::ProgramManager,
    tree::nodes::{RootViewChildren, TreeRoot},
};
use solana_pubkey::Pubkey;

/// When set, golden tests write updated JSON under [`tests_fixtures_dir`] (see module docs).
pub const SEER_TEST_SAVE_ENV: &str = "SEER_TEST_SAVE";

pub fn seer_test_save_enabled() -> bool {
    std::env::var(SEER_TEST_SAVE_ENV).is_ok()
}

/// Relative to [`env!("CARGO_MANIFEST_DIR")`]; must stay in sync with compile-time includes (see
/// [`include_tests_fixture`]).
pub const TESTS_FIXTURES_REL: &str = "tests/fixtures";

/// `seer-core/tests/fixtures` (manifest-relative).
pub fn tests_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(TESTS_FIXTURES_REL)
}

/// `include_str!` for a UTF-8 file under `tests/fixtures/<path>` (`path` is relative to that dir).
/// Import with `#[macro_use] mod common` in the integration test crate.
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

/// Codama IDL parse goldens: `tests/fixtures/idl/codama/canonical_result/`.
pub fn codama_idl_golden_dir() -> PathBuf {
    tests_fixtures_dir().join("idl/codama/canonical_result")
}

/// Anchor IDL parse goldens: `tests/fixtures/idl/anchor/canonical_result/`.
pub fn anchor_idl_golden_dir() -> PathBuf {
    tests_fixtures_dir().join("idl/anchor/canonical_result")
}

pub fn _run_tx(
    analysis_root: &PathBuf,
    fee_payer: Pubkey,
    index: u8,
    signature: &String,
    program_manager: &ProgramManager,
) -> TreeRoot<RootViewChildren> {
    let mut tracer = Tracer::new(fee_payer);

    let analysis_trace = Analysis::load(analysis_root, signature.clone(), index);

    let mut next_uid = 0u64;
    let mut trace_order = 0u64;

    for e in analysis_trace.events {
        match e {
            ExecutionEvent::StartProgram(program_address) => {
                tracer.start_program(
                    Vec::new(),
                    Vec::new(),
                    program_address,
                    next_uid,
                    trace_order,
                );
                next_uid += 1;
            }
            ExecutionEvent::EndProgram(err) => tracer.end_program(err, trace_order),
            ExecutionEvent::AccountDiff(mut data) => {
                data.step_order = trace_order;
                tracer.account_diff(data);
            }
            ExecutionEvent::Log(log) => tracer.log(&log, trace_order),
            ExecutionEvent::Step(step) => {
                tracer.step(program_manager, step, trace_order);
                trace_order += 1;
            }
        }
    }

    let maybe_trace_tree: Option<TreeRoot<RootViewChildren>> = tracer.into();

    if let Some(trace_tree) = maybe_trace_tree {
        trace_tree
    } else {
        panic!("Some WTF happened on {:?}_{:?}", signature, index);
    }
}

pub fn get_analysis_directories(
    fixture_id: &str,
    source_project_root: &str,
) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let cwd = tests_fixtures_dir().join(fixture_id);

    let mut deploy_folder_root = cwd.clone();
    deploy_folder_root.push("target/deploy");

    let mut analysis_root = cwd.clone();
    analysis_root.push("analysis");

    let mut canonical_result_root = cwd.clone();
    canonical_result_root.push("canonical_result");

    let source_project_root = PathBuf::from(
        // corresponds to DWARF project root in fixture
        source_project_root,
    );

    (
        source_project_root,
        cwd,
        deploy_folder_root,
        analysis_root,
        canonical_result_root,
    )
}
