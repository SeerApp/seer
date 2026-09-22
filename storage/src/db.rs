use std::path::Path;

use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::Mode;

const TABLES: &[&str] = &[
    include_str!("tables/simulation.sql"),
    include_str!("tables/program.sql"),
    include_str!("tables/historical_transaction.sql"),
    include_str!("tables/run.sql"),
    include_str!("tables/run_ix.sql"),
    include_str!("tables/reg.sql"),
    include_str!("tables/glassbox.sql"),
];

pub struct Db {
    conn: Connection,
    mode: Mode,
}

impl Db {
    pub(crate) fn open(dir: impl AsRef<Path>, mode: Mode) -> Result<Self> {
        let dir = crate::home::ensure(dir.as_ref().to_path_buf())?;
        let conn = Connection::open(dir.join("seer.sqlite"))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        for sql in TABLES {
            conn.execute_batch(sql)?;
        }
        Ok(Self { conn, mode })
    }

    pub fn insert_simulation(
        &self,
        transaction_blob_hash: &[u8; 32],
        state_blob_hash: &[u8; 32],
    ) -> Result<()> {
        match self.mode {
            Mode::Default => {
                self.conn.execute(
                    "INSERT OR IGNORE INTO simulation (transaction_blob_hash, state_blob_hash, created_at, created_in_dir) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        transaction_blob_hash.as_slice(),
                        state_blob_hash.as_slice(),
                        chrono::Utc::now().to_rfc3339(),
                        std::env::current_dir()?.display().to_string(),
                    ],
                )?;
                Ok(())
            }
            Mode::Compare => {
                let found: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM simulation WHERE transaction_blob_hash = ?1 AND state_blob_hash = ?2",
                    rusqlite::params![transaction_blob_hash.as_slice(), state_blob_hash.as_slice()],
                    |row| row.get(0),
                )?;
                if found != 1 {
                    bail!("compare: missing simulation");
                }
                Ok(())
            }
        }
    }

    pub fn insert_run(
        &self,
        transaction_blob_hash: &[u8; 32],
        state_blob_hash: &[u8; 32],
        overrides: &str,
    ) -> Result<i64> {
        match self.mode {
            Mode::Default => {
                self.conn.execute(
                    "INSERT INTO run (transaction_blob_hash, state_blob_hash, run_at, run_in_dir, overrides, status) VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
                    rusqlite::params![
                        transaction_blob_hash.as_slice(),
                        state_blob_hash.as_slice(),
                        chrono::Utc::now().to_rfc3339(),
                        std::env::current_dir()?.display().to_string(),
                        overrides,
                    ],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
            Mode::Compare => {
                let (id, tx, state, got_overrides) = self.only_run()?;
                if tx.as_slice() != transaction_blob_hash.as_slice()
                    || state.as_slice() != state_blob_hash.as_slice()
                    || got_overrides != overrides
                {
                    bail!("compare: run mismatch");
                }
                Ok(id)
            }
        }
    }

    pub fn finish_run(&self, run_id: i64, error: Option<&str>) -> Result<()> {
        match self.mode {
            Mode::Default => {
                self.conn.execute(
                    "UPDATE run SET status = 'finished', error = ?2 WHERE id = ?1",
                    rusqlite::params![run_id, error],
                )?;
                Ok(())
            }
            Mode::Compare => {
                let (id, _, _, _) = self.only_run()?;
                if id != run_id {
                    bail!("compare: expected run {id}, got {run_id}");
                }
                let got: Option<String> = self.conn.query_row(
                    "SELECT error FROM run WHERE id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )?;
                if got.as_deref() != error {
                    bail!("compare: run error mismatch");
                }
                Ok(())
            }
        }
    }

    pub fn insert_run_ix(&self, run_id: i64, ix: i64) -> Result<()> {
        match self.mode {
            Mode::Default => {
                self.conn.execute(
                    "INSERT INTO run_ix (run_id, ix, status) VALUES (?1, ?2, 'pending')",
                    rusqlite::params![run_id, ix],
                )?;
                Ok(())
            }
            Mode::Compare => {
                let found: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM run_ix WHERE run_id = ?1 AND ix = ?2",
                    rusqlite::params![run_id, ix],
                    |row| row.get(0),
                )?;
                if found != 1 {
                    bail!("compare: missing run_ix {run_id}/{ix}");
                }
                Ok(())
            }
        }
    }

    pub fn finish_run_ix(
        &self,
        run_id: i64,
        ix: i64,
        trace_blob_hash: Option<&[u8; 32]>,
    ) -> Result<()> {
        match self.mode {
            Mode::Default => {
                self.conn.execute(
                    "UPDATE run_ix SET status = 'finished', trace_blob_hash = ?3 WHERE run_id = ?1 AND ix = ?2",
                    rusqlite::params![run_id, ix, trace_blob_hash.map(|h| h.as_slice())],
                )?;
                Ok(())
            }
            Mode::Compare => {
                let got: Option<Vec<u8>> = self.conn.query_row(
                    "SELECT trace_blob_hash FROM run_ix WHERE run_id = ?1 AND ix = ?2",
                    rusqlite::params![run_id, ix],
                    |row| row.get(0),
                )?;
                if got.as_deref() != trace_blob_hash.map(|h| h.as_slice()) {
                    bail!("compare: run_ix {run_id}/{ix} trace mismatch");
                }
                Ok(())
            }
        }
    }

    pub fn insert_program(&self, self_blob_hash: &[u8; 32]) -> Result<()> {
        match self.mode {
            Mode::Default => {
                self.conn.execute(
                    "INSERT OR IGNORE INTO program (self_blob_hash) VALUES (?1)",
                    rusqlite::params![self_blob_hash.as_slice()],
                )?;
                Ok(())
            }
            Mode::Compare => {
                let found: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM program WHERE self_blob_hash = ?1",
                    rusqlite::params![self_blob_hash.as_slice()],
                    |row| row.get(0),
                )?;
                if found != 1 {
                    bail!("compare: missing program {}", hex::encode(self_blob_hash));
                }
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_reg(
        &self,
        run_id: i64,
        ix: i64,
        start_step: i64,
        end_step: i64,
        self_blob_hash: &[u8; 32],
        program_blob_hash: &[u8; 32],
        pubkey: &[u8; 32],
    ) -> Result<()> {
        match self.mode {
            Mode::Default => {
                self.conn.execute(
                    "INSERT INTO reg (run_id, ix, start_step, end_step, self_blob_hash, program_blob_hash, pubkey) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        run_id,
                        ix,
                        start_step,
                        end_step,
                        self_blob_hash.as_slice(),
                        program_blob_hash.as_slice(),
                        pubkey.as_slice(),
                    ],
                )?;
                Ok(())
            }
            Mode::Compare => {
                let found: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM reg WHERE run_id = ?1 AND ix = ?2 AND start_step = ?3 AND end_step = ?4 AND self_blob_hash = ?5 AND program_blob_hash = ?6 AND pubkey = ?7",
                    rusqlite::params![
                        run_id,
                        ix,
                        start_step,
                        end_step,
                        self_blob_hash.as_slice(),
                        program_blob_hash.as_slice(),
                        pubkey.as_slice(),
                    ],
                    |row| row.get(0),
                )?;
                if found != 1 {
                    bail!("compare: missing reg {run_id}/{ix}");
                }
                Ok(())
            }
        }
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

    fn only_run(&self) -> Result<(i64, Vec<u8>, Vec<u8>, String)> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM run", [], |row| row.get(0))?;
        if n != 1 {
            bail!("compare: expected exactly one run, found {n}");
        }
        Ok(self.conn.query_row(
            "SELECT id, transaction_blob_hash, state_blob_hash, overrides FROM run",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?)
    }
}
