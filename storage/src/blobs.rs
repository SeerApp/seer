//! Storage of byte data by sha hash name is an
//! invariant enforced at Storage level.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::Mode;

pub struct Blob {
    dir: PathBuf,
    mode: Mode,
}

impl Blob {
    pub(crate) fn open(dir: impl AsRef<Path>, mode: Mode) -> Result<Self> {
        Ok(Self {
            dir: crate::home::ensure(dir.as_ref().to_path_buf())?,
            mode,
        })
    }

    pub fn store(&self, bytes: &[u8]) -> Result<[u8; 32]> {
        let hash = digest(bytes);
        let dest = self.path(&hash);
        match self.mode {
            Mode::Default => {
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
            Mode::Compare => {
                let existing = self.read(&hash)?;
                if existing != bytes {
                    bail!("compare: blob {} mismatch", hex::encode(hash));
                }
                Ok(hash)
            }
        }
    }

    pub fn read(&self, hash: &[u8; 32]) -> Result<Vec<u8>> {
        let bytes = fs::read(self.path(hash))
            .with_context(|| format!("missing blob {}", hex::encode(hash)))?;
        if digest(&bytes) != *hash {
            bail!("blob {} failed hash check", hex::encode(hash));
        }
        Ok(bytes)
    }

    pub fn print_transaction(&self, hash: &[u8; 32]) -> Result<String> {
        let tx: solana_transaction::versioned::VersionedTransaction =
            bincode::deserialize(&self.read(hash)?).context("transaction wire")?;
        serde_json::to_string_pretty(&pretty_tx(&tx)).context("transaction json")
    }

    pub fn print_state(&self, hash: &[u8; 32]) -> Result<String> {
        let value: serde_json::Value =
            serde_json::from_slice(&self.read(hash)?).context("state json")?;
        serde_json::to_string_pretty(&value).context("state json")
    }

    fn path(&self, hash: &[u8; 32]) -> PathBuf {
        self.dir.join(hex::encode(hash))
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn pretty_tx(tx: &solana_transaction::versioned::VersionedTransaction) -> serde_json::Value {
    serde_json::json!({
        "signatures": tx.signatures.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "message": pretty_message(tx),
    })
}

fn pretty_message(tx: &solana_transaction::versioned::VersionedTransaction) -> serde_json::Value {
    let message = &tx.message;
    let header = message.header();
    let mut out = serde_json::json!({
        "header": {
            "num_required_signatures": header.num_required_signatures,
            "num_readonly_signed_accounts": header.num_readonly_signed_accounts,
            "num_readonly_unsigned_accounts": header.num_readonly_unsigned_accounts,
        },
        "account_keys": message
            .static_account_keys()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        "recent_blockhash": message.recent_blockhash().to_string(),
        "instructions": message
            .instructions()
            .iter()
            .map(|ix| {
                serde_json::json!({
                    "program_id_index": ix.program_id_index,
                    "accounts": ix.accounts,
                    "data": hex::encode(&ix.data),
                })
            })
            .collect::<Vec<_>>(),
    });
    if let Some(lookups) = message.address_table_lookups() {
        out["address_table_lookups"] = lookups
            .iter()
            .map(|lookup| {
                serde_json::json!({
                    "account_key": lookup.account_key.to_string(),
                    "writable_indexes": lookup.writable_indexes,
                    "readonly_indexes": lookup.readonly_indexes,
                })
            })
            .collect();
    }
    out
}
