use std::path::PathBuf;

use anyhow::{anyhow, Result};

pub(crate) fn ensure(path: PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn default_root() -> Result<PathBuf> {
    dirs::data_local_dir()
        .ok_or_else(|| anyhow!("no user data directory"))
        .map(|p| p.join("seer"))
}
