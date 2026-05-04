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
                notes: TxNotes { warnings: vec![] },
            },
        }
    }

    pub fn set_error(&mut self, err: InstructionError, idl: Option<&dyn IdlTreeParser>) {
        // Keep the first observed program error as canonical transaction metadata.
        // Program exits unwind from inner to outer, so the earliest error we see is innermost.
        if self.data.output.error.is_none() {
            let message = idl
                .map(|parser| parser.get_error(err.clone()))
                .unwrap_or_else(|| err.to_string());
            self.data.output.error = Some(TxError { message });
        }
        self.data.success = false;
    }
}

#[cfg(test)]
mod tests {
    use super::TxMetadata;
    use solana_instruction_error::InstructionError;

    #[test]
    fn keeps_first_error_as_canonical_output() {
        let mut meta = TxMetadata::default();

        meta.set_error(InstructionError::Custom(1), None);
        meta.set_error(InstructionError::Custom(2), None);

        assert!(!meta.data.success);
        assert_eq!(
            meta.data.output.error.as_ref().unwrap().message,
            "custom program error: 0x1"
        );
    }

    #[test]
    fn subsequent_errors_do_not_replace_canonical_error() {
        let mut meta = TxMetadata::default();

        meta.set_error(InstructionError::Custom(7), None);
        meta.set_error(InstructionError::Custom(8), None);

        assert!(!meta.data.success);
        assert_eq!(
            meta.data.output.error.as_ref().unwrap().message,
            "custom program error: 0x7"
        );
    }
}
