mod common;

use std::str::FromStr;

use std::sync::{Arc, Mutex};

use seer_core::{
    artifacts::{layout::load_trace_tree, AtomicFileWriter},
    init_seer_logger,
    program_manager::GlobalProgramContext,
    SeerLogger,
};
use solana_pubkey::Pubkey;

use crate::common::{_run_tx, get_analysis_directories, seer_test_save_enabled};

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

    let file_writer = Arc::new(Mutex::new(AtomicFileWriter::new()));
    let global_program_context =
        GlobalProgramContext::init(&cwd, &source_project_root)
            .expect("GlobalProgramContext::init");

    let result = _run_tx(&analysis_root, fee_payer, 0, &sig, &global_program_context);
    if seer_test_save_enabled() {
        file_writer
            .lock()
            .expect("file writer lock")
            .save_trace_tree_to_dir(&canonical_result_root, &sig, 0, result);
    } else {
        let expected = load_trace_tree(&canonical_result_root, 0, &sig);
        assert!(expected == result, "Puppet trace tree mismatch at index");
    }
}
