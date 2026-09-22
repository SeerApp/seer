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
}
