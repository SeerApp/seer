use bincode::serialized_size;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;

const ELF_MAGIC: &[u8; 4] = b"\x7FELF";

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
