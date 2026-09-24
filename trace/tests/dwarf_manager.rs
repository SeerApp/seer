mod common;

use std::path::PathBuf;

use trace::{dwarf::manager::DwarfManager, path_resolver::PathResolver, sources::Sources};

use crate::common::get_analysis_directories;

#[test]
fn test_relative_paths() {
    let dwarf_compile_dir = "/Users/vasilygerrans/Desktop/work/Seer/code/program-examples/tokens/transfer-tokens/anchor";

    let (source_project_root, cwd, deploy_folder_root, _, _) =
        get_analysis_directories("native/transfer-tokens", dwarf_compile_dir);

    println!("SPR: {:?}", source_project_root);

    let path_resolver = PathResolver::new(PathBuf::from(dwarf_compile_dir), cwd);

    let mut dwarf_manager = DwarfManager::new();
    dwarf_manager
        .set_dwarf_section(&deploy_folder_root.join("transfer_tokens.debug"))
        .expect("load transfer_tokens DWARF");

    let source_files = dwarf_manager.get_all_source_files(&path_resolver);

    println!("source files {:?}", source_files);

    let sources = Sources::new(path_resolver, source_files);
    assert!(
        sources
            .infos
            .keys()
            .any(|p| p.ends_with("programs/transfer-tokens/src/lib.rs")),
        "DWARF prefix strip must map onto fixture programs/transfer-tokens/src/lib.rs, got {:?}",
        sources
            .infos
            .keys()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>(),
    );
}
