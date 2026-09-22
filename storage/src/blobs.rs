use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

pub struct Blob {
    dir: PathBuf,
}

impl Blob {
    pub(crate) fn open(dir: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            dir: crate::home::ensure(dir.as_ref().to_path_buf())?,
        })
    }

    pub fn store(&self, bytes: &[u8]) -> Result<[u8; 32]> {
        let hash = digest(bytes);
        let dest = self.path(&hash);
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

    pub fn read(&self, hash: &[u8; 32]) -> Result<Vec<u8>> {
        let bytes = fs::read(self.path(hash))
            .with_context(|| format!("missing blob {}", hex::encode(hash)))?;
        if digest(&bytes) != *hash {
            bail!("blob {} failed hash check", hex::encode(hash));
        }
        Ok(bytes)
    }

    fn path(&self, hash: &[u8; 32]) -> PathBuf {
        self.dir.join(hex::encode(hash))
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
