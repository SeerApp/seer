mod common;

use std::{collections::HashMap, str::FromStr};

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

use crate::common::{get_analysis_directories, run_tx};

#[test]
fn test_instruction_context() {
    init_seer_logger(SeerLogger::from_env());

    let (source_project_root, cwd, deploy_folder_root, analysis_root, canonical_result_root) =
        get_analysis_directories(
            "anchor/puppets",
            "/Users/vasilygerrans/Desktop/work/Seer/code/puppets/examples/tutorial/basic-3",
        );

    let fee_payer = Pubkey::from_str("EmPkKuzAdBZRC4jH2N12D4HJS3TZTQ41c9NB5Gzzdqrq")
        .ok()
        .unwrap();
    let sig: String =
        "5jVZw9AHxDMW346wjG1XeBu4gcvazLcQWJEVoiVNi1efhJXUF8Rmb7H8BE3PwXSBWaMMehdefMTVq8pkcnKNX6ZQ"
            .to_string();

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

    let result = run_tx(&analysis_root, fee_payer, 0, &sig, &lookups);
    if std::env::var("SEER_TEST_SAVE").is_ok() {
        save_trace_tree(&sig, 0, result);
    } else {
        let expected = load_trace_tree(&canonical_result_root, 0, &sig);
        assert!(expected == result, "Puppet trace tree mismatch at index");
    }
}
