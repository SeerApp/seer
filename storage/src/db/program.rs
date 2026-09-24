use anyhow::Result;

use super::Db;

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
        hash.map(super::as32).transpose()
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
}
