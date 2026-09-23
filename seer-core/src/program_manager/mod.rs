//! Purpose: compose the program manager module layers.

pub mod entrypoints;
pub mod known_programs;
#[allow(clippy::module_inception)]
pub mod program_manager;
pub(crate) mod types;
mod utils;

pub use types::GlobalProgramContext;
