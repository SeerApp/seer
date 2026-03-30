use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use solana_pubkey::Pubkey;
use solana_signer::Signer;

#[derive(Debug, Clone)]
pub struct Target {
    pub base: String,
    pub executable: Option<PathBuf>,
    pub dwarf: Option<PathBuf>,
    pub idl: Option<PathBuf>,
}

pub fn get_targets(target_dir: &PathBuf) -> HashMap<Pubkey, Target> {
    let mut bases: HashSet<String> = HashSet::new();
    let mut targets: HashMap<Pubkey, Target> = HashMap::new();

    let entries = match fs::read_dir(target_dir) {
        Ok(e) => e,
        Err(_) => return targets,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        if let Some(base) = file_name.strip_suffix("-keypair.json") {
            bases.insert(base.to_string());
        } else if let Some(base) = file_name.strip_suffix("-pubkey.json") {
            bases.insert(base.to_string());
        } else if let Some(base) = file_name.strip_suffix(".so") {
            bases.insert(base.to_string());
        } else if let Some(base) = file_name.strip_suffix(".debug") {
            bases.insert(base.to_string());
        }
    }

    for base in bases {
        let keypair_path = target_dir.join(format!("{base}-keypair.json"));
        let pubkey_path = target_dir.join(format!("{base}-pubkey.json"));
        let executable_path = target_dir.join(format!("{base}.so"));
        let dwarf_path = target_dir.join(format!("{base}.debug"));
        let idl_path = target_dir.join("idl").join(format!("{base}.json"));

        let pubkey = if keypair_path.exists() {
            let keypair = solana_keypair::read_keypair_file(&keypair_path).unwrap_or_else(|e| {
                panic!("Failed to read keypair `{}`: {e}", keypair_path.display())
            });
            keypair.pubkey()
        } else if pubkey_path.exists() {
            let pubkey_str: String =
                serde_json::from_str(&fs::read_to_string(&pubkey_path).unwrap_or_else(|e| {
                    panic!(
                        "Failed to read pubkey file `{}`: {e}",
                        pubkey_path.display()
                    )
                }))
                .unwrap_or_else(|e| {
                    panic!(
                        "Failed to parse pubkey file `{}`: {e}",
                        pubkey_path.display()
                    )
                });
            pubkey_str
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse pubkey string `{}`: {e}", pubkey_str))
        } else {
            panic!(
                "Program `{base}` is missing both keypair and pubkey file (expected `{}` or `{}`)",
                keypair_path.display(),
                pubkey_path.display()
            );
        };

        targets.insert(
            pubkey,
            Target {
                base,
                executable: executable_path.exists().then_some(executable_path),
                dwarf: dwarf_path.exists().then_some(dwarf_path),
                idl: idl_path.exists().then_some(idl_path),
            },
        );
    }

    targets
}