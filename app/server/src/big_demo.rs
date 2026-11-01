//! The big demo database (owner decision 2026-11-01, 03 "Demo seed data"): the synthetic FileMaker export of
//! karbon-synth, imported by the I3 importer. The desktop app creates it at first run, when its data folder has no
//! database; `karbon-dev-server --big-demo` serves the same data.
//!
//! - An existing database is never replaced or changed: [`create`] does nothing when `karbonfm.db` exists.
//! - The export and the new database are built in `<data dir>/.big-demo-staging`, then moved into the data folder,
//!   the database file last. A run that stops half way leaves no `karbonfm.db`, so the next start builds it again.
//! - The database is marked as demo data (`app_meta.demo_data`), so the sidebar shows DEMO.
//! - Deterministic: the same seed gives the same rows (only the import stamps differ).

use crate::backup::FILES_DIR;
use crate::db::{self, DEMO_KEY};
use crate::import::{self, ImportError};
use std::fmt;
use std::path::Path;
use std::time::Instant;

/// The staging folder inside the data folder (same volume, so the final moves are renames).
pub const STAGING_DIR: &str = ".big-demo-staging";

/// The `app_meta.demo_data` value of the big demo.
pub const DEMO_VALUE: &str = "karbon-synth seed 20261101, imported by karbon-import (owner, 2026-11-01)";

/// What [`create`] did, for the start-up log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The data folder already has a database: nothing was done.
    Existing,
    /// The data folder has a file store but no database (a half-restored or hand-edited folder): nothing was done,
    /// so no file of it is mixed with the demo's.
    FilesWithoutDatabase,
    Created {
        rows: usize,
        generate_ms: u128,
        import_ms: u128,
        total_ms: u128,
    },
}

#[derive(Debug)]
pub enum BigDemoError {
    Io(String),
    Import(ImportError),
    Db(db::DbError),
}

impl fmt::Display for BigDemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BigDemoError::Io(e) => write!(f, "big demo: {e}"),
            BigDemoError::Import(e) => write!(f, "big demo import: {e}"),
            BigDemoError::Db(e) => write!(f, "big demo: {e}"),
        }
    }
}

impl std::error::Error for BigDemoError {}

impl From<ImportError> for BigDemoError {
    fn from(e: ImportError) -> Self {
        BigDemoError::Import(e)
    }
}

impl From<rusqlite::Error> for BigDemoError {
    fn from(e: rusqlite::Error) -> Self {
        BigDemoError::Db(db::DbError::Sqlite(e))
    }
}

fn io<'a>(what: &'a str, path: &'a Path) -> impl Fn(std::io::Error) -> BigDemoError + 'a {
    move |e| BigDemoError::Io(format!("{what} {}: {e}", path.display()))
}

/// Creates the big demo database in `data_dir` when it has none. Call before `db::open`.
pub fn create(data_dir: &Path) -> Result<Outcome, BigDemoError> {
    let db_path = db::database_path(data_dir);
    if db_path.exists() {
        return Ok(Outcome::Existing);
    }
    if data_dir.join(FILES_DIR).exists() {
        return Ok(Outcome::FilesWithoutDatabase);
    }
    let started = Instant::now();
    let staging = data_dir.join(STAGING_DIR);
    if staging.exists() {
        // Left by a run that stopped half way.
        std::fs::remove_dir_all(&staging).map_err(io("remove", &staging))?;
    }
    let (export, built) = (staging.join("export"), staging.join("data"));
    std::fs::create_dir_all(&built).map_err(io("create", &built))?;

    karbon_synth::generate(karbon_synth::Config::default(), &export).map_err(io("generate", &export))?;
    let generate_ms = started.elapsed().as_millis();
    let report = import::import_export(&export, &built, &import::Options::default())?;

    // Mark it as demo data (DEMO badge), and leave one self-contained file: no -wal or -shm beside it.
    {
        let conn = rusqlite::Connection::open(db::database_path(&built))?;
        conn.execute("INSERT OR REPLACE INTO app_meta (key, value) VALUES (?1, ?2)", [DEMO_KEY, DEMO_VALUE])?;
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    }

    // Move everything but the database, then the database: its arrival is what makes the folder "has a database".
    let built_db = db::database_path(&built);
    let mut entries: Vec<_> = std::fs::read_dir(&built)
        .map_err(io("read", &built))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| *p != built_db)
        .collect();
    entries.sort();
    for from in entries {
        let to = data_dir.join(from.file_name().unwrap_or_default());
        std::fs::rename(&from, &to).map_err(io("move", &from))?;
    }
    std::fs::rename(&built_db, &db_path).map_err(io("move", &built_db))?;
    std::fs::remove_dir_all(&staging).map_err(io("remove", &staging))?;
    Ok(Outcome::Created {
        rows: report.loaded(),
        generate_ms,
        import_ms: report.elapsed_ms,
        total_ms: started.elapsed().as_millis(),
    })
}
