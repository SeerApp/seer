mod common;

use std::collections::HashMap;

use common::run_tx;
use seer_core::{
    dwarf::{manager::DwarfManager, source_die::SourceDieTrace},
    entrypoint_lookup::EntrypointLookup,
    save::load_trace_tree,
    sources::Sources,
};
use solana_pubkey::Pubkey;

use crate::common::get_analysis_directories;

#[test]
fn test_instruction_context() {
    let (source_project_root, cwd, deploy_folder_root, analysis_root, canonical_result_root) =
        get_analysis_directories(
            "native/cypherpunk",
            "/Users/vasilygerrans/Desktop/work/code/seer-repo/demo",
        );

    let fee_payer = Pubkey::new_unique();

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

    let sig =
        "4VU2UdbGYyE6pcr8E6bMhiT4ZPaiVyN4zVBRhBj3BqTiALcRv9S3VWZeCgFMSNfFoeEr2dyMeDrHMm3Bsqc4siQf"
            .to_string();
    assert!(
        load_trace_tree(&canonical_result_root, 0, &sig)
            == run_tx(&analysis_root, fee_payer, 0, &sig, &lookups)
    );
    assert!(
        load_trace_tree(&canonical_result_root, 1, &sig)
            == run_tx(&analysis_root, fee_payer, 1, &sig, &lookups)
    );
    assert!(
        load_trace_tree(&canonical_result_root, 2, &sig)
            == run_tx(&analysis_root, fee_payer, 2, &sig, &lookups)
    );
}
