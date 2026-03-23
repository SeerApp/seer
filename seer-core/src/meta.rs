use serde::Serialize;
use solana_instruction_error::InstructionError;

#[derive(Serialize, Clone)]
struct TxError {
    /// Human-readable text from [`InstructionError`]; JSON always uses a string (not a tagged enum).
    message: String,
}

#[derive(Serialize, Clone)]
struct TxOutput {
    error: Option<TxError>,
}

#[derive(Serialize, Clone)]
struct TxData {
    success: bool,
    output: TxOutput,
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
            },
        }
    }

    pub fn set_output(&mut self, error_message: Option<InstructionError>) {
        if let Some(msg) = error_message {
            self.data.success = false;
            self.data.output.error = Some(TxError {
                message: msg.to_string(),
            });
        } else {
            self.data.success = true;
            self.data.output.error = None;
        }
    }
}
