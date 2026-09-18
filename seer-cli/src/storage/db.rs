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
