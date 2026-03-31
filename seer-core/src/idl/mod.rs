pub mod lookup;
pub mod types;

use std::{fs::File, io::BufReader, path::PathBuf};
use codama_nodes::RootNode;

use crate::{idl::lookup::IdlLookup, target_reader::Target};

/// Reads IDL from target, converts non-Codama IDL to Codama,
/// returns IdlLookup if successful.
pub fn get_idl_from_target(target: &Target) -> Option<IdlLookup> {
    let idl = target.idl.as_ref()?;
    get_idl_from_disk(idl).ok()
}

/// Reads IDL from disk for known programs.
pub fn get_idl_from_disk(idl_path: &PathBuf) -> anyhow::Result<IdlLookup> {
    let f = File::open(idl_path)?;
    let reader = BufReader::new(f);
    let root_node: RootNode = serde_json::from_reader(reader)?;
    Ok(IdlLookup::new(root_node))
}