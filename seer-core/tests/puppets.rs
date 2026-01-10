#![cfg(feature = "step_trace")]
use std::{collections::HashMap, path::PathBuf};

use seer_core::{
    call_trace_lookup::CallTraceLookup, dwarf_manager::DwarfManager, get_cwd, save::save,
    source_die_trace::SourceDieTrace, sources::Sources, step_trace::StepTrace,
};
use solana_pubkey::Pubkey;

#[test]
fn test_puppets() {
    let (source_project_root, cwd, deploy_folder_root, step_trace_root) = get_directories();

    let step_trace: StepTrace = StepTrace::load(step_trace_root);

    let dwarf_manager = DwarfManager::new(deploy_folder_root);
    let sources = Sources::new(
        source_project_root.clone(),
        dwarf_manager.get_all_source_files(&cwd, &source_project_root),
    );

    let mut lookups: HashMap<Pubkey, CallTraceLookup> = HashMap::new();

    for program_address in dwarf_manager.get_pubkeys() {
        let dwarf = dwarf_manager.get_dwarf(program_address).unwrap();
        let source_die_trace = SourceDieTrace::new(&dwarf, &sources);

        save(
            serde_json::to_string_pretty(&source_die_trace).unwrap(),
            format!("debug_{}", program_address),
            "json",
            false,
        );

        let call_trace_lookup: CallTraceLookup = source_die_trace.into();

        lookups.insert(program_address.clone(), call_trace_lookup);
    }

    for program_address in dwarf_manager.get_pubkeys() {
        let cache = step_trace.cache.get(program_address).unwrap();
        let lookup = lookups.get(program_address).unwrap();

        for i in cache {
            lookup.get_call_trace(*i);
        }
    }
}

fn get_directories() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let mut cwd = get_cwd();
    cwd.push("tests/fixtures/anchor/puppets");

    let mut deploy_folder_root = cwd.clone();
    deploy_folder_root.push("target/deploy");

    let mut step_trace_root = cwd.clone();
    step_trace_root.push("step_trace");

    let source_project_root = PathBuf::from(
        // corresponds to DWARF project root in fixture
        "/Users/vasilygerrans/Desktop/work/Seer/code/puppets/examples/tutorial/basic-3",
    );

    (
        source_project_root,
        cwd,
        deploy_folder_root,
        step_trace_root,
    )
}
