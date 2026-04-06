mod common;

use std::str::FromStr;

use seer_core::{
    init_seer_logger,
    program_manager::program_manager::ProgramManager,
    save::{load_trace_tree, save_trace_tree_to_dir},
    SeerLogger,
};
use solana_pubkey::Pubkey;

use crate::common::{
    get_analysis_directories, seer_test_save_enabled, _run_tx,
};

#[test]
fn test_instruction_context() {
    init_seer_logger(SeerLogger::from_env());

    let (source_project_root, cwd, _deploy_folder_root, analysis_root, canonical_result_root) =
        get_analysis_directories(
            "native/cypherpunk",
            "/Users/vasilygerrans/Desktop/work/code/seer-repo/demo",
        );

    let fee_payer = Pubkey::from_str("J6X9c9BNoWFmE7RNa3qJ7kQ3e7JwJN63hjc2X1VdQGS9")
        .ok()
        .unwrap();

    let (program_manager, _warnings) =
        ProgramManager::init(&cwd, &source_project_root).expect("ProgramManager::init");

    let sig =
        "JuiMHw4p3kgdBsgXK8134Vb4jaL8gfvsXYNvxfi6XgRRckZCugVNuReWUBpg1dTncXoEi8QmAz5fbHP1cvgb45Q"
            .to_string();

    for index in 0..=2 {
        let result = _run_tx(&analysis_root, fee_payer, index, &sig, &program_manager);
        if seer_test_save_enabled() {
            save_trace_tree_to_dir(&canonical_result_root, &sig, index, result);
        } else {
            let expected = load_trace_tree(&canonical_result_root, index, &sig);
            assert!(expected == result, "Trace tree mismatch at index {}", index);
        }
    }
}
