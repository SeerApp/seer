//! Storage of byte data by sha hash name is an
//! invariant enforced at Storage level.

use std::fs;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

pub fn store_blob(bytes: &[u8]) -> Result<[u8; 32]> {
    let hash = digest(bytes);
    let dest = super::home::blob()?.join(hex::encode(hash));
    if dest.exists() {
        return Ok(hash);
    }
    let staging = dest.with_extension("tmp");
    if let Err(e) = fs::write(&staging, bytes) {
        let _ = fs::remove_file(&staging);
        return Err(e.into());
    }
    if let Err(e) = fs::rename(&staging, &dest) {
        let _ = fs::remove_file(&staging);
        if dest.exists() {
            return Ok(hash);
        }
        return Err(e.into());
    }
    Ok(hash)
}

pub fn read_blob(hash: &[u8; 32]) -> Result<Vec<u8>> {
    let bytes = fs::read(super::home::blob()?.join(hex::encode(hash)))
        .with_context(|| format!("missing blob {}", hex::encode(hash)))?;
    if digest(&bytes) != *hash {
        bail!("blob {} failed hash check", hex::encode(hash));
    }
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
