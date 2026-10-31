//! Backup and restore (OP-ADMIN-06, OP-ADMIN-07; BR-ADMIN-06, BR-ADMIN-07; 03 foundation gate "Backup").
//!
//! A backup is one folder, `KarbonFM backup <yyyy-mm-dd hhmmss>`, in a folder the user picked:
//! - `karbonfm.db`: a `VACUUM INTO` copy of the open database. VACUUM INTO reads in one transaction, so the copy is
//!   consistent while the app runs; the server also holds its only connection for the copy, so no write interleaves.
//! - `files/`: a copy of the file store (attachments, D-17; empty until I4).
//! - `manifest.json`, written last, so a folder without it is an unfinished backup: format, app version, schema
//!   version (refinery_schema_history), time, the row count of every table and the file count.
//!
//! Secrets are not in a backup: they are in the OS keychain, not in SQLite (AP-03).
//!
//! Restore checks the folder first ([`validate`]: manifest, `PRAGMA integrity_check`, `foreign_key_check`, the row
//! counts of the manifest, a schema version not newer than this app's). Only then does it take a safety backup of the
//! current data into `<data folder>/before-restore/`, close the connection, put the backup's database and files in
//! place and reopen (migrating an older schema). If the reopen fails, the safety copy goes back.

use crate::db::{self, Seed};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "manifest.json";
pub const FILES_DIR: &str = "files";
pub const FORMAT: u32 = 1;
const APP: &str = "KarbonFM";
/// Safety copies of the data as it was before each restore, in the data folder.
pub const SAFETY_DIR: &str = "before-restore";

/// The file store of an install: attachments are copied into the app data folder (D-17).
pub fn files_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(FILES_DIR)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub app: String,
    pub app_version: String,
    /// The highest applied migration (refinery_schema_history.version).
    pub schema_version: i64,
    /// Local civil time (D-05).
    pub created_at: String,
    /// Row count of every table of the copy, by name.
    pub tables: BTreeMap<String, i64>,
    pub files: u64,
    pub file_bytes: u64,
}

impl Manifest {
    pub fn total_rows(&self) -> i64 {
        self.tables.values().sum()
    }

    pub fn rows(&self, table: &str) -> i64 {
        self.tables.get(table).copied().unwrap_or(0)
    }
}

/// A backup folder and what its manifest says.
#[derive(Debug, Clone)]
pub struct Backup {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

#[derive(Debug)]
pub enum BackupError {
    /// The folder is not a KarbonFM backup (no or unreadable manifest, no database).
    NotABackup(String),
    /// The backup is damaged: integrity_check, foreign keys or the row counts disagree.
    Damaged(String),
    /// Made by a newer KarbonFM: its schema is newer than this app's.
    Newer { backup: i64, app: i64 },
    /// Reading or writing failed (disk, permissions, SQLite).
    Io(String),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackupError::NotABackup(why) => write!(f, "This folder is not a KarbonFM backup ({why})."),
            BackupError::Damaged(why) => write!(f, "This backup is damaged and cannot be restored ({why})."),
            BackupError::Newer { backup, app } => write!(
                f,
                "This backup was made by a newer version of KarbonFM (schema {backup}; this app has {app}). \
                 Update KarbonFM first."
            ),
            BackupError::Io(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for BackupError {}

fn io(what: &str, path: &Path, e: impl fmt::Display) -> BackupError {
    BackupError::Io(format!("{what} {}: {e}", path.display()))
}

/// The schema version this app migrates to.
pub fn app_schema_version() -> i64 {
    db::migrations().iter().map(|m| i64::from(m.version())).max().unwrap_or(0)
}

pub fn schema_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COALESCE(MAX(version), 0) FROM refinery_schema_history", [], |r| r.get(0))
}

/// Row count of every table (not views, not SQLite's own tables).
pub fn table_counts(conn: &Connection) -> rusqlite::Result<BTreeMap<String, i64>> {
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    names
        .into_iter()
        .map(|n| {
            let count =
                conn.query_row(&format!("SELECT COUNT(*) FROM \"{}\"", n.replace('"', "\"\"")), [], |r| r.get(0))?;
            Ok((n, count))
        })
        .collect()
}

/// OP-ADMIN-06: writes a new backup folder into `parent` and returns it.
pub fn create(conn: &Connection, files: &Path, parent: &Path) -> Result<Backup, BackupError> {
    if !parent.is_dir() {
        return Err(BackupError::Io(format!("{} is not a folder", parent.display())));
    }
    let stamp = chrono::Local::now();
    let base = format!("KarbonFM backup {}", stamp.format("%Y-%m-%d %H%M%S"));
    let dir = (1..)
        .map(|n| if n == 1 { parent.join(&base) } else { parent.join(format!("{base} ({n})")) })
        .find(|p| !p.exists())
        .expect("a free folder name");
    fs::create_dir(&dir).map_err(|e| io("could not create", &dir, e))?;
    let result = write_into(conn, files, &dir, stamp.format(crate::audit::TIMESTAMP_FORMAT).to_string());
    if result.is_err() {
        // Only the folder this call created, which has no manifest: an unfinished backup.
        let _ = fs::remove_dir_all(&dir);
    }
    result.map(|manifest| Backup { dir, manifest })
}

fn write_into(conn: &Connection, files: &Path, dir: &Path, created_at: String) -> Result<Manifest, BackupError> {
    let db_copy = db::database_path(dir);
    let target = db_copy.to_str().ok_or_else(|| BackupError::Io("the folder name is not valid UTF-8".into()))?;
    conn.execute("VACUUM INTO ?1", [target]).map_err(|e| io("could not copy the database to", &db_copy, e))?;
    let copy = open_read_only(&db_copy).map_err(|e| io("could not read", &db_copy, e))?;
    let tables = table_counts(&copy).map_err(|e| io("could not count the rows of", &db_copy, e))?;
    let schema_version = schema_version(&copy).map_err(|e| io("could not read the schema version of", &db_copy, e))?;
    drop(copy);
    let (n_files, file_bytes) = copy_tree(files, &dir.join(FILES_DIR))?;
    let manifest = Manifest {
        format: FORMAT,
        app: APP.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        schema_version,
        created_at,
        tables,
        files: n_files,
        file_bytes,
    };
    let json = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
    let tmp = dir.join("manifest.json.part");
    fs::write(&tmp, json).map_err(|e| io("could not write", &tmp, e))?;
    fs::rename(&tmp, dir.join(MANIFEST)).map_err(|e| io("could not write", &dir.join(MANIFEST), e))?;
    Ok(manifest)
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
}

/// Copies a folder tree (the file store); a missing source is an empty store. Returns (files, bytes).
fn copy_tree(from: &Path, to: &Path) -> Result<(u64, u64), BackupError> {
    fs::create_dir_all(to).map_err(|e| io("could not create", to, e))?;
    if !from.is_dir() {
        return Ok((0, 0));
    }
    let mut totals = (0, 0);
    for entry in fs::read_dir(from).map_err(|e| io("could not read", from, e))? {
        let entry = entry.map_err(|e| io("could not read", from, e))?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().map_err(|e| io("could not read", &src, e))?;
        if kind.is_dir() {
            let (n, b) = copy_tree(&src, &dst)?;
            totals = (totals.0 + n, totals.1 + b);
        } else if kind.is_file() {
            let b = fs::copy(&src, &dst).map_err(|e| io("could not copy", &src, e))?;
            totals = (totals.0 + 1, totals.1 + b);
        }
    }
    Ok(totals)
}

fn count_tree(dir: &Path) -> (u64, u64) {
    let Ok(entries) = fs::read_dir(dir) else { return (0, 0) };
    entries.flatten().fold((0, 0), |(n, b), e| match e.file_type() {
        Ok(t) if t.is_dir() => {
            let (n2, b2) = count_tree(&e.path());
            (n + n2, b + b2)
        }
        Ok(t) if t.is_file() => (n + 1, b + e.metadata().map(|m| m.len()).unwrap_or(0)),
        _ => (n, b),
    })
}

/// Checks a backup folder without changing anything (the first half of OP-ADMIN-07).
pub fn validate(dir: &Path) -> Result<Backup, BackupError> {
    let text =
        fs::read_to_string(dir.join(MANIFEST)).map_err(|_| BackupError::NotABackup("no manifest.json".into()))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| BackupError::NotABackup(format!("manifest.json: {e}")))?;
    if manifest.app != APP || manifest.format != FORMAT {
        return Err(BackupError::NotABackup(format!("format {} of {}", manifest.format, manifest.app)));
    }
    let app = app_schema_version();
    if manifest.schema_version > app {
        return Err(BackupError::Newer { backup: manifest.schema_version, app });
    }
    let db_file = db::database_path(dir);
    if !db_file.is_file() {
        return Err(BackupError::NotABackup(format!("no {}", db::FILE_NAME)));
    }
    let damaged = |e: rusqlite::Error| BackupError::Damaged(e.to_string());
    let conn = open_read_only(&db_file).map_err(damaged)?;
    let check: Vec<String> =
        conn.prepare("PRAGMA integrity_check")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    if check != ["ok"] {
        return Err(BackupError::Damaged(format!("integrity_check: {}", check.join("; "))));
    }
    let version = schema_version(&conn).map_err(damaged)?;
    if version > app {
        return Err(BackupError::Newer { backup: version, app });
    }
    if version != manifest.schema_version {
        return Err(BackupError::Damaged(format!("schema {version}, manifest says {}", manifest.schema_version)));
    }
    let fk: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).map_err(damaged)?;
    if fk != 0 {
        return Err(BackupError::Damaged(format!("{fk} rows fail the foreign key check")));
    }
    if table_counts(&conn).map_err(damaged)? != manifest.tables {
        return Err(BackupError::Damaged("the row counts differ from the manifest".into()));
    }
    if count_tree(&dir.join(FILES_DIR)) != (manifest.files, manifest.file_bytes) {
        return Err(BackupError::Damaged("the files differ from the manifest".into()));
    }
    Ok(Backup { dir: dir.to_path_buf(), manifest })
}

impl From<rusqlite::Error> for BackupError {
    fn from(e: rusqlite::Error) -> Self {
        BackupError::Damaged(e.to_string())
    }
}

/// What a restore did.
#[derive(Debug, Clone)]
pub struct Restored {
    pub manifest: Manifest,
    /// The safety backup of the data from before the restore.
    pub safety: PathBuf,
}

/// OP-ADMIN-07: validates `backup`, keeps a safety copy of the current data, swaps the backup in and reopens `slot`
/// (the server's connection) on it. A refused backup leaves everything as it was.
pub fn restore(slot: &mut Connection, data_dir: &Path, backup: &Path) -> Result<Restored, BackupError> {
    let chosen = validate(backup)?;
    let db_path = db::database_path(data_dir);
    let files = files_dir(data_dir);
    let safety_parent = data_dir.join(SAFETY_DIR);
    fs::create_dir_all(&safety_parent).map_err(|e| io("could not create", &safety_parent, e))?;
    let safety = create(slot, &files, &safety_parent)?;

    // Close the connection: SQLite checkpoints the WAL and removes it, so the file is complete on its own.
    let old = std::mem::replace(slot, Connection::open_in_memory().map_err(|e| BackupError::Io(e.to_string()))?);
    if let Err((conn, e)) = old.close() {
        *slot = conn;
        return Err(BackupError::Io(format!("could not close the database: {e}")));
    }
    let swapped = put_in_place(&chosen.dir, &db_path, &files).and_then(|()| reopen(&db_path));
    match swapped {
        Ok(conn) => {
            *slot = conn;
            Ok(Restored { manifest: chosen.manifest, safety: safety.dir })
        }
        Err(e) => {
            // Put the data from before back; the safety copy stays where it is.
            put_in_place(&safety.dir, &db_path, &files)?;
            *slot = reopen(&db_path)?;
            Err(BackupError::Io(format!("the restore failed and the data from before was put back: {e}")))
        }
    }
}

/// Replaces the database file and the file store with the ones in `from` (a backup folder).
fn put_in_place(from: &Path, db_path: &Path, files: &Path) -> Result<(), BackupError> {
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", db_path.display()));
        if side.exists() {
            fs::remove_file(&side).map_err(|e| io("could not remove", &side, e))?;
        }
    }
    let part = db_path.with_extension("db.restoring");
    fs::copy(db::database_path(from), &part).map_err(|e| io("could not copy the backup to", &part, e))?;
    fs::rename(&part, db_path).map_err(|e| io("could not replace", db_path, e))?;
    if files.exists() {
        fs::remove_dir_all(files).map_err(|e| io("could not replace", files, e))?;
    }
    copy_tree(&from.join(FILES_DIR), files)?;
    Ok(())
}

/// Opens the restored file like at start: pragmas, pending migrations (an older backup is migrated), reference seed
/// (already marked). Never the demo seed: a restored database is the user's data.
fn reopen(db_path: &Path) -> Result<Connection, BackupError> {
    db::open(db_path, Seed::Reference).map(|(c, _)| c).map_err(|e| io("could not open", db_path, e))
}
