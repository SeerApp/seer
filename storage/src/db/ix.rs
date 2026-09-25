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

    #[allow(clippy::type_complexity)]
    pub fn list_reg(
        &self,
        run_id: i64,
    ) -> Result<Vec<(i64, i64, i64, [u8; 32], [u8; 32], [u8; 32])>> {
        let mut stmt = self.conn.prepare(
            "SELECT ix, start_step, end_step, self_blob_hash, program_blob_hash, pubkey FROM reg WHERE run_id = ?1 ORDER BY ix, start_step",
        )?;
        let rows = stmt.query_map([run_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, Vec<u8>>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (ix, start, end, self_h, prog_h, pk) = row?;
            Ok((ix, start, end, as32(self_h)?, as32(prog_h)?, as32(pk)?))
        })
        .collect()
    }

    pub fn glassbox_blob_hash(&self, run_id: i64, ix: i64) -> Result<Option<[u8; 32]>> {
        let mut stmt = self.conn.prepare(
            "SELECT glassbox_blob_hash FROM glassbox WHERE run_id = ?1 AND ix = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![run_id, ix])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        as32(row.get(0)?).map(Some)
    }

    pub fn upsert_glassbox(
        &self,
        run_id: i64,
        ix: i64,
        glassbox_blob_hash: &[u8; 32],
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO glassbox (run_id, ix, created_at, glassbox_blob_hash) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(run_id, ix) DO UPDATE SET created_at = excluded.created_at, glassbox_blob_hash = excluded.glassbox_blob_hash",
            rusqlite::params![
                run_id,
                ix,
                chrono::Utc::now().to_rfc3339(),
                glassbox_blob_hash.as_slice(),
            ],
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
