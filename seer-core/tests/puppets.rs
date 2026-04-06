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
            "anchor/puppets",
            "/Users/vasilygerrans/Desktop/work/Seer/code/puppets/examples/tutorial/basic-3",
        );

    let fee_payer = Pubkey::from_str("EmPkKuzAdBZRC4jH2N12D4HJS3TZTQ41c9NB5Gzzdqrq")
        .ok()
        .unwrap();
    let sig: String =
        "5jVZw9AHxDMW346wjG1XeBu4gcvazLcQWJEVoiVNi1efhJXUF8Rmb7H8BE3PwXSBWaMMehdefMTVq8pkcnKNX6ZQ"
            .to_string();

    let (program_manager, _warnings) =
        ProgramManager::init(&cwd, &source_project_root).expect("ProgramManager::init");

    let result = _run_tx(&analysis_root, fee_payer, 0, &sig, &program_manager);
    if seer_test_save_enabled() {
        save_trace_tree_to_dir(&canonical_result_root, &sig, 0, result);
    } else {
        let expected = load_trace_tree(&canonical_result_root, 0, &sig);
        assert!(expected == result, "Puppet trace tree mismatch at index");
    }
}
