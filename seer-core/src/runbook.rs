use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

#[derive(Debug, Clone)]
struct ProgramArtifact {
    program_id: Pubkey,
    so_rel_path: String,
}

pub fn generate_runbooks(authority: Pubkey, runtime_dir: &PathBuf) -> Option<(String, String)> {
    let deploy_dir = runtime_dir.join("target").join("deploy");

    if !deploy_dir.is_dir() {
        return None;
    }

    let programs = discover_program_artifacts(runtime_dir, &deploy_dir);
    let txtx_yml = render_txtx_yml();
    let main_tx = render_main_tx(authority, &programs);

    Some((txtx_yml, main_tx))
}

fn discover_program_artifacts(runtime_dir: &Path, deploy_dir: &Path) -> Vec<ProgramArtifact> {
    let mut so_files: BTreeMap<String, PathBuf> = BTreeMap::new();

    for entry in fs::read_dir(deploy_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", deploy_dir.display()))
    {
        let entry = entry.unwrap_or_else(|e| panic!("failed to read dir entry: {e}"));
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };

        if ext != "so" {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            panic!("invalid .so filename: {}", path.display());
        };

        so_files.insert(stem.to_string(), path);
    }

    let mut out = Vec::with_capacity(so_files.len());

    for (name, so_path) in so_files {
        let pubkey = load_program_pubkey_for_name(deploy_dir, &name);

        let so_rel_path = pathdiff::diff_paths(&so_path, runtime_dir)
            .unwrap_or_else(|| {
                panic!(
                    "failed to make relative path from {} to {}",
                    so_path.display(),
                    runtime_dir.display()
                )
            })
            .to_string_lossy()
            .replace('\\', "/");

        out.push(ProgramArtifact {
            program_id: pubkey,
            so_rel_path: format!("./{so_rel_path}"),
        });
    }

    out
}

fn load_program_pubkey_for_name(deploy_dir: &Path, program_name: &str) -> Pubkey {
    let pubkey_json_path = deploy_dir.join(format!("{program_name}-pubkey.json"));
    if pubkey_json_path.is_file() {
        return read_pubkey_json_string(&pubkey_json_path);
    }

    let keypair_json_path = deploy_dir.join(format!("{program_name}-keypair.json"));
    if keypair_json_path.is_file() {
        return read_pubkey_from_keypair_json(&keypair_json_path);
    }

    panic!(
        "missing pubkey file for program '{}': expected either {} or {}",
        program_name,
        pubkey_json_path.display(),
        keypair_json_path.display()
    );
}

fn read_pubkey_json_string(path: &Path) -> Pubkey {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let s: String = serde_json::from_str(&raw).unwrap_or_else(|e| {
        panic!(
            "failed to parse pubkey JSON string in {}: {e}",
            path.display()
        )
    });

    s.parse::<Pubkey>()
        .unwrap_or_else(|e| panic!("invalid pubkey in {}: {e}", path.display()))
}

fn read_pubkey_from_keypair_json(path: &Path) -> Pubkey {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let bytes: Vec<u8> = serde_json::from_str(&raw).unwrap_or_else(|e| {
        panic!(
            "failed to parse keypair JSON array in {}: {e}",
            path.display()
        )
    });

    let keypair = Keypair::try_from(bytes.as_slice())
        .unwrap_or_else(|e| panic!("invalid keypair bytes in {}: {e}", path.display()));

    keypair.pubkey()
}

fn render_txtx_yml() -> String {
    r#"---
name: seer
id: seer
runbooks:
  - name: deployment
    description: Deploy programs
    location: runbooks/deployment
environments:
  localnet:
      network_id: localnet
      rpc_api_url: http://127.0.0.1:8899
"#
    .to_string()
}

fn render_main_tx(authority: Pubkey, programs: &[ProgramArtifact]) -> String {
    let mut out = String::new();

    out.push_str(
        "addon \"svm\" {\n\
         \x20\x20\x20\x20rpc_api_url = input.rpc_api_url\n\
         \x20\x20\x20\x20network_id = input.network_id\n\
         }\n\
         \n\
         action \"setup\" \"svm::setup_surfnet\" {\n",
    );

    for program in programs {
        out.push_str("    deploy_program {\n");
        out.push_str(&format!(
            "        program_id = \"{}\"\n",
            program.program_id
        ));
        out.push_str(&format!(
            "        binary_path = \"{}\"\n",
            program.so_rel_path
        ));
        out.push_str(&format!("        authority = \"{}\"\n", authority));
        out.push_str("        instant_surfnet_deployment = true\n");
        out.push_str("    }\n");
    }

    out.push_str("}\n");
    out
}
