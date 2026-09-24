use serde::{Deserialize, Serialize};
use solana_instruction_error::InstructionError;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TreeError {
    #[serde(default)]
    pub step_order: u64,
    pub instruction_error: InstructionError,
    pub parsed: Option<String>,
}

impl PartialEq for TreeError {
    fn eq(&self, other: &Self) -> bool {
        self.instruction_error == other.instruction_error && self.parsed == other.parsed
    }
}

impl TreeError {
    pub fn new(instruction_error: InstructionError, step_order: u64) -> Self {
        Self {
            step_order,
            instruction_error,
            parsed: None,
        }
    }
}
