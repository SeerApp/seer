use anyhow::Result;
use rusqlite::Connection;

const TABLES: &[&str] = &[
    include_str!("tables/simulation.sql"),
    include_str!("tables/program.sql"),
    include_str!("tables/historical_transaction.sql"),
    include_str!("tables/run.sql"),
    include_str!("tables/run_ix.sql"),
    include_str!("tables/reg.sql"),
    include_str!("tables/glassbox.sql"),
];

pub fn connect() -> Result<Connection> {
    let conn = Connection::open(super::home::db()?.join("seer.sqlite"))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    for sql in TABLES {
        conn.execute_batch(sql)?;
    }
    Ok(conn)
}

pub fn insert_simulation(
    conn: &Connection,
    hash: &[u8; 32],
    transaction_blob_hash: &[u8; 32],
    state_blob_hash: &[u8; 32],
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO simulation (hash, created_at, created_in_dir, transaction_blob_hash, state_blob_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            hash.as_slice(),
            chrono::Utc::now().to_rfc3339(),
            std::env::current_dir()?.display().to_string(),
            transaction_blob_hash.as_slice(),
            state_blob_hash.as_slice(),
        ],
    )?;
    Ok(())
}
