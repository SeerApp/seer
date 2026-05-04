use serde::{Deserialize, Serialize};
use solana_instruction_error::InstructionError;

use crate::idl::IdlTreeParser;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TreeError {
    #[serde(default)]
    pub step_order: u64,
    /// Human-readable text from [`InstructionError`]; JSON is always a string on write. Reads accept
    /// either a string or legacy tagged `InstructionError` JSON (e.g. `{"Custom": 0}`).
    #[serde(deserialize_with = "deserialize_tree_error_message")]
    pub message: String,
    /// Not persisted on disk (covered by `message`). After `Deserialize`, this is a placeholder.
    #[serde(
        skip_serializing,
        skip_deserializing,
        default = "TreeError::deser_placeholder_error"
    )]
    pub instruction_error: InstructionError,
}

impl PartialEq for TreeError {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
    }
}

impl TreeError {
    fn deser_placeholder_error() -> InstructionError {
        InstructionError::Custom(0)
    }

    pub fn new(message: InstructionError, step_order: u64) -> Self {
        Self {
            step_order,
            instruction_error: message.clone(),
            message: message.to_string(),
        }
    }

    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        self.message = parser.get_error(self.instruction_error.clone());
    }
}

fn deserialize_tree_error_message<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Text(String),
        Error(InstructionError),
    }
    Ok(match Repr::deserialize(deserializer)? {
        Repr::Text(s) => s,
        Repr::Error(e) => e.to_string(),
    })
}
