use serde::Serialize;
use solana_instruction_error::InstructionError;

use crate::idl::IdlTreeParser;

#[derive(Serialize, Clone)]
struct TxError {
    /// IDL-resolved description when an IDL is available; otherwise [`InstructionError`] display text.
    message: String,
}

#[derive(Serialize, Clone)]
struct TxOutput {
    error: Option<TxError>,
}

#[derive(Serialize, Clone)]
struct TxNotes {
    warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
struct TxData {
    success: bool,
    output: TxOutput,
    notes: TxNotes,
}

#[derive(Serialize, Clone)]
pub struct TxMetadata {
    version: u32,
    data: TxData,
}

impl TxMetadata {
    pub fn default() -> Self {
        Self {
            version: 1,
            data: TxData {
                success: true,
                output: TxOutput { error: None },
                notes: TxNotes { warnings: vec![] }
            },
        }
    }

    pub fn set_output(
        &mut self,
        error: Option<InstructionError>,
        idl: Option<&dyn IdlTreeParser>,
    ) {
        if let Some(err) = error {
            self.data.success = false;
            let message = idl
                .map(|parser| parser.get_error(err.clone()))
                .unwrap_or_else(|| err.to_string());
            self.data.output.error = Some(TxError { message });
        } else {
            self.data.success = true;
            self.data.output.error = None;
        }
    }
}
