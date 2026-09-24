use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TreeLog {
    #[serde(default)]
    pub step_order: u64,
    pub message: String,
}

impl PartialEq for TreeLog {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
    }
}

impl TreeLog {
    pub fn new(message: String, step_order: u64) -> Self {
        Self {
            step_order,
            message,
        }
    }
}
