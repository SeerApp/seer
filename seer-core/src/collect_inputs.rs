use std::{collections::{HashMap, HashSet}, env, fs, path::PathBuf};

use solana_keypair::{read_keypair_file};
use solana_signer::Signer;
use solana_pubkey::Pubkey;

pub fn collect_inputs(source_project_root: Option<PathBuf>, deploy_folder_root: Option<PathBuf>) -> (
    PathBuf,
    HashMap<Pubkey, PathBuf>,
) {
    let final_source_project_root = source_project_root.unwrap_or_else(|| get_cwd());

    (
        final_source_project_root,
        get_dwarf_sources(
            deploy_folder_root
                .unwrap_or_else(|| get_cwd().join("target").join("deploy")
            )
        ),
    )
}

fn get_cwd() -> PathBuf {
    env::current_dir().expect("env::curnet_dir failed!")
}

fn get_dwarf_sources(deploy_folder_root: PathBuf) -> HashMap<Pubkey, PathBuf> {
    let mut bases: HashSet<String> = HashSet::new();

    let entries = match fs::read_dir(&deploy_folder_root) {
        Ok(e) => e,
        Err(_) => return HashMap::new(), // empty acceptable
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        if let Some(base) = file_name.strip_suffix("-keypair.json") {
            bases.insert(base.to_string());
        } else if let Some(base) = file_name.strip_suffix(".so") {
            bases.insert(base.to_string());
        } else if let Some(base) = file_name.strip_suffix(".debug") {
            bases.insert(base.to_string());
        }
    }

    let mut out = HashMap::new();

    for base in bases {
        let keypair_path = deploy_folder_root.join(format!("{base}-keypair.json"));
        let dwarf_path = deploy_folder_root.join(format!("{base}.debug"));

        if !keypair_path.exists() {
            panic!(
                "Program `{base}` is missing keypair file: {}",
                keypair_path.display()
            );
        }

        if !dwarf_path.exists() {
            panic!(
                "Program `{base}` is missing DWARF file: {}",
                dwarf_path.display()
            );
        }

        let keypair = read_keypair_file(&keypair_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read keypair `{}`: {e}",
                keypair_path.display()
            )
        });

        let pubkey = keypair.pubkey();
        out.insert(pubkey, dwarf_path);
    }

    out
}