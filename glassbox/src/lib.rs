#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::default_trait_access)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::used_underscore_binding)]

pub mod analysis;
pub mod ancestry;
pub mod astwalk;
pub mod coverage;
pub mod grammar;
pub mod input_abi;
pub mod parse;
pub mod path_condition;
mod reg;
pub mod regions;
pub mod rewrite;
pub mod sat;
pub mod state;
pub mod step;
pub mod store;
pub mod sym;
pub mod syscalls;
pub mod view;
pub mod vm;

pub use analysis::{
    Analysis, DisplayOptions, HideDataLenChildren, HideMode, HideNumAccountChildren, Report,
};
pub use step::Step;
pub use store::{register_timeline, store, RegisterStep};
pub use view::{view, ViewOpts};
pub use vm::Vm;
