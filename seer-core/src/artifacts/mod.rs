//! On-disk Seer analysis artifacts: path layout, staged I/O, and typed saves.

pub mod layout;
mod store;
pub mod writer;

pub use layout::is_staging_tmp_path;
pub use writer::AtomicFileWriter;
