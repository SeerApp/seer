use anyhow::Result;

use super::{as32, Db};

type Lookup = ([u8; 32], [u8; 32]);

impl Db {
    pub fn lookup_sig(&self, sig: &[u8; 64], network: Option<&str>) -> Result<Option<Lookup>> {
        let mut stmt = self.conn.prepare(
            "SELECT transaction_blob_hash, state_blob_hash FROM historical_transaction WHERE sig = ?1 AND (?2 IS NULL OR network = ?2) LIMIT 1",
        )?;
        let mut rows = stmt.query(rusqlite::params![sig.as_slice(), network])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some((as32(row.get(0)?)?, as32(row.get(1)?)?)))
    }

    pub fn insert_historical(
        &self,
        transaction_blob_hash: &[u8; 32],
        state_blob_hash: &[u8; 32],
        network: &str,
        sig: &[u8; 64],
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO historical_transaction (transaction_blob_hash, state_blob_hash, network, sig) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                transaction_blob_hash.as_slice(),
                state_blob_hash.as_slice(),
                network,
                sig.as_slice(),
            ],
        )?;
        Ok(())
    }
}
