use anyhow::Result;

use super::Db;

impl Db {
    pub fn insert_simulation(
        &self,
        transaction_blob_hash: &[u8; 32],
        state_blob_hash: &[u8; 32],
    ) -> Result<()> {
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
}
