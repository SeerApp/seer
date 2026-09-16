#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::{anyhow, Result};

fn ensure(path: PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn root_path() -> Result<PathBuf> {
    dirs::data_local_dir()
        .ok_or_else(|| anyhow!("no user data directory"))
        .map(|p| p.join("seer"))
}

pub fn root() -> Result<PathBuf> {
    ensure(root_path()?)
}

pub fn bin() -> Result<PathBuf> {
    ensure(root_path()?.join("bin"))
}

pub fn blob() -> Result<PathBuf> {
    ensure(root_path()?.join("blob"))
}

pub fn db() -> Result<PathBuf> {
    ensure(root_path()?.join("db"))
}
