use anyhow::Result;

use super::{as32, Db};

impl Db {
    pub fn list_run_ix(&self, run_id: i64) -> Result<Vec<(i64, Option<[u8; 32]>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT ix, trace_blob_hash FROM run_ix WHERE run_id = ?1 ORDER BY ix")?;
        let rows = stmt.query_map([run_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
        })?;
        rows.map(|row| {
            let (ix, hash) = row?;
            Ok((ix, hash.map(as32).transpose()?))
        })
        .collect()
    }

    pub fn insert_run_ix(&self, run_id: i64, ix: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO run_ix (run_id, ix, status) VALUES (?1, ?2, 'pending')",
            rusqlite::params![run_id, ix],
        )?;
        Ok(())
    }

    pub fn finish_run_ix(
        &self,
        run_id: i64,
        ix: i64,
        trace_blob_hash: Option<&[u8; 32]>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE run_ix SET status = 'finished', trace_blob_hash = ?3 WHERE run_id = ?1 AND ix = ?2",
            rusqlite::params![run_id, ix, trace_blob_hash.map(|h| h.as_slice())],
        )?;
        Ok(())
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
}
