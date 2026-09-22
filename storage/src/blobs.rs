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

    pub fn print_trace(&self, hash: &[u8; 32]) -> Result<String> {
        let value: serde_json::Value =
            serde_json::from_slice(&self.read(hash)?).context("trace json")?;
        let mut out = String::new();
        write_trace(&mut out, &value, 0);
        Ok(out)
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

fn write_trace(out: &mut String, value: &serde_json::Value, depth: usize) {
    match value {
        serde_json::Value::Object(obj) if obj.contains_key("receiver") => {
            write_root(out, obj, depth)
        }
        serde_json::Value::Object(obj) => write_tagged(out, obj, depth),
        _ => line(out, depth, &value.to_string()),
    }
}

fn write_root(out: &mut String, obj: &serde_json::Map<String, serde_json::Value>, depth: usize) {
    let receiver = str_field(obj, "receiver");
    let name = parsed_name(obj.get("parsed"));
    if name.is_empty() {
        line(out, depth, receiver);
    } else {
        line(out, depth, &format!("{receiver}  {name}"));
    }
    write_parsed_args(out, obj.get("parsed"), depth.saturating_add(1));
    if let Some(serde_json::Value::Array(children)) = obj.get("children") {
        for child in children {
            write_trace(out, child, depth.saturating_add(1));
        }
    }
}

fn write_tagged(out: &mut String, obj: &serde_json::Map<String, serde_json::Value>, depth: usize) {
    if let Some(body) = obj.get("Log") {
        line(out, depth, &format!("log: {}", str_value(body, "message")));
    } else if let Some(body) = obj.get("Error") {
        line(
            out,
            depth,
            &format!("error: {}", str_value(body, "message")),
        );
    } else if let Some(body) = obj.get("Account") {
        let key = str_value(body, "key");
        let before = body
            .get("before")
            .and_then(|a| a.get("lamports"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let after = body
            .get("after")
            .and_then(|a| a.get("lamports"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        line(out, depth, &format!("account {key}  {before} -> {after}"));
    } else if let Some(body) = obj.get("Invoke") {
        write_trace(out, body, depth);
    } else if let Some(body) = obj.get("Entrypoint").or_else(|| obj.get("FnCall")) {
        let tag = if obj.contains_key("Entrypoint") {
            "entrypoint"
        } else {
            "fn"
        };
        line(
            out,
            depth,
            &format!("{tag} {}", str_value(body, "signature")),
        );
        if let Some(serde_json::Value::Array(children)) = body.get("children") {
            for child in children {
                write_trace(out, child, depth.saturating_add(1));
            }
        }
    } else if obj.contains_key("receiver") {
        write_root(out, obj, depth);
    } else {
        line(
            out,
            depth,
            &serde_json::Value::Object(obj.clone()).to_string(),
        );
    }
}

fn write_parsed_args(out: &mut String, parsed: Option<&serde_json::Value>, depth: usize) {
    let Some(serde_json::Value::Array(args)) = parsed.and_then(|p| p.get("args")) else {
        return;
    };
    for arg in args {
        let name = str_value(arg, "name");
        line(
            out,
            depth,
            &format!("{name}: {}", pretty_arg(arg.get("value"))),
        );
    }
}

fn parsed_name(parsed: Option<&serde_json::Value>) -> &str {
    parsed
        .and_then(|p| p.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

fn pretty_arg(value: Option<&serde_json::Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if let Some(n) = value
        .pointer("/value/value")
        .and_then(serde_json::Value::as_str)
    {
        return n.to_owned();
    }
    if let Some(s) = value.as_str() {
        return s.to_owned();
    }
    value.to_string()
}

fn str_field<'a>(obj: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> &'a str {
    obj.get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?")
}

fn str_value<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?")
}

fn line(out: &mut String, depth: usize, text: &str) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(text);
    out.push('\n');
}
