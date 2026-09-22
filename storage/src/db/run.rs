use anyhow::{Context, Result};

use super::{as32, Db, RunRow};

const RUN_SELECT: &str = "SELECT id, transaction_blob_hash, state_blob_hash, run_at, environment, status, error, parent_id, patches, source FROM run";

impl Db {
    pub fn insert_run(
        &self,
        transaction_blob_hash: &[u8; 32],
        state_blob_hash: &[u8; 32],
        environment: &str,
        parent_id: Option<i64>,
        patches: &str,
        source: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO run (transaction_blob_hash, state_blob_hash, run_at, run_in_dir, environment, status, parent_id, patches, source) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?8)",
            rusqlite::params![
                transaction_blob_hash.as_slice(),
                state_blob_hash.as_slice(),
                chrono::Utc::now().to_rfc3339(),
                std::env::current_dir()?.display().to_string(),
                environment,
                parent_id,
                patches,
                source,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_run(&self, run_id: i64, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE run SET status = 'finished', error = ?2 WHERE id = ?1",
            rusqlite::params![run_id, error],
        )?;
        Ok(())
    }

    pub fn get_run(&self, id: i64) -> Result<RunRow> {
        self.conn
            .query_row(&format!("{RUN_SELECT} WHERE id = ?1"), [id], run_from_row)
            .with_context(|| format!("no run {id}"))
            .and_then(convert_run)
    }

    pub fn list_runs(&self) -> Result<Vec<RunRow>> {
        let mut stmt = self.conn.prepare(&format!("{RUN_SELECT} ORDER BY id"))?;
        let rows = stmt.query_map([], run_from_row)?;
        rows.map(|row| row.map_err(Into::into).and_then(convert_run))
            .collect()
    }
}

type RawRun = (
    i64,
    Vec<u8>,
    Vec<u8>,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    String,
    String,
);

fn run_from_row(row: &rusqlite::Row) -> rusqlite::Result<RawRun> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

fn convert_run(raw: RawRun) -> Result<RunRow> {
    Ok(RunRow {
        id: raw.0,
        transaction_blob_hash: as32(raw.1)?,
        state_blob_hash: as32(raw.2)?,
        run_at: raw.3,
        environment: raw.4,
        status: raw.5,
        error: raw.6,
        parent_id: raw.7,
        patches: raw.8,
        source: raw.9,
    })
}
