//! Opens the SQLite database. For the spike the demo database is built from schema 04 and the demo seed on first
//! open; I1 replaces this with refinery migrations.

use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = include_str!("../../../migration/04_database_schema.sql");
const DEMO_SEED: &str = include_str!("../../../migration/04_demo_seed.sql");

/// Opens the database at `path` (in memory when `None`), and loads schema 04 and the demo seed if it is empty.
pub fn open_demo(path: Option<&Path>) -> rusqlite::Result<Connection> {
    let conn = match path {
        Some(p) => Connection::open(p)?,
        None => Connection::open_in_memory()?,
    };
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    let empty: bool =
        conn.query_row("SELECT COUNT(*) = 0 FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0))?;
    if empty {
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(DEMO_SEED)?;
    }
    Ok(conn)
}
