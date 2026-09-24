use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IrrecoverableError {
    #[error("Could not read target file \"{filename}\". Underlying IO error: {detail}")]
    TargetFileRead { filename: String, detail: String },

    #[error("Could not parse target file \"{filename}\". Parse error: {detail}")]
    TargetFileParse { filename: String, detail: String },

    #[error("Invalid target \"{target}\": missing both keypair and pubkey files.")]
    TargetKeyMissing { target: String },

    #[error("Could not read debug file \"{filename}\". Underlying IO error: {detail}")]
    DwarfFileRead { filename: String, detail: String },

    #[error("Could not parse debug file \"{filename}\". Parse error: {detail}")]
    DwarfFileParse { filename: String, detail: String },
}

#[derive(Serialize)]
pub struct IrrecoverableErrorBody {
    pub code: &'static str,
    pub message: String,
}
