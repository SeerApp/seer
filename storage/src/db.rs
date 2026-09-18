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
    transaction_blob_hash: &[u8; 32],
    state_blob_hash: &[u8; 32],
) -> Result<()> {
    conn.execute(
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

pub fn insert_run(
    conn: &Connection,
    transaction_blob_hash: &[u8; 32],
    state_blob_hash: &[u8; 32],
    overrides: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO run (transaction_blob_hash, state_blob_hash, run_at, run_in_dir, overrides, status) VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
        rusqlite::params![
            transaction_blob_hash.as_slice(),
            state_blob_hash.as_slice(),
            chrono::Utc::now().to_rfc3339(),
            std::env::current_dir()?.display().to_string(),
            overrides,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_run(conn: &Connection, run_id: i64, error: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE run SET status = 'finished', error = ?2 WHERE id = ?1",
        rusqlite::params![run_id, error],
    )?;
    Ok(())
}

pub fn insert_run_ix(conn: &Connection, run_id: i64, ix: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO run_ix (run_id, ix, status) VALUES (?1, ?2, 'pending')",
        rusqlite::params![run_id, ix],
    )?;
    Ok(())
}

pub fn finish_run_ix(
    conn: &Connection,
    run_id: i64,
    ix: i64,
    trace_blob_hash: Option<&[u8; 32]>,
) -> Result<()> {
    conn.execute(
        "UPDATE run_ix SET status = 'finished', trace_blob_hash = ?3 WHERE run_id = ?1 AND ix = ?2",
        rusqlite::params![run_id, ix, trace_blob_hash.map(|h| h.as_slice())],
    )?;
    Ok(())
}

pub fn insert_program(conn: &Connection, self_blob_hash: &[u8; 32]) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO program (self_blob_hash) VALUES (?1)",
        rusqlite::params![self_blob_hash.as_slice()],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_reg(
    conn: &Connection,
    run_id: i64,
    ix: i64,
    start_step: i64,
    end_step: i64,
    self_blob_hash: &[u8; 32],
    program_blob_hash: &[u8; 32],
    pubkey: &[u8; 32],
) -> Result<()> {
    conn.execute(
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
