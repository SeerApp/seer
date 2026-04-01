use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};
use solana_pubkey::Pubkey;
use solana_signer::Signer;

use crate::errors::IrrecoverableError;

#[derive(Debug, Clone)]
pub struct Target {
    pub base: String,
    pub executable: Option<PathBuf>,
    pub dwarf: Option<PathBuf>,
    pub idl: Option<PathBuf>,
}

pub fn get_targets(target_dir: &PathBuf) -> Result<HashMap<Pubkey, Target>, IrrecoverableError> {
    let mut bases: HashSet<String> = HashSet::new();
    let mut targets: HashMap<Pubkey, Target> = HashMap::new();

    let entries = match fs::read_dir(target_dir) {
        Ok(e) => e,
        Err(_) => return Ok(targets),
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
            let keypair = solana_keypair::read_keypair_file(&keypair_path).map_err(|e| {
                IrrecoverableError::TargetFileRead {
                    filename: keypair_path.display().to_string(),
                    detail: e.to_string(),
                }
            })?;
            keypair.pubkey()
        } else if pubkey_path.exists() {
            let pubkey_contents =
                fs::read_to_string(&pubkey_path).map_err(|e| IrrecoverableError::TargetFileRead {
                    filename: pubkey_path.display().to_string(),
                    detail: e.to_string(),
                })?;
            let pubkey_str: String =
                serde_json::from_str(&pubkey_contents).map_err(|e| {
                    IrrecoverableError::TargetFileParse {
                        filename: pubkey_path.display().to_string(),
                        detail: e.to_string(),
                    }
                })?;
            pubkey_str.parse().map_err(|e: solana_pubkey::ParsePubkeyError| {
                IrrecoverableError::TargetFileParse {
                    filename: pubkey_path.display().to_string(),
                    detail: format!("invalid pubkey string `{pubkey_str}`: {e}"),
                }
            })?
        } else {
            return Err(IrrecoverableError::TargetKeyMissing {
                target: base.to_string(),
            });
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

    Ok(targets)
}