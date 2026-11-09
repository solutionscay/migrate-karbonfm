//! The app database: one SQLite file in the app data folder, schema 04 applied by refinery migrations, the
//! reference seed (OP-ADMIN-02) and, in a demo build, the demo seed.
//!
//! Open order, every start:
//! 1. Open the file and set the pragmas: `foreign_keys = ON` (OD-04-01), `journal_mode = WAL`,
//!    `synchronous = NORMAL`, `busy_timeout = 5000`.
//! 2. Run the refinery migrations that are not applied yet (history in `refinery_schema_history`), from
//!    `app/server/migrations/`. V1 is schema 04 as first written; later changes are new migrations, never edits of an
//!    applied one. `migration/04_database_schema.sql` shows the schema after the last migration (test
//!    `schema_04_equals_the_migrations`).
//! 3. Reference seed (OP-ADMIN-02) once per database, marked by `app_meta.reference_seed`.
//! 4. Demo build only: the demo seed (`migration/04_demo_seed.sql`) once, when the database has no party and no
//!    `app_meta.demo_data` row, in one transaction that ends with an empty `PRAGMA foreign_key_check` (03, "Demo seed
//!    data"). A database file in a data folder also gets the seed's stored files ([`DEMO_FILES`]).

use crate::audit::Actor;
use crate::reference;
use refinery::{Migration, Runner};
use rusqlite::Connection;
use std::fmt;
use std::path::{Path, PathBuf};

/// The database file in the app data folder (Tauri `app_data_dir`: `~/Library/Application Support/<identifier>` on
/// macOS, `%APPDATA%\<identifier>` on Windows, `~/.local/share/<identifier>` on Linux; D-17).
pub const FILE_NAME: &str = "karbonfm.db";

pub fn database_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

/// V1: schema 04 as it was when I1 shipped. Frozen: refinery refuses a database whose applied V1 differs.
const SCHEMA_04: &str = include_str!("../migrations/V1__schema_04.sql");
/// V2: the fixes of the DDR schema audit (migration/10_ddr_schema_audit.md).
const V2_DDR_AUDIT: &str = include_str!("../migrations/V2__ddr_audit.sql");
/// V3: retainer lines are not revenue (BR-SALES-17).
const V3_SALES_SUMMARY_VIEW: &str = include_str!("../migrations/V3__sales_summary_view.sql");
/// V4: the integration switches of I6, off by default (AP-12, BR-INTEG-02).
const V4_INTEGRATION_SETTINGS: &str = include_str!("../migrations/V4__integration_settings.sql");
const DEMO_SEED: &str = include_str!("../../../migration/04_demo_seed.sql");

/// The stored copies of the demo seed's file rows (B3): small placeholder files, each at its row's container_path
/// inside the data folder. Each row's file_size is the size of its copy (617 s104).
pub const DEMO_FILES: &[(&str, &[u8])] = &[
    ("files/2026/f-001.pdf", include_bytes!("../../../migration/demo_files/f-001.pdf")),
    ("files/2026/f-002.zip", include_bytes!("../../../migration/demo_files/f-002.zip")),
    ("files/2026/f-003.xlsx", include_bytes!("../../../migration/demo_files/f-003.xlsx")),
    ("files/2026/f-004.pdf", include_bytes!("../../../migration/demo_files/f-004.pdf")),
];

/// What to seed into a database that has none yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seed {
    /// The reference rows only (OP-ADMIN-02). A real install: setup or an import adds the rest.
    Reference,
    /// The reference rows, then the demo data of 04_demo_seed.sql when the database has no party, at its fixed dates
    /// (the tests).
    Demo,
    /// The same demo data, with its dates moved so the seed's 2026-10-28 falls on this day (B11: the dev server and the
    /// desktop's `demo` build pass the local date).
    DemoAsOf(chrono::NaiveDate),
}

/// What happened while opening: for the start-up log line and the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OpenReport {
    /// Migrations applied by this open, like `V1__schema_04`.
    pub migrations_applied: Vec<String>,
    pub reference_rows_inserted: usize,
    pub demo_seed_loaded: bool,
}

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Migration(refinery::Error),
    /// `PRAGMA foreign_key_check` found rows after the demo seed: (table, rowid, parent).
    ForeignKeyCheck(Vec<(String, i64, String)>),
    /// A pragma did not take, like `journal_mode` on a read-only folder.
    Pragma(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "database: {e}"),
            DbError::Migration(e) => write!(f, "database migration: {e}"),
            DbError::ForeignKeyCheck(rows) => {
                write!(f, "demo seed: foreign_key_check found {} rows: {rows:?}", rows.len())
            }
            DbError::Pragma(p) => write!(f, "database pragma: {p}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

impl From<refinery::Error> for DbError {
    fn from(e: refinery::Error) -> Self {
        DbError::Migration(e)
    }
}

/// Opens (creating it if needed) the database file at `path`, migrates and seeds it.
pub fn open(path: &Path, seed: Seed) -> Result<(Connection, OpenReport), DbError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| DbError::Pragma(format!("create {}: {e}", dir.display())))?;
    }
    let (conn, report) = prepare(Connection::open(path)?, seed, true)?;
    if report.demo_seed_loaded {
        if let Some(dir) = path.parent() {
            write_demo_files(dir)?;
        }
    }
    Ok((conn, report))
}

/// Writes the demo seed's stored copies into the data folder's file store; a file already there is left alone.
pub fn write_demo_files(data_dir: &Path) -> Result<(), DbError> {
    for (rel, bytes) in DEMO_FILES {
        let to = data_dir.join(rel);
        if to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DbError::Pragma(format!("create {}: {e}", parent.display())))?;
        }
        std::fs::write(&to, bytes).map_err(|e| DbError::Pragma(format!("write {}: {e}", to.display())))?;
    }
    Ok(())
}

/// An in-memory database, migrated and seeded the same way (tests, the dev server).
pub fn open_in_memory(seed: Seed) -> Result<(Connection, OpenReport), DbError> {
    prepare(Connection::open_in_memory()?, seed, false)
}

/// The S-01 entry point, kept for callers written against it: the demo database, in memory for `None`.
pub fn open_demo(path: Option<&Path>) -> Result<Connection, DbError> {
    let (conn, _) = match path {
        Some(p) => open(p, Seed::Demo)?,
        None => open_in_memory(Seed::Demo)?,
    };
    Ok(conn)
}

fn prepare(mut conn: Connection, seed: Seed, file: bool) -> Result<(Connection, OpenReport), DbError> {
    set_pragmas(&conn, file)?;
    let mut report = OpenReport { migrations_applied: migrate(&mut conn)?, ..Default::default() };
    report.reference_rows_inserted = reference::reference_seed(&mut conn, &Actor::system())?;
    match seed {
        Seed::Reference => {}
        Seed::Demo => report.demo_seed_loaded = load_demo_seed(&mut conn, None)?,
        Seed::DemoAsOf(today) => report.demo_seed_loaded = load_demo_seed(&mut conn, Some(today))?,
    }
    Ok((conn, report))
}

fn set_pragmas(conn: &Connection, file: bool) -> Result<(), DbError> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if fk != 1 {
        return Err(DbError::Pragma("foreign_keys is off".into()));
    }
    if file {
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(DbError::Pragma(format!("journal_mode is {mode}, not wal")));
        }
        conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    }
    Ok(())
}

/// The migrations, in order. V1 is schema 04 without its own `PRAGMA`/`BEGIN`/`COMMIT` lines: refinery runs each
/// migration in its own transaction, and the connection already has foreign keys on. Later files have no such lines.
pub fn migrations() -> Vec<Migration> {
    vec![
        Migration::unapplied("V1__schema_04", &strip_transaction(SCHEMA_04)).expect("V1 migration name"),
        Migration::unapplied("V2__ddr_audit", V2_DDR_AUDIT).expect("V2 migration name"),
        Migration::unapplied("V3__sales_summary_view", V3_SALES_SUMMARY_VIEW).expect("V3 migration name"),
        Migration::unapplied("V4__integration_settings", V4_INTEGRATION_SETTINGS).expect("V4 migration name"),
    ]
}

/// Applies the pending migrations and returns their names.
pub fn migrate(conn: &mut Connection) -> Result<Vec<String>, DbError> {
    let report = Runner::new(&migrations()).set_abort_divergent(true).set_abort_missing(true).run(conn)?;
    Ok(report.applied_migrations().iter().map(|m| m.to_string()).collect())
}

/// Removes the `PRAGMA foreign_keys = ON;`, `BEGIN;` and `COMMIT;` lines that let 04 and the demo seed run on their
/// own in `sqlite3`. Panics if the file does not have exactly one BEGIN and one COMMIT (a changed file must be looked
/// at, not silently run outside a transaction).
pub fn strip_transaction(sql: &str) -> String {
    let mut begins = 0;
    let mut commits = 0;
    let body: Vec<&str> = sql
        .lines()
        .filter(|line| match line.trim() {
            "BEGIN;" => {
                begins += 1;
                false
            }
            "COMMIT;" => {
                commits += 1;
                false
            }
            "PRAGMA foreign_keys = ON;" => false,
            _ => true,
        })
        .collect();
    assert!(begins == 1 && commits == 1, "expected one BEGIN and one COMMIT, found {begins} and {commits}");
    body.join("\n")
}

/// The app_meta key the demo seed writes.
pub const DEMO_KEY: &str = "demo_data";

/// True when the database holds the demo data (its `app_meta.demo_data` row). The sidebar shows DEMO only then.
pub fn has_demo_data(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM app_meta WHERE key = ?1)", [DEMO_KEY], |r| r.get(0))
}

/// Loads the demo seed when the database has no party and was never demo-seeded. Returns whether it loaded. With a date,
/// the seed's dates move so its 2026-10-28 falls on that day (B11, `demo_dates`), in the same transaction.
pub fn load_demo_seed(conn: &mut Connection, as_of: Option<chrono::NaiveDate>) -> Result<bool, DbError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let (parties, seeded): (i64, bool) = tx.query_row(
        "SELECT (SELECT COUNT(*) FROM party), EXISTS (SELECT 1 FROM app_meta WHERE key = ?1)",
        [DEMO_KEY],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if parties > 0 || seeded {
        return Ok(false);
    }
    tx.execute_batch(&strip_transaction(DEMO_SEED))?;
    if let Some(today) = as_of {
        let anchor = crate::demo_dates::parse_date(crate::demo_dates::SMALL_SEED_ANCHOR).expect("the anchor is a date");
        crate::demo_dates::shift(&tx, anchor, today)?;
    }
    let orphans = foreign_key_check(&tx)?;
    if !orphans.is_empty() {
        return Err(DbError::ForeignKeyCheck(orphans));
    }
    tx.commit()?;
    Ok(true)
}

/// `PRAGMA foreign_key_check` rows: (table, rowid, parent table).
pub fn foreign_key_check(conn: &Connection) -> rusqlite::Result<Vec<(String, i64, String)>> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0), r.get(2)?)))?.collect();
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_transaction_keeps_the_statements() {
        let sql = "PRAGMA foreign_keys = ON;\n\nBEGIN;\nCREATE TABLE t (id);\nCOMMIT;\n-- end";
        assert_eq!(strip_transaction(sql), "\nCREATE TABLE t (id);\n-- end");
        let schema = strip_transaction(SCHEMA_04);
        assert_eq!(schema.matches("CREATE TABLE").count(), SCHEMA_04.matches("CREATE TABLE").count());
        assert!(!schema.lines().any(|l| matches!(l.trim(), "BEGIN;" | "COMMIT;")));
    }

    #[test]
    #[should_panic(expected = "one BEGIN and one COMMIT")]
    fn strip_transaction_refuses_a_file_without_its_transaction() {
        strip_transaction("CREATE TABLE t (id);");
    }
}
