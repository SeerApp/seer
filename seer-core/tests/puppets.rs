mod common;

use std::collections::HashMap;

use seer_core::{
    dwarf::{manager::DwarfManager, source_die::SourceDieTrace},
    entrypoint_lookup::EntrypointLookup,
    save::{load_trace_tree, save_trace_tree},
    sources::Sources,
};
use solana_pubkey::Pubkey;

use crate::common::{get_analysis_directories, run_tx};

#[test]
fn test_instruction_context() {
    let (source_project_root, cwd, deploy_folder_root, analysis_root, canonical_result_root) =
        get_analysis_directories(
            "anchor/puppets",
            "/Users/vasilygerrans/Desktop/work/Seer/code/puppets/examples/tutorial/basic-3",
        );

    let fee_payer = Pubkey::new_unique();
    let sig: String =
        "C8P1zJQbsoThR9QrswLCg34shp4yWaEfyzMUtA4S5VnwSRaznzCK95QzZWLZ1R7tYaPLgindDaF7Au2CK8C6gfS"
            .to_string();

    let dwarf_manager = DwarfManager::new(deploy_folder_root);
    let sources = Sources::new(
        source_project_root.clone(),
        dwarf_manager.get_all_source_files(&cwd, &source_project_root),
    );

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
