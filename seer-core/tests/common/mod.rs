use std::{collections::HashMap, path::PathBuf};

use seer_core::{
    analysis::{Analysis, ExecutionEvent},
    entrypoint_lookup::EntrypointLookup,
    get_cwd,
    tracer::Tracer,
    tree::nodes::{RootViewChildren, TreeRoot},
};
use solana_pubkey::Pubkey;

pub fn _run_tx(
    analysis_root: &PathBuf,
    fee_payer: Pubkey,
    index: u8,
    signature: &String,
    lookups: &HashMap<Pubkey, EntrypointLookup>,
) -> TreeRoot<RootViewChildren> {
    let mut tracer = Tracer::new(fee_payer);

    let analysis_trace = Analysis::load(analysis_root, signature.clone(), index);

    for e in analysis_trace.events {
        match e {
            ExecutionEvent::StartProgram(program_address) => tracer.start_program(program_address),
            ExecutionEvent::EndProgram(err) => tracer.end_program(err),
            ExecutionEvent::AccountDiff(data) => tracer.account_diff(data),
            ExecutionEvent::Log(log) => tracer.log(&log),
            ExecutionEvent::Step(step) => tracer.step(&lookups, step),
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
    let mut cwd = get_cwd();
    cwd.push(format!("tests/fixtures/{}", fixture_id));

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
