mod disasm;
mod lift;
mod symbols;

use serde_json::Value;

pub use disasm::{disasm_chunks, store_disasm};
pub use lift::{lifted_chunks, store_lifted};

pub struct JsonChunk {
    pub start_pc: u64,
    pub end_pc: u64,
    pub json: Value,
}
