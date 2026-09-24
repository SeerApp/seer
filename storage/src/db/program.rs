use anyhow::Result;

use super::{as32, Db};

pub struct ProgramChunk {
    pub start_pc: i64,
    pub end_pc: i64,
    pub blob_hash: [u8; 32],
}

impl Db {
    pub fn insert_program(&self, self_blob_hash: &[u8; 32]) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO program (self_blob_hash) VALUES (?1)",
            rusqlite::params![self_blob_hash.as_slice()],
        )?;
        Ok(())
    }

    pub fn program_idl_blob_hash(&self, self_blob_hash: &[u8; 32]) -> Result<Option<[u8; 32]>> {
        let mut stmt = self
            .conn
            .prepare("SELECT idl_blob_hash FROM program WHERE self_blob_hash = ?1")?;
        let mut rows = stmt.query(rusqlite::params![self_blob_hash.as_slice()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let hash: Option<Vec<u8>> = row.get(0)?;
        hash.map(as32).transpose()
    }

    pub fn set_program_idl_blob_hash(
        &self,
        self_blob_hash: &[u8; 32],
        idl_blob_hash: &[u8; 32],
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE program SET idl_blob_hash = ?2 WHERE self_blob_hash = ?1 AND idl_blob_hash IS NULL",
            rusqlite::params![self_blob_hash.as_slice(), idl_blob_hash.as_slice()],
        )?;
        Ok(())
    }

    pub fn program_disasm_chunks(&self, program_blob_hash: &[u8; 32]) -> Result<Vec<ProgramChunk>> {
        list_chunks(&self.conn, "program_disasm", program_blob_hash)
    }

    pub fn program_lifted_chunks(&self, program_blob_hash: &[u8; 32]) -> Result<Vec<ProgramChunk>> {
        list_chunks(&self.conn, "program_lifted", program_blob_hash)
    }

    pub fn insert_program_disasm(
        &self,
        program_blob_hash: &[u8; 32],
        chunks: &[ProgramChunk],
    ) -> Result<()> {
        insert_chunks(&self.conn, "program_disasm", program_blob_hash, chunks)
    }

    pub fn insert_program_lifted(
        &self,
        program_blob_hash: &[u8; 32],
        chunks: &[ProgramChunk],
    ) -> Result<()> {
        insert_chunks(&self.conn, "program_lifted", program_blob_hash, chunks)
    }
}

fn list_chunks(
    conn: &rusqlite::Connection,
    table: &str,
    program_blob_hash: &[u8; 32],
) -> Result<Vec<ProgramChunk>> {
    let sql = format!(
        "SELECT start_pc, end_pc, blob_hash FROM {table} WHERE program_blob_hash = ?1 ORDER BY start_pc"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![program_blob_hash.as_slice()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (start_pc, end_pc, blob_hash) = row?;
        Ok(ProgramChunk {
            start_pc,
            end_pc,
            blob_hash: as32(blob_hash)?,
        })
    })
    .collect()
}

fn insert_chunks(
    conn: &rusqlite::Connection,
    table: &str,
    program_blob_hash: &[u8; 32],
    chunks: &[ProgramChunk],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let count_sql = format!("SELECT COUNT(*) FROM {table} WHERE program_blob_hash = ?1");
    let exists: i64 = tx.query_row(
        &count_sql,
        rusqlite::params![program_blob_hash.as_slice()],
        |row| row.get(0),
    )?;
    if exists > 0 {
        tx.commit()?;
        return Ok(());
    }
    {
        let insert_sql = format!(
            "INSERT INTO {table} (program_blob_hash, start_pc, end_pc, blob_hash) VALUES (?1, ?2, ?3, ?4)"
        );
        let mut stmt = tx.prepare(&insert_sql)?;
        for chunk in chunks {
            stmt.execute(rusqlite::params![
                program_blob_hash.as_slice(),
                chunk.start_pc,
                chunk.end_pc,
                chunk.blob_hash.as_slice(),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}
