use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IrrecoverableError {
    #[error(
        "Could not read target file \"{filename}\". Underlying IO error: {detail}"
    )]
    TargetFileRead {
        filename: String,
        detail: String,
    },

    #[error(
        "Could not parse target file \"{filename}\". Parse error: {detail}"
    )]
    TargetFileParse {
        filename: String,
        detail: String,
    },

    #[error(
        "Invalid target \"{target}\": missing both keypair and pubkey files."
    )]
    TargetKeyMissing {
        target: String,
    },

    #[error(
        "Could not open IDL file \"{filename}\". Underlying IO error: {detail}"
    )]
    IdlFileOpen {
        filename: String,
        detail: String,
    },

    #[error(
        "Target \"{target}\" does not have an IDL path configured."
    )]
    TargetFileOpen {
        target: String,
    },

    #[error(
        "The provided IDL file \"{filename}\" could not be parsed into either Anchor v0.30.0+ or Codama IDL.\
        Please make sure you are providing these IDLs to the debugger. Failed with error message: {detail}"
    )]
    InvalidIdl {
        filename: String,
        detail: String,
    },

    #[error(
        "Could not read debug file \"{filename}\". Underlying IO error: {detail}"
    )]
    DwarfFileRead {
        filename: String,
        detail: String,
    },

    #[error(
        "Could not parse debug file \"{filename}\". Parse error: {detail}"
    )]
    DwarfFileParse {
        filename: String,
        detail: String,
    },
}

#[derive(Serialize)]
pub struct IrrecoverableErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Error)]
pub enum Warning {
    #[error("Your debug file is not parsable for program {key} ({program}): {detail}")]
    UnparsableDebugFile {
        key: String,
        program: String,
        detail: String,
    },

    #[error("Your idl file is not parsable for program {key} ({program}): {detail}")]
    UnparsableIdlFile {
        key: String,
        program: String,
        detail: String,
    },
}