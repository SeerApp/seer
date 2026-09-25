use bincode::serialized_size;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use storage::Storage;

const ELF_MAGIC: &[u8; 4] = b"\x7FELF";

/// Account datas from this run's state blob. Load once per run.
pub(crate) fn run_state_accounts(storage: &Storage, run_id: i64) -> Vec<(Pubkey, Vec<u8>)> {
    let row = storage.db.get_run(run_id).expect("get run for ELF unwrap");
    let json: serde_json::Value = serde_json::from_slice(
        &storage
            .blob
            .read(&row.state_blob_hash)
            .expect("read run state"),
    )
    .expect("run state JSON");
    let obj = json.as_object().expect("run state object");
    let mut out = Vec::with_capacity(obj.len());
    for (pk, account) in obj {
        let pubkey: Pubkey = pk.parse().expect("state pubkey");
        let data_hex = account
            .get("data")
            .and_then(|v| v.as_str())
            .expect("state data hash");
        let hash: [u8; 32] = hex::decode(data_hex)
            .expect("state data hash hex")
            .try_into()
            .expect("state data hash 32 bytes");
        out.push((pubkey, storage.blob.read(&hash).expect("read account data")));
    }
    out
}

/// Unwrapped ELF bytes for `program_id`, or empty if they cannot be recovered.
pub fn program_elf_bytes(accounts: &[(Pubkey, Vec<u8>)], program_id: &Pubkey) -> Vec<u8> {
    let Some((_, data)) = accounts.iter().find(|(k, _)| k == program_id) else {
        return Vec::new();
    };
    if data.starts_with(ELF_MAGIC) {
        return data.clone();
    }
    let Ok(UpgradeableLoaderState::Program {
        programdata_address,
    }) = bincode::deserialize(data)
    else {
        return data.clone();
    };
    let programdata_address = Pubkey::new_from_array(programdata_address.to_bytes());
    let Some((_, bytes)) = accounts.iter().find(|(k, _)| k == &programdata_address) else {
        return Vec::new();
    };
    let offset = match bincode::deserialize(bytes.as_slice()) {
        Ok(UpgradeableLoaderState::ProgramData {
            upgrade_authority_address,
            ..
        }) => {
            if upgrade_authority_address.is_some() {
                UpgradeableLoaderState::size_of_programdata_metadata()
            } else {
                UpgradeableLoaderState::size_of_programdata_metadata()
                    .saturating_sub(serialized_size(&Pubkey::default()).unwrap_or(0) as usize)
            }
        }
        _ => return Vec::new(),
    };
    bytes.get(offset..).unwrap_or(&[]).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (Storage, std::path::PathBuf) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "seer-elf-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        (Storage::open_at(&p).unwrap(), p)
    }

    fn pk(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn programdata_account(elf: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; UpgradeableLoaderState::size_of_programdata_metadata()];
        let header = bincode::serialize(&UpgradeableLoaderState::ProgramData {
            slot: 1,
            upgrade_authority_address: Some(pk(3)),
        })
        .unwrap();
        bytes[..header.len()].copy_from_slice(&header);
        bytes.extend_from_slice(elf);
        bytes
    }

    #[test]
    fn upgradeable_elf_is_empty_without_programdata() {
        let program = pk(1);
        let programdata = pk(2);
        let header = bincode::serialize(&UpgradeableLoaderState::Program {
            programdata_address: programdata,
        })
        .unwrap();
        assert!(program_elf_bytes(&[(program, header)], &program).is_empty());
    }

    #[test]
    fn run_state_accounts_feed_the_unwrap() {
        let (storage, root) = tmp();
        let program = pk(1);
        let programdata = pk(2);
        let elf = b"\x7FELFpayload";
        let header = bincode::serialize(&UpgradeableLoaderState::Program {
            programdata_address: programdata,
        })
        .unwrap();
        let h_prog = storage.blob.store(&header).unwrap();
        let h_pd = storage.blob.store(&programdata_account(elf)).unwrap();
        let owner = Pubkey::default().to_string();
        let state = serde_json::json!({
            program.to_string(): {
                "lamports": "1",
                "data": hex::encode(h_prog),
                "owner": owner,
                "executable": true
            },
            programdata.to_string(): {
                "lamports": "1",
                "data": hex::encode(h_pd),
                "owner": owner,
                "executable": false
            }
        });
        let state_hash = storage
            .blob
            .store(&serde_json::to_vec(&state).unwrap())
            .unwrap();
        let tx_hash = storage.blob.store(b"tx").unwrap();
        storage.db.insert_simulation(&tx_hash, &state_hash).unwrap();
        let id = storage
            .db
            .insert_run(&tx_hash, &state_hash, "{}", None, "[]", "")
            .unwrap();
        let accounts = run_state_accounts(&storage, id);
        assert_eq!(program_elf_bytes(&accounts, &program), elf);
        std::fs::remove_dir_all(root).ok();
    }
}
