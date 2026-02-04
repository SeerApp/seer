use std::{collections::HashMap, path::PathBuf};

use seer_core::{
    analysis::{Analysis, ExecutionEvent},
    call_trace_lookup::CallTraceLookup,
    dwarf::{manager::DwarfManager, source_die::SourceDieTrace},
    get_cwd,
    save::save_trace_tree,
    sources::Sources,
    tracer::Tracer,
    tree::nodes::{RootViewChildren, TreeRoot},
};
use solana_pubkey::Pubkey;

#[test]
fn test_instruction_context() {
    let (source_project_root, cwd, deploy_folder_root, analysis_root) = get_analysis_directories();

    let fee_payer = Pubkey::new_unique();

    let dwarf_manager = DwarfManager::new(deploy_folder_root);
    let sources = Sources::new(
        source_project_root.clone(),
        dwarf_manager.get_all_source_files(&cwd, &source_project_root),
    );

    let mut lookups: HashMap<Pubkey, CallTraceLookup> = HashMap::new();

    for program_address in dwarf_manager.get_pubkeys() {
        let dwarf = dwarf_manager.get_dwarf(program_address).unwrap();
        let source_die_trace = SourceDieTrace::new(&dwarf, &sources);

        let call_trace_lookup: CallTraceLookup = source_die_trace.into();

        lookups.insert(program_address.clone(), call_trace_lookup);
    }

    run_tx(
        &analysis_root,
        fee_payer,
        0,
        "SineH3FCq8YHoCnyvGJh3H5g2nahTFMZzynqKLnme9Djj7N84Fj4rEJaByC1qjupEcpCvSqyJYhdYVrbzwej2gq"
            .to_string(),
        &lookups,
    );
    run_tx(
        &analysis_root,
        fee_payer,
        1,
        "SineH3FCq8YHoCnyvGJh3H5g2nahTFMZzynqKLnme9Djj7N84Fj4rEJaByC1qjupEcpCvSqyJYhdYVrbzwej2gq"
            .to_string(),
        &lookups,
    );
    run_tx(
        &analysis_root,
        fee_payer,
        2,
        "SineH3FCq8YHoCnyvGJh3H5g2nahTFMZzynqKLnme9Djj7N84Fj4rEJaByC1qjupEcpCvSqyJYhdYVrbzwej2gq"
            .to_string(),
        &lookups,
    );
}

fn run_tx(
    analysis_root: &PathBuf,
    fee_payer: Pubkey,
    index: u8,
    signature: String,
    lookups: &HashMap<Pubkey, CallTraceLookup>,
) {
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
        save_trace_tree(signature, index, trace_tree);
    } else {
        panic!("Some WTF happened on {:?}_{:?}", signature, index);
    }
}

fn get_analysis_directories() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let mut cwd = get_cwd();
    cwd.push("tests/fixtures/native/cypherpunk");

    let mut deploy_folder_root = cwd.clone();
    deploy_folder_root.push("target/deploy");

    let mut analysis_root = cwd.clone();
    analysis_root.push("analysis");

    let source_project_root = PathBuf::from(
        // corresponds to DWARF project root in fixture
        "/Users/vasilygerrans/Desktop/work/code/seer-repo/demo",
    );

    (source_project_root, cwd, deploy_folder_root, analysis_root)
}
