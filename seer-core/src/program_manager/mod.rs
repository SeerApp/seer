//! Purpose: compose the program manager module layers.

pub mod entrypoints;
#[allow(clippy::module_inception)]
pub mod program_manager;
pub(crate) mod types;

pub use types::GlobalProgramContext;
