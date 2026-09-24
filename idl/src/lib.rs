pub mod anchor;
pub mod codama;
pub mod cursor;
mod decorate;
mod known;

use std::{fs, io::Read, path::Path};

use flate2::read::ZlibDecoder;
use solana_instruction_error::InstructionError;
use solana_pubkey::Pubkey;
use thiserror::Error;
use trace::tree::parsed::{ParsedAccount, ParsedInstruction};

use crate::anchor::AnchorIdlLookup;
use crate::codama::CodamaIdlLookup;

pub use decorate::{decorate, decorate_bytes};
pub use known::{builtin, get_known_programs, SYSTEM_PROGRAM_ADDRESS, SYSTEM_PROGRAM_PUBKEY};

pub trait IdlTreeParser {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction>;

    fn get_account(&self, data: &[u8]) -> Option<ParsedAccount>;

    fn get_error(&self, error: InstructionError) -> String;
}

pub(crate) fn display_error_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub enum IdlLookup {
    Anchor(AnchorIdlLookup),
    Codama(CodamaIdlLookup),
}

impl IdlTreeParser for IdlLookup {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction> {
        match self {
            Self::Anchor(anchor) => anchor.get_instruction(data),
            Self::Codama(codama) => codama.get_instruction(data),
        }
    }

    fn get_account(&self, data: &[u8]) -> Option<ParsedAccount> {
        match self {
            Self::Anchor(anchor) => anchor.get_account(data),
            Self::Codama(codama) => codama.get_account(data),
        }
    }

    fn get_error(&self, error: InstructionError) -> String {
        match self {
            Self::Anchor(anchor) => anchor.get_error(error),
            Self::Codama(codama) => codama.get_error(error),
        }
    }
}

#[derive(Debug, Error)]
pub enum IdlLoadError {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Warning(String),
}

impl IdlLookup {
    pub fn new(idl_json: &str, source_label: &str) -> Result<Self, IdlLoadError> {
        match AnchorIdlLookup::from_json_str(idl_json) {
            Ok(anchor) => Ok(Self::Anchor(anchor)),
            Err(anchor_err) => match CodamaIdlLookup::from_json_str(idl_json) {
                Ok(codama) => Ok(Self::Codama(codama)),
                Err(codama_err) => Err(IdlLoadError::Warning(format!(
                    "The provided IDL source \"{}\" could not be parsed into either Anchor v0.30.0+ or Codama IDL. \
                    Anchor parse error: {anchor_err}; Codama parse error: {codama_err}",
                    source_label
                ))),
            },
        }
    }

    pub fn new_from_path(idl_path: &Path) -> Result<Self, IdlLoadError> {
        let idl_json = fs::read_to_string(idl_path).map_err(|err| {
            IdlLoadError::Io(format!(
                "Could not open IDL file \"{}\": {err}",
                idl_path.to_string_lossy()
            ))
        })?;
        IdlLookup::new(&idl_json, &idl_path.to_string_lossy())
    }
}

pub fn anchor_idl_address(program_id: &Pubkey) -> Result<Pubkey, String> {
    let program_signer = Pubkey::find_program_address(&[], program_id).0;
    Pubkey::create_with_seed(&program_signer, "anchor:idl", program_id)
        .map_err(|e| format!("derive anchor idl address: {e}"))
}

pub fn idl_json_from_anchor_account(account_data: &[u8]) -> Result<String, String> {
    let compressed = extract_anchor_idl_compressed_bytes(account_data)?;
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut idl_json = Vec::new();
    decoder
        .read_to_end(&mut idl_json)
        .map_err(|e| format!("decompress anchor idl: {e}"))?;
    String::from_utf8(idl_json).map_err(|e| format!("utf8 decode anchor idl: {e}"))
}

fn extract_anchor_idl_compressed_bytes(account_data: &[u8]) -> Result<Vec<u8>, String> {
    const DISCRIMINATOR_LEN: usize = 8;
    const AUTHORITY_LEN: usize = 32;
    const DATA_LEN_LEN: usize = 4;
    const HEADER_LEN: usize = DISCRIMINATOR_LEN
        .saturating_add(AUTHORITY_LEN)
        .saturating_add(DATA_LEN_LEN);

    if account_data.len() < HEADER_LEN {
        return Err(format!(
            "anchor idl account too small: {} bytes (need >= {})",
            account_data.len(),
            HEADER_LEN
        ));
    }
    let data_len_offset = DISCRIMINATOR_LEN.saturating_add(AUTHORITY_LEN);
    let data_len = u32::from_le_bytes(
        account_data[data_len_offset..data_len_offset.saturating_add(DATA_LEN_LEN)]
            .try_into()
            .map_err(|_| "anchor idl data length decode failed".to_string())?,
    ) as usize;
    let data_start = HEADER_LEN;
    let data_end = data_start.saturating_add(data_len);
    if account_data.len() < data_end {
        return Err(format!(
            "anchor idl account truncated: {} bytes (need >= {})",
            account_data.len(),
            data_end
        ));
    }
    Ok(account_data[data_start..data_end].to_vec())
}

pub(crate) fn number_format(f: codama_nodes::NumberFormat) -> trace::tree::parsed::NumberFormat {
    use codama_nodes::NumberFormat as C;
    use trace::tree::parsed::NumberFormat as S;
    match f {
        C::U8 => S::U8,
        C::U16 => S::U16,
        C::U32 => S::U32,
        C::U64 => S::U64,
        C::U128 => S::U128,
        C::I8 => S::I8,
        C::I16 => S::I16,
        C::I32 => S::I32,
        C::I64 => S::I64,
        C::I128 => S::I128,
        C::F32 => S::F32,
        C::F64 => S::F64,
        C::ShortU16 => S::ShortU16,
    }
}
