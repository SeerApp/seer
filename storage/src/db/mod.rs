mod historical;
mod ix;
mod program;
mod run;
mod simulation;

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

const TABLES: &[&str] = &[
    include_str!("../tables/simulation.sql"),
    include_str!("../tables/program.sql"),
    include_str!("../tables/historical_transaction.sql"),
    include_str!("../tables/run.sql"),
    include_str!("../tables/run_ix.sql"),
    include_str!("../tables/reg.sql"),
    include_str!("../tables/glassbox.sql"),
];

pub struct Db {
    conn: Connection,
}

#[derive(Clone)]
pub struct RunRow {
    pub id: i64,
    pub transaction_blob_hash: [u8; 32],
    pub state_blob_hash: [u8; 32],
    pub run_at: String,
    pub environment: String,
    pub status: String,
    pub error: Option<String>,
    pub parent_id: Option<i64>,
    pub patches: String,
    pub source: String,
}

impl Db {
    pub(crate) fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = crate::home::ensure(dir.as_ref().to_path_buf())?;
        let conn = Connection::open(dir.join("seer.sqlite"))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        for sql in TABLES {
            conn.execute_batch(sql)?;
        }
        Ok(Self { conn })
    }

    pub fn query(&self, sql: &str) -> Result<String> {
        self.conn.pragma_update(None, "query_only", true)?;
        let result = (|| -> Result<String> {
            let mut stmt = self.conn.prepare(sql)?;
            let names: Vec<String> = stmt
                .column_names()
                .iter()
                .map(|n| (*n).to_string())
                .collect();
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                let mut object = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    object.insert(
                        name.clone(),
                        match row.get(i)? {
                            rusqlite::types::Value::Null => serde_json::Value::Null,
                            rusqlite::types::Value::Integer(n) => n.into(),
                            rusqlite::types::Value::Real(n) => serde_json::json!(n),
                            rusqlite::types::Value::Text(t) => t.into(),
                            rusqlite::types::Value::Blob(b) => hex::encode(b).into(),
                        },
                    );
                }
                out.push(object);
            }
            Ok(serde_json::to_string_pretty(&out)?)
        })();
        let _ = self.conn.pragma_update(None, "query_only", false);
        result
    }
}

fn as32(v: Vec<u8>) -> Result<[u8; 32]> {
    v.try_into().ok().context("expected 32-byte hash")
}
