mod common;

use std::{collections::HashMap, str::FromStr};

use common::run_tx;
use seer_core::{
    dwarf::{manager::DwarfManager, source_die::SourceDieTrace},
    entrypoint_lookup::EntrypointLookup,
    init_seer_logger,
    path_resolver::PathResolver,
    save::{load_trace_tree, save_trace_tree},
    sources::Sources,
    SeerLogger,
};
use solana_pubkey::Pubkey;

use crate::common::get_analysis_directories;

#[test]
fn test_instruction_context() {
    init_seer_logger(SeerLogger::from_env());

    let (source_project_root, cwd, deploy_folder_root, analysis_root, canonical_result_root) =
        get_analysis_directories(
            "native/cypherpunk",
            "/Users/vasilygerrans/Desktop/work/code/seer-repo/demo",
        );

    let fee_payer = Pubkey::from_str("J6X9c9BNoWFmE7RNa3qJ7kQ3e7JwJN63hjc2X1VdQGS9")
        .ok()
        .unwrap();

    let dwarf_manager = DwarfManager::new(&deploy_folder_root);
    let path_resolver = PathResolver::new(source_project_root, cwd);
    let source_files = dwarf_manager.get_all_source_files(&path_resolver);
    let sources = Sources::new(path_resolver, source_files);

    let mut lookups: HashMap<Pubkey, EntrypointLookup> = HashMap::new();

    for program_address in dwarf_manager.get_pubkeys() {
        let dwarf = dwarf_manager.get_dwarf(program_address).unwrap();
        let source_die_trace = SourceDieTrace::new(&dwarf, &sources);

        let call_trace_lookup: EntrypointLookup = source_die_trace.into();

        lookups.insert(program_address.clone(), call_trace_lookup);
    }

    let sig =
        "JuiMHw4p3kgdBsgXK8134Vb4jaL8gfvsXYNvxfi6XgRRckZCugVNuReWUBpg1dTncXoEi8QmAz5fbHP1cvgb45Q"
            .to_string();

    for index in 0..=2 {
        let result = run_tx(&analysis_root, fee_payer, index, &sig, &lookups);
        if std::env::var("SEER_TEST_SAVE").is_ok() {
            save_trace_tree(&sig, index, result);
        } else {
            let expected = load_trace_tree(&canonical_result_root, index, &sig);
            assert!(expected == result, "Trace tree mismatch at index {}", index);
        }
    }
}
