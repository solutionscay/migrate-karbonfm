//! OP-IMPORT-01 `import_filemaker_export` and the data behind OP-IMPORT-02 `import_report_get` (BR-IMPORT-01,
//! BR-IMPORT-02, BR-PARTY-06, BR-SALES-01/02/06; 03 "Data migration runbook (per install)").
//!
//! The export is one UTF-8 CSV per FileMaker base table, named `<Table>.csv`, with the field names as the header
//! (OD-03-01), plus the external container folder `Container/`. The import:
//! 1. reads every file (name, size, SHA-256, rows) and refuses an unknown table, a file that is not UTF-8, a broken
//!    CSV or a missing Party.csv before it creates anything;
//! 2. creates a NEW database in the data folder (schema and reference seed, `db::open`), and loads everything in one
//!    transaction, so a failed run leaves nothing (D-34);
//! 3. maps each field with the DDR audit register (`migration/10_ddr_schema_audit.tsv`): a field with a column is
//!    converted to the column's form, every other field is not imported and its non-empty values are counted;
//! 4. loads the tables in foreign-key order (a topological sort of the schema), keeping source ids and stamps; a row
//!    with an empty or duplicate key, a value of the wrong type or a missing parent goes to import_rejected_row;
//! 5. recomputes the stored calculations, caches and totals (D-39; [`recompute`]) and lists every value it stored
//!    differently from the export in import_changed_value;
//! 6. checks `PRAGMA foreign_key_check`, records the batch, its files and the report, and commits.
//!
//! Not imported, by decision: the Chat, ChatConsent and ChatMessage tables and chat-message attachments (OD-05-02);
//! the secret and PII fields of D-09 (BR-IMPORT-02); every field the register drops (QBO, plumbing, deprecated, ...).

pub mod convert;
pub mod csv;
pub mod recompute;

use crate::audit;
use crate::db::{self, DbError, Seed};
use karbon_money::filemaker;
use rusqlite::types::Value;
use rusqlite::{params, Connection, Transaction};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

const REGISTER: &str = include_str!("../../../../migration/10_ddr_schema_audit.tsv");

/// Source tables folded into a parent (D-15): their rows update the parent row, they have no table of their own.
const FOLDED: &[(&str, &str)] = &[("ActivityMessage", "activity"), ("Note", "activity"), ("Meeting", "event")];
/// Source tables that are skipped without being an error (OD-05-02).
const CHAT: &[&str] = &["Chat", "ChatConsent", "ChatMessage"];
/// The entity type of a chat message: its attachments are skipped (OD-05-02).
const CHAT_MESSAGE_ENTITY_TYPE: &str = "14";

/// How a run is identified and stamped. Tests fix both so two runs are byte for byte comparable.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub batch_id: Option<String>,
    /// Local civil time `YYYY-MM-DD HH:MM:SS` used for the batch stamps (D-05).
    pub now: Option<String>,
    /// The `user` row that is "me" (UI-18 step 2, BR-ADMIN-04). Default: the earliest user, as `admin::current_user`.
    pub me_user_id: Option<String>,
}

#[derive(Debug)]
pub enum ImportError {
    NotAFolder(PathBuf),
    /// The import loads into a new database only.
    DatabaseExists(PathBuf),
    UnknownTable(String),
    MissingTable(&'static str),
    NotUtf8(String),
    Csv {
        file: String,
        error: csv::CsvError,
    },
    EmptyFile(String),
    MeNotFound(String),
    Io(String),
    Db(DbError),
    /// Rows that still have a missing parent after the load (a bug: every row is checked before it is inserted).
    ForeignKeyCheck(Vec<(String, i64, String)>),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NotAFolder(p) => write!(f, "{} is not a folder", p.display()),
            ImportError::DatabaseExists(p) => {
                write!(f, "{} already exists: the import loads into a new database", p.display())
            }
            ImportError::UnknownTable(t) => write!(f, "{t}.csv is not a table of the FileMaker files"),
            ImportError::MissingTable(t) => write!(f, "the export has no {t}.csv"),
            ImportError::NotUtf8(t) => write!(f, "{t} is not UTF-8 text"),
            ImportError::Csv { file, error } => write!(f, "{file}: {error}"),
            ImportError::EmptyFile(t) => write!(f, "{t} has no header row"),
            ImportError::MeNotFound(u) => write!(f, "the export has no user {u}"),
            ImportError::Io(e) => write!(f, "{e}"),
            ImportError::Db(e) => write!(f, "{e}"),
            ImportError::ForeignKeyCheck(rows) => write!(f, "foreign_key_check found {} rows: {rows:?}", rows.len()),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<rusqlite::Error> for ImportError {
    fn from(e: rusqlite::Error) -> Self {
        ImportError::Db(DbError::Sqlite(e))
    }
}

impl From<DbError> for ImportError {
    fn from(e: DbError) -> Self {
        ImportError::Db(e)
    }
}

/// One export file and what became of its rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileReport {
    pub source_table: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub rows_read: usize,
    pub rows_loaded: usize,
    pub rows_rejected: usize,
    /// Rows not loaded by decision (chat tables and attachments, OD-05-02).
    pub rows_skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub source_table: String,
    pub source_key: Option<String>,
    pub reason: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub source_table: String,
    pub source_key: Option<String>,
    pub source_field: String,
    pub source_value: Option<String>,
    pub stored_value: Option<String>,
    pub reason: String,
}

/// The import report (OP-IMPORT-02). Also in the database: import_batch, import_file, import_rejected_row,
/// import_changed_value.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub batch_id: String,
    pub database: PathBuf,
    pub report_path: PathBuf,
    pub files: Vec<FileReport>,
    pub rejected: Vec<Rejected>,
    pub changed: Vec<Changed>,
    /// (source table, field) -> (rows with a value, decision) for every field the register does not import.
    pub not_imported: BTreeMap<(String, String), (usize, String)>,
    /// Stored calculation or cache column -> rows written by the recompute.
    pub recomputed: BTreeMap<String, usize>,
    pub containers_copied: usize,
    pub containers_missing: usize,
    pub warnings: Vec<String>,
    pub me_user_id: Option<String>,
    pub elapsed_ms: u128,
    /// Phase -> milliseconds (read, load, recompute, check, lineage).
    pub timings: Vec<(String, u128)>,
}

impl Report {
    pub fn file(&self, table: &str) -> Option<&FileReport> {
        self.files.iter().find(|f| f.source_table == table)
    }

    pub fn loaded(&self) -> usize {
        self.files.iter().map(|f| f.rows_loaded).sum()
    }
}

/// One register row of a karbon_data field.
#[derive(Debug, Clone)]
struct Field {
    data_type: &'static str,
    status: &'static str,
    /// (table, column) for status "column".
    target: Option<(&'static str, &'static str)>,
    target_type: &'static str,
    decision: &'static str,
}

struct Register {
    /// source table -> field -> register row
    fields: HashMap<&'static str, HashMap<&'static str, Field>>,
    /// every karbon_data table name, with the decision of a table that has no column
    tables: BTreeMap<&'static str, Option<&'static str>>,
    /// "table.column" -> (source table, field), for naming a recomputed value in the report
    by_target: HashMap<String, (&'static str, &'static str)>,
}

fn register() -> Register {
    let mut fields = HashMap::new();
    let mut tables: BTreeMap<&'static str, Option<&'static str>> = BTreeMap::new();
    let mut by_target = HashMap::new();
    for line in REGISTER.lines().skip(1) {
        let c: Vec<&'static str> = line.split('\t').collect();
        if c[0] != "karbon_data.fmp12" {
            continue;
        }
        let target = c[11].split_once('.').filter(|_| c[10] == "column");
        if let Some((t, col)) = target {
            by_target.entry(format!("{t}.{col}")).or_insert((c[2], c[4]));
        }
        let entry = tables.entry(c[2]).or_insert(Some(c[15]));
        if target.is_some() {
            *entry = None;
        }
        fields
            .entry(c[2])
            .or_insert_with(HashMap::new)
            .insert(c[4], Field { data_type: c[5], status: c[10], target, target_type: c[12], decision: c[15] });
    }
    Register { fields, tables, by_target }
}

/// A file read and parsed before the database is created.
struct Source {
    table: &'static str,
    report: FileReport,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

/// Imports the export in `export_dir` into a new database in `data_dir` (`karbonfm.db`, `files/`).
pub fn import_export(export_dir: &Path, data_dir: &Path, opts: &Options) -> Result<Report, ImportError> {
    let started = Instant::now();
    if !export_dir.is_dir() {
        return Err(ImportError::NotAFolder(export_dir.to_path_buf()));
    }
    let db_path = db::database_path(data_dir);
    if db_path.exists() {
        return Err(ImportError::DatabaseExists(db_path));
    }
    let reg = register();
    let sources = read_export(export_dir, &reg)?;
    let read_ms = started.elapsed().as_millis();
    let mut created: Vec<PathBuf> = Vec::new();
    let result = run(export_dir, data_dir, &db_path, &reg, sources, opts, &mut created);
    match result {
        Ok(mut report) => {
            report.timings.insert(0, ("read".into(), read_ms));
            report.elapsed_ms = started.elapsed().as_millis();
            Ok(report)
        }
        Err(e) => {
            // Leave nothing behind (D-34): the database, its WAL files and the copied containers.
            for p in created.iter().rev() {
                let _ = std::fs::remove_file(p);
            }
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(PathBuf::from(format!("{}{suffix}", db_path.display())));
            }
            Err(e)
        }
    }
}

fn read_export(dir: &Path, reg: &Register) -> Result<Vec<Source>, ImportError> {
    let io = |e: std::io::Error| ImportError::Io(format!("{}: {e}", dir.display()));
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("csv")))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for path in names {
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let table = *reg.tables.keys().find(|t| **t == stem).ok_or_else(|| ImportError::UnknownTable(stem.clone()))?;
        let bytes = std::fs::read(&path).map_err(io)?;
        let sha256 = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
        let text = String::from_utf8(bytes.clone()).map_err(|_| ImportError::NotUtf8(file_name.clone()))?;
        let mut records = csv::parse(&text).map_err(|error| ImportError::Csv { file: file_name.clone(), error })?;
        if records.is_empty() {
            return Err(ImportError::EmptyFile(file_name));
        }
        let header = records.remove(0);
        out.push(Source {
            table,
            report: FileReport {
                source_table: table.to_string(),
                file_name,
                size_bytes: bytes.len() as u64,
                sha256,
                rows_read: records.len(),
                ..Default::default()
            },
            header,
            rows: records,
        });
    }
    if !out.iter().any(|s| s.table == "Party") {
        return Err(ImportError::MissingTable("Party"));
    }
    Ok(out)
}

/// Facts about a target table, read from the schema.
#[derive(Debug, Clone)]
struct TargetTable {
    pk: String,
    pk_numeric: bool,
    /// column -> (parent table, parent column numeric?)
    fks: BTreeMap<String, (String, bool)>,
    /// had rows before the import (reference seed): rows are upserted
    seeded: bool,
}

/// How a field's text becomes its column value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conv {
    Text,
    /// A text primary or foreign key: stored trimmed, as the key checks compare it.
    Key,
    Number,
    /// Number field into an INTEGER money column (OD-04-02).
    Money,
    /// sales_line.qty: at most 4 decimals (BR-SALES-01).
    Qty,
    /// A Text field into a NUMERIC column (ActivityEntity.EntityTypeId; D-16).
    NumberFromText,
    /// vendor.credit_limit: Text in the source, money in the target; not a number -> NULL, listed (BR-IMPORT-01).
    CreditLimit,
    Date,
    Time,
    Timestamp,
    Container,
}

struct Col {
    idx: usize,
    field: &'static str,
    column: &'static str,
    conv: Conv,
}

struct Loader<'a> {
    tx: &'a Transaction<'a>,
    reg: &'a Register,
    export_dir: &'a Path,
    data_dir: &'a Path,
    targets: HashMap<String, TargetTable>,
    /// target table -> canonical keys present (seeded or loaded)
    keys: HashMap<String, HashSet<String>>,
    /// target table -> keys loaded by this run (duplicate check)
    loaded: HashMap<String, HashSet<String>>,
    /// (table, column) foreign keys that close a cycle: checked after the load
    deferred: BTreeSet<(String, String)>,
    report: Report,
    created: &'a mut Vec<PathBuf>,
}

fn run(
    export_dir: &Path,
    data_dir: &Path,
    db_path: &Path,
    reg: &Register,
    mut sources: Vec<Source>,
    opts: &Options,
    created: &mut Vec<PathBuf>,
) -> Result<Report, ImportError> {
    let mut clock = Instant::now();
    let mut timings: Vec<(String, u128)> = Vec::new();
    let mut lap = |name: &str| {
        timings.push((name.to_string(), clock.elapsed().as_millis()));
        clock = Instant::now();
    };
    let (mut conn, _) = db::open(db_path, Seed::Reference)?;
    let now = opts.now.clone().unwrap_or_else(audit::local_now);
    let batch_id = opts.batch_id.clone().unwrap_or_else(crate::ids::new_id);
    let (order, deferred) = load_order(&conn)?;
    let targets = target_tables(&conn)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
    tx.execute(
        "INSERT INTO import_batch (id, started_at, source_description, status) VALUES (?1, ?2, ?3, 'running')",
        params![batch_id, now, export_dir.display().to_string()],
    )?;
    let mut keys: HashMap<String, HashSet<String>> = HashMap::new();
    for (t, info) in &targets {
        let mut stmt = tx.prepare(&format!("SELECT CAST({} AS TEXT) FROM \"{t}\"", info.pk))?;
        let set = stmt
            .query_map([], |r| r.get::<_, Option<String>>(0))?
            .filter_map(|k| k.ok().flatten())
            .map(|k| convert::canonical_key(&k, info.pk_numeric))
            .collect();
        keys.insert(t.clone(), set);
    }
    let mut loader = Loader {
        tx: &tx,
        reg,
        export_dir,
        data_dir,
        targets,
        keys,
        loaded: HashMap::new(),
        deferred,
        report: Report { batch_id: batch_id.clone(), database: db_path.to_path_buf(), ..Default::default() },
        created,
    };

    // Load order: by the position of the target table in the foreign-key order; folded tables after their parent.
    let position = |s: &Source| -> (usize, bool, &'static str) {
        let folded = FOLDED.iter().find(|(t, _)| *t == s.table).map(|(_, p)| *p);
        let target = folded.or_else(|| loader_target(reg, s.table));
        let pos = target.and_then(|t| order.iter().position(|o| o == t)).unwrap_or(usize::MAX);
        (pos, folded.is_some(), s.table)
    };
    sources.sort_by_key(position);
    for mut s in sources {
        if CHAT.contains(&s.table) {
            s.report.rows_skipped = s.rows.len();
        } else if FOLDED.iter().any(|(t, _)| *t == s.table) {
            loader.load_folded(&mut s)?;
        } else if let Some(target) = loader_target(reg, s.table) {
            loader.load_table(&mut s, target)?;
        } else {
            // A karbon_data table the plan drops (meta, Globals, HTTPRequestLog, ...): skipped by its decision.
            s.report.rows_skipped = s.rows.len();
            let decision = reg.tables.get(s.table).copied().flatten().unwrap_or("");
            loader.report.warnings.push(format!("{}: not imported ({decision})", s.report.file_name));
        }
        loader.report.files.push(s.report);
    }

    loader.resolve_deferred()?;
    lap("load");

    if let Some(me) = &opts.me_user_id {
        if !loader.keys.get("user").is_some_and(|k| k.contains(me)) {
            return Err(ImportError::MeNotFound(me.clone()));
        }
        tx.execute("INSERT OR REPLACE INTO app_meta (key, value) VALUES ('local_user', ?1)", [me])?;
        loader.report.me_user_id = Some(me.clone());
    } else {
        loader.report.me_user_id = crate::admin::current_user(&tx)?.and_then(|a| a.user_id);
    }

    recompute::all(&tx, &mut loader.report, &|target: &str| {
        reg.by_target.get(target).map(|(t, f)| (t.to_string(), f.to_string()))
    })?;

    lap("recompute");
    let orphans = db::foreign_key_check(&tx)?;
    lap("foreign_key_check");
    if !orphans.is_empty() {
        return Err(ImportError::ForeignKeyCheck(orphans));
    }
    let report = loader.report;
    write_lineage(&tx, &report)?;
    let report_path = data_dir.join(format!("import-report-{}.md", &batch_id[..batch_id.len().min(8)]));
    tx.execute(
        "UPDATE import_batch SET finished_at = ?2, status = 'completed', report_path = ?3 WHERE id = ?1",
        params![batch_id, now, report_path.file_name().unwrap().to_string_lossy()],
    )?;
    tx.execute("INSERT OR REPLACE INTO app_meta (key, value) VALUES ('import_batch', ?1)", [&batch_id])?;
    tx.commit()?;
    // Statistics for the query planner: an imported database is large from its first page.
    conn.execute_batch("ANALYZE;")?;
    lap("lineage, commit and analyze");
    let mut report = report;
    let detail = std::mem::take(&mut report.timings);
    report.timings = timings;
    report.timings.extend(detail);
    report.report_path = report_path.clone();
    std::fs::write(&report_path, render_markdown(&report)).map_err(|e| ImportError::Io(e.to_string()))?;
    created.push(report_path);
    drop(conn);
    Ok(report)
}

/// The target table of a source table: the table its columns go to.
fn loader_target(reg: &Register, table: &str) -> Option<&'static str> {
    reg.fields.get(table)?.values().find_map(|f| f.target.map(|(t, _)| t))
}

/// Foreign keys checked after the load: (table, column).
type Deferred = BTreeSet<(String, String)>;

/// The tables of the schema in foreign-key order, every parent before its children, and the foreign keys that close
/// a cycle (activity.activity_purpose_id -> activity_purpose -> activity). Those columns are checked after the load
/// with foreign keys deferred (03 runbook: `PRAGMA defer_foreign_keys`).
fn load_order(conn: &Connection) -> Result<(Vec<String>, Deferred), ImportError> {
    let tables: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             AND name <> 'refinery_schema_history' ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        rows
    };
    // table -> [(column, parent)]
    let mut edges: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for t in &tables {
        let mut stmt = conn.prepare(&format!("PRAGMA foreign_key_list('{t}')"))?;
        let es: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(3)?, r.get::<_, String>(2)?)))?
            .filter_map(Result::ok)
            .filter(|(_, p)| p != t)
            .collect();
        edges.insert(t.clone(), es);
    }
    let mut order = Vec::new();
    let mut done = BTreeSet::new();
    let mut deferred = BTreeSet::new();
    while order.len() < tables.len() {
        let unmet = |t: &String, done: &BTreeSet<String>, deferred: &BTreeSet<(String, String)>| {
            edges[t].iter().filter(|(c, p)| !done.contains(p) && !deferred.contains(&(t.clone(), c.clone()))).count()
        };
        let next: Vec<String> =
            tables.iter().filter(|t| !done.contains(*t) && unmet(t, &done, &deferred) == 0).cloned().collect();
        if next.is_empty() {
            // A cycle: defer the unmet keys of the table with the fewest of them.
            let t = tables
                .iter()
                .filter(|t| !done.contains(*t))
                .min_by_key(|t| (unmet(t, &done, &deferred), (*t).clone()))
                .unwrap()
                .clone();
            for (c, p) in &edges[&t] {
                if !done.contains(p) {
                    deferred.insert((t.clone(), c.clone()));
                }
            }
            continue;
        }
        for t in next {
            done.insert(t.clone());
            order.push(t);
        }
    }
    Ok((order, deferred))
}

fn target_tables(conn: &Connection) -> Result<HashMap<String, TargetTable>, ImportError> {
    let mut out = HashMap::new();
    let names: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             AND name NOT IN ('refinery_schema_history', 'app_meta', 'value_list_item') AND name NOT LIKE 'import_%'",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        rows
    };
    let col_type = |t: &str, c: &str| -> rusqlite::Result<String> {
        conn.query_row(&format!("SELECT type FROM pragma_table_info('{t}') WHERE name = ?1"), [c], |r| r.get(0))
    };
    for t in names {
        let (pk, pk_type): (String, String) =
            conn.query_row(&format!("SELECT name, type FROM pragma_table_info('{t}') WHERE pk = 1"), [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let mut fks = BTreeMap::new();
        let mut stmt = conn.prepare(&format!("PRAGMA foreign_key_list('{t}')"))?;
        let rows: Vec<(String, String, String)> =
            stmt.query_map([], |r| Ok((r.get(3)?, r.get(2)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
        for (from, parent, parent_col) in rows {
            let numeric = col_type(&parent, &parent_col)? != "TEXT";
            fks.insert(from, (parent, numeric));
        }
        let seeded: bool = conn.query_row(&format!("SELECT EXISTS (SELECT 1 FROM \"{t}\")"), [], |r| r.get(0))?;
        out.insert(t, TargetTable { pk, pk_numeric: pk_type != "TEXT", fks, seeded });
    }
    Ok(out)
}

/// A rejected row's fields as JSON (header -> value), for import_rejected_row.raw_json. Only the fields the import
/// stores are kept: every field the register drops (the D-09 secrets, Twilio tokens, QBO keys, ...) and every field
/// the DDR does not have is left out, so a rejected row never stores what an imported row may not (BR-IMPORT-02).
fn raw_json(reg: &Register, table: &str, header: &[String], row: &[String]) -> String {
    let fields = reg.fields.get(table);
    let map: serde_json::Map<String, serde_json::Value> = header
        .iter()
        .zip(row.iter().chain(std::iter::repeat(&String::new())))
        .filter(|(_, v)| !v.is_empty())
        .filter(|(h, _)| fields.and_then(|m| m.get(h.as_str())).is_some_and(|f| f.target.is_some()))
        .map(|(h, v)| (h.clone(), serde_json::Value::String(v.clone())))
        .collect();
    serde_json::Value::Object(map).to_string()
}

impl Loader<'_> {
    fn reject(&mut self, s: &mut Source, row: &[String], key: Option<&str>, reason: String) {
        s.report.rows_rejected += 1;
        self.report.rejected.push(Rejected {
            source_table: s.table.to_string(),
            source_key: key.filter(|k| !k.is_empty()).map(str::to_string),
            reason,
            raw_json: raw_json(self.reg, s.table, &s.header, row),
        });
    }

    fn change(&mut self, table: &str, key: &str, field: &str, from: Option<&str>, to: Option<String>, reason: &str) {
        self.report.changed.push(Changed {
            source_table: table.to_string(),
            source_key: Some(key.to_string()),
            source_field: field.to_string(),
            source_value: from.map(str::to_string),
            stored_value: to,
            reason: reason.to_string(),
        });
    }

    /// The columns a source table's header feeds, and the counting of fields that are not imported.
    fn plan(&mut self, s: &Source, target: &str) -> Vec<Col> {
        let mut cols = Vec::new();
        for (idx, name) in s.header.iter().enumerate() {
            let Some((&field, f)) = self.reg.fields.get(s.table).and_then(|m| m.get_key_value(name.as_str())) else {
                self.report.warnings.push(format!("{}: field {name} is not in the DDR; ignored", s.report.file_name));
                continue;
            };
            let values = s.rows.iter().filter(|r| r.get(idx).is_some_and(|v| !v.trim().is_empty())).count();
            match f.target {
                Some((t, column)) if t == target && f.status == "column" => {
                    let conv = match (f.data_type, f.target_type) {
                        _ if t == "sales_line" && column == "qty" => Conv::Qty,
                        ("Text", "INTEGER") => Conv::CreditLimit,
                        ("Text", "NUMERIC") => Conv::NumberFromText,
                        ("Text", _)
                            if column == self.targets[target].pk || self.targets[target].fks.contains_key(column) =>
                        {
                            Conv::Key
                        }
                        ("Text", _) => Conv::Text,
                        ("Number", "INTEGER") => Conv::Money,
                        ("Number", _) => Conv::Number,
                        ("Date", _) => Conv::Date,
                        ("Time", _) => Conv::Time,
                        ("TimeStamp", _) => Conv::Timestamp,
                        ("Binary", _) => Conv::Container,
                        other => unreachable!("type {other:?}"),
                    };
                    cols.push(Col { idx, field, column, conv });
                }
                _ if values > 0 => {
                    self.report
                        .not_imported
                        .insert((s.table.to_string(), field.to_string()), (values, f.decision.to_string()));
                }
                _ => {}
            }
        }
        cols
    }

    fn load_table(&mut self, s: &mut Source, target: &'static str) -> Result<(), ImportError> {
        let cols = self.plan(s, target);
        let info = self.targets[target].clone();
        let singleton = target == "system_setting";
        let key_col = cols.iter().position(|c| c.column == info.pk);
        if key_col.is_none() && !singleton {
            self.report.warnings.push(format!("{}: no key field; nothing loaded", s.report.file_name));
            s.report.rows_rejected = s.rows.len();
            return Ok(());
        }
        let mut columns: Vec<&str> = cols.iter().map(|c| c.column).collect();
        if singleton {
            columns.push("singleton");
        }
        let names = columns.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
        let marks = (1..=columns.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
        let mut sql = format!("INSERT INTO \"{target}\" ({names}) VALUES ({marks})");
        if info.seeded {
            let set = columns
                .iter()
                .filter(|c| **c != info.pk)
                .map(|c| format!("\"{c}\" = excluded.\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            sql.push_str(&format!(" ON CONFLICT (\"{}\") DO UPDATE SET {set}", info.pk));
        }
        let rows = std::mem::take(&mut s.rows);
        for row in &rows {
            let get = |i: usize| row.get(i).map_or("", |v| v.as_str());
            let key = key_col.map(|k| get(cols[k].idx).trim().to_string()).unwrap_or_else(|| "1".into());
            if key.is_empty() {
                self.reject(s, row, None, "empty key".into());
                continue;
            }
            let canon = convert::canonical_key(&key, info.pk_numeric);
            if self.loaded.get(target).is_some_and(|l| l.contains(&canon)) {
                let why =
                    if singleton { "more than one System row".to_string() } else { format!("duplicate key {key}") };
                self.reject(s, row, Some(&key), why);
                continue;
            }
            if target == "file_attachment" {
                let et = cols.iter().find(|c| c.column == "entity_type_id").map(|c| get(c.idx).trim());
                if et == Some(CHAT_MESSAGE_ENTITY_TYPE) {
                    s.report.rows_skipped += 1;
                    continue;
                }
            }
            let mut values: Vec<Value> = Vec::with_capacity(columns.len());
            let mut changes: Vec<(&'static str, String, Option<String>, &'static str)> = Vec::new();
            let mut problem = None;
            for c in &cols {
                let raw = get(c.idx);
                match self.convert(c, raw, target, &key) {
                    Ok((v, change)) => {
                        if let Some((stored, why)) = change {
                            changes.push((c.field, raw.to_string(), stored, why));
                        }
                        values.push(v);
                    }
                    Err(reason) => {
                        problem = Some(reason);
                        break;
                    }
                }
            }
            if problem.is_none() {
                problem = self.missing_parent(target, &info, &cols, &values);
            }
            if let Some(reason) = problem {
                self.reject(s, row, Some(&key), reason);
                continue;
            }
            if singleton {
                values.push(Value::Integer(1));
            }
            // BR-PARTY-05: the source wrote customer roles with an empty party_role_type_id; kept and listed.
            if target == "party_role" {
                let is =
                    |col: &str| cols.iter().zip(&values).any(|(c, v)| c.column == col && *v == Value::Text("1".into()));
                let empty_type =
                    !cols.iter().zip(&values).any(|(c, v)| c.column == "party_role_type_id" && *v != Value::Null);
                if is("customer_ind") && empty_type {
                    changes.push((
                        "PartyRoleTypeId",
                        String::new(),
                        None,
                        "customer role with an empty party_role_type_id, kept (BR-PARTY-05)",
                    ));
                }
            }
            self.tx.prepare_cached(&sql)?.execute(rusqlite::params_from_iter(values.iter()))?;
            self.keys.entry(target.to_string()).or_default().insert(canon.clone());
            self.loaded.entry(target.to_string()).or_default().insert(canon);
            s.report.rows_loaded += 1;
            for (field, from, to, why) in changes {
                let from = (!from.is_empty()).then_some(from);
                self.change(s.table, &key, field, from.as_deref(), to, why);
            }
        }
        Ok(())
    }

    /// The first foreign key whose parent row is not there.
    fn missing_parent(&self, target: &str, info: &TargetTable, cols: &[Col], values: &[Value]) -> Option<String> {
        for (c, v) in cols.iter().zip(values) {
            let Some((parent, numeric)) = info.fks.get(c.column) else { continue };
            if self.deferred.contains(&(target.to_string(), c.column.to_string())) {
                continue;
            }
            let text = match v {
                Value::Null => continue,
                Value::Text(t) => t.clone(),
                Value::Integer(i) => i.to_string(),
                Value::Real(f) => f.to_string(),
                Value::Blob(_) => continue,
            };
            let canon = convert::canonical_key(&text, *numeric);
            if !self.keys.get(parent).is_some_and(|k| k.contains(&canon)) {
                return Some(format!("missing parent {parent} for {} '{text}'", c.field));
            }
        }
        None
    }

    /// Converts one field. Ok((value, Some((stored text, reason)))) when the stored value differs from the export.
    #[allow(clippy::type_complexity)]
    fn convert(
        &mut self,
        c: &Col,
        raw: &str,
        target: &str,
        key: &str,
    ) -> Result<(Value, Option<(Option<String>, &'static str)>), String> {
        let v = raw.trim();
        if v.is_empty() && c.conv != Conv::Text {
            return Ok((Value::Null, None));
        }
        let bad = |what: &str| format!("{what} in {}: '{raw}'", c.field);
        Ok(match c.conv {
            Conv::Text => (if raw.is_empty() { Value::Null } else { Value::Text(raw.to_string()) }, None),
            Conv::Key => {
                let change = (v != raw).then(|| (Some(v.to_string()), "spaces around a key removed (BR-IMPORT-01)"));
                (Value::Text(v.to_string()), change)
            }
            Conv::Number | Conv::NumberFromText => {
                if !convert::is_number(v) {
                    return Err(bad("not a number"));
                }
                (Value::Text(v.to_string()), None)
            }
            Conv::Money | Conv::CreditLimit => match filemaker::cents(v) {
                Ok(r) => {
                    let change = (!r.exact).then(|| {
                        (Some(r.value.to_input_value()), "rounded to the cent, half away from zero (BR-IMPORT-01)")
                    });
                    (Value::Integer(r.value.0), change)
                }
                Err(_) if c.conv == Conv::CreditLimit => {
                    (Value::Null, Some((None, "credit_limit is not a number: stored as empty (BR-IMPORT-01)")))
                }
                Err(_) => return Err(bad("not a number")),
            },
            Conv::Qty => match filemaker::qty(v) {
                Ok(r) => {
                    let text = r.value.to_input_value();
                    let change =
                        (!r.exact).then(|| (Some(text.clone()), "quantity rounded to 4 decimals (BR-SALES-01)"));
                    (Value::Text(text), change)
                }
                Err(_) => return Err(bad("not a number")),
            },
            Conv::Date => (Value::Text(convert::date(v).ok_or_else(|| bad("bad date"))?), None),
            Conv::Time => (Value::Text(convert::time(v).ok_or_else(|| bad("bad time"))?), None),
            Conv::Timestamp => (Value::Text(convert::timestamp(v).ok_or_else(|| bad("bad timestamp"))?), None),
            Conv::Container => match self.copy_container(v, target, c.column, key) {
                Some(path) => (Value::Text(path), None),
                None => (Value::Null, Some((None, "container file missing from the export (D-17)"))),
            },
        })
    }

    /// Copies `Container/<...>` from the export into `files/` of the data folder (D-17) and returns the stored path.
    fn copy_container(&mut self, value: &str, table: &str, column: &str, key: &str) -> Option<String> {
        // Only a relative path inside the export folder: no `..`, no drive or root, no link leading out of it.
        let rel = value.trim_start_matches(['/', '\\']).replace('\\', "/");
        let inside = !rel.split('/').any(|p| p == ".." || p.contains(':')) && !rel.is_empty();
        let from = self.export_dir.join(&rel);
        let contained = inside
            && match (from.canonicalize(), self.export_dir.canonicalize()) {
                (Ok(f), Ok(base)) => f.starts_with(base) && f.is_file(),
                _ => false,
            };
        if !contained {
            self.report.containers_missing += 1;
            return None;
        }
        let ext = from.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let year =
            rel.split('/').nth(1).filter(|y| y.len() == 4 && y.bytes().all(|b| b.is_ascii_digit())).unwrap_or("import");
        let safe: String =
            key.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '_' }).collect();
        let suffix = match (table, column) {
            ("file", "container_path") => String::new(),
            (_, col) => format!("-{}", col.trim_end_matches("_path")),
        };
        let stored = format!("{}/{year}/{safe}{suffix}{ext}", crate::backup::FILES_DIR);
        let to = self.data_dir.join(&stored);
        std::fs::create_dir_all(to.parent()?).ok()?;
        std::fs::copy(&from, &to).ok()?;
        self.created.push(to);
        self.report.containers_copied += 1;
        Some(stored)
    }

    /// The cycle-closing foreign keys, checked once every table is loaded: a reference whose parent is still missing
    /// is cleared and listed (the row itself stays, its children are already loaded).
    fn resolve_deferred(&mut self) -> Result<(), ImportError> {
        for (table, column) in self.deferred.clone() {
            let info = self.targets[&table].clone();
            let (parent, _) = info.fks[&column].clone();
            let parent_pk = self.targets[&parent].pk.clone();
            let sql = format!(
                "SELECT t.\"{}\", CAST(t.\"{column}\" AS TEXT) FROM \"{table}\" t WHERE t.\"{column}\" IS NOT NULL \
                 AND NOT EXISTS (SELECT 1 FROM \"{parent}\" p WHERE p.\"{parent_pk}\" = t.\"{column}\") ORDER BY 1",
                info.pk
            );
            let rows: Vec<(String, Option<String>)> = {
                let mut stmt = self.tx.prepare(&sql)?;
                let rows =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
                rows
            };
            let field = self
                .reg
                .by_target
                .get(&format!("{table}.{column}"))
                .map_or((String::new(), column.clone()), |(t, f)| (t.to_string(), f.to_string()));
            for (key, value) in rows {
                self.tx.execute(
                    &format!("UPDATE \"{table}\" SET \"{column}\" = NULL WHERE \"{}\" = ?1", info.pk),
                    [&key],
                )?;
                let why = format!("missing parent {parent}: reference cleared (BR-IMPORT-01)");
                self.change(&field.0, &key, &field.1, value.as_deref(), None, &why);
            }
        }
        Ok(())
    }

    /// ActivityMessage, Note and Meeting (D-15): each row updates its parent; no parent is a rejected row.
    fn load_folded(&mut self, s: &mut Source) -> Result<(), ImportError> {
        let parent = FOLDED.iter().find(|(t, _)| *t == s.table).unwrap().1;
        let idx = |name: &str| s.header.iter().position(|h| h == name);
        let key_idx = if s.table == "ActivityMessage" { idx("ActivityId") } else { idx("Id") };
        // The row's own FileMaker Id names it in the report (an ActivityMessage has one besides its ActivityId).
        let own_idx = idx("Id");
        let (msg, edited) = (idx("Message"), idx("ManuallyEditedInd"));
        // Count the fields that are not imported (stamps of the folded row: the parent's stamps win).
        for (i, name) in s.header.iter().enumerate() {
            let used = Some(i) == key_idx || Some(i) == msg || Some(i) == edited;
            let values = s.rows.iter().filter(|r| r.get(i).is_some_and(|v| !v.trim().is_empty())).count();
            if !used && values > 0 {
                let decision = self
                    .reg
                    .fields
                    .get(s.table)
                    .and_then(|m| m.get(name.as_str()))
                    .map_or("not in the DDR", |f| f.decision);
                let decision = if decision.is_empty() { "folded; the parent's value is kept (D-15)" } else { decision };
                self.report.not_imported.insert((s.table.to_string(), name.clone()), (values, decision.to_string()));
            }
        }
        let rows = std::mem::take(&mut s.rows);
        let mut seen = HashSet::new();
        for row in &rows {
            let key = key_idx.and_then(|i| row.get(i)).map_or("", |v| v.trim()).to_string();
            let own = own_idx.and_then(|i| row.get(i)).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            let name = own.clone().unwrap_or_else(|| key.clone());
            if key.is_empty() {
                self.reject(s, row, own.as_deref(), "empty key".into());
                continue;
            }
            if !self.keys.get(parent).is_some_and(|k| k.contains(&key)) {
                let field = if s.table == "ActivityMessage" { "ActivityId" } else { "Id" };
                self.reject(s, row, Some(&name), format!("missing parent {parent} for {field} '{key}'"));
                continue;
            }
            if !seen.insert(key.clone()) {
                self.reject(s, row, Some(&name), format!("duplicate key {key} (one {} per {parent}, D-15)", s.table));
                continue;
            }
            match s.table {
                "ActivityMessage" => {
                    let get = |i: Option<usize>| i.and_then(|i| row.get(i)).filter(|v| !v.is_empty()).cloned();
                    let edited_v = get(edited);
                    if edited_v.as_deref().is_some_and(|v| !convert::is_number(v)) {
                        self.reject(
                            s,
                            row,
                            Some(&name),
                            format!("not a number in ManuallyEditedInd: '{}'", edited_v.unwrap()),
                        );
                        continue;
                    }
                    self.tx
                        .prepare_cached("UPDATE activity SET message = ?2, manually_edited_ind = ?3 WHERE id = ?1")?
                        .execute(params![key, get(msg), edited_v])?;
                }
                "Note" => {
                    self.tx.prepare_cached("UPDATE activity SET note_ind = 1 WHERE id = ?1")?.execute([&key])?;
                }
                _ => {
                    self.tx.prepare_cached("UPDATE event SET meeting_ind = 1 WHERE id = ?1")?.execute([&key])?;
                }
            }
            s.report.rows_loaded += 1;
        }
        Ok(())
    }
}

fn write_lineage(tx: &Transaction, r: &Report) -> rusqlite::Result<()> {
    let mut f = tx.prepare(
        "INSERT INTO import_file (batch_id, source_table, file_name, size_bytes, sha256, encoding, rows_read, rows_loaded, \
         rows_rejected) VALUES (?1, ?2, ?3, ?4, ?5, 'UTF-8', ?6, ?7, ?8)",
    )?;
    for x in &r.files {
        f.execute(params![
            r.batch_id,
            x.source_table,
            x.file_name,
            x.size_bytes as i64,
            x.sha256,
            x.rows_read as i64,
            x.rows_loaded as i64,
            x.rows_rejected as i64
        ])?;
    }
    let mut rej = tx.prepare(
        "INSERT INTO import_rejected_row (batch_id, source_table, source_key, reason, raw_json) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for x in &r.rejected {
        rej.execute(params![r.batch_id, x.source_table, x.source_key, x.reason, x.raw_json])?;
    }
    let mut ch = tx.prepare(
        "INSERT INTO import_changed_value (batch_id, source_table, source_key, source_field, source_value, stored_value, \
         reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for x in &r.changed {
        ch.execute(params![
            r.batch_id,
            x.source_table,
            x.source_key,
            x.source_field,
            x.source_value,
            x.stored_value,
            x.reason
        ])?;
    }
    Ok(())
}

/// The report as Markdown: counts per table, rejects, changed values by reason, values not imported.
pub fn render_markdown(r: &Report) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "# FileMaker import {}\n", r.batch_id);
    let _ = writeln!(
        s,
        "{} rows loaded from {} files, {} rejected, {} values changed, {} containers copied ({} missing), \"me\": {}.\n",
        r.loaded(),
        r.files.len(),
        r.rejected.len(),
        r.changed.len(),
        r.containers_copied,
        r.containers_missing,
        r.me_user_id.as_deref().unwrap_or("none")
    );
    let _ = writeln!(
        s,
        "## Files\n\n| File | Rows | Loaded | Rejected | Skipped | SHA-256 |\n|---|---:|---:|---:|---:|---|"
    );
    for f in &r.files {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | `{}` |",
            f.file_name,
            f.rows_read,
            f.rows_loaded,
            f.rows_rejected,
            f.rows_skipped,
            &f.sha256[..12]
        );
    }
    let _ = writeln!(s, "\n## Rejected rows\n\n| Table | Key | Reason |\n|---|---|---|");
    for x in &r.rejected {
        let _ = writeln!(s, "| {} | {} | {} |", x.source_table, x.source_key.as_deref().unwrap_or(""), x.reason);
    }
    let mut by_reason: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for x in &r.changed {
        *by_reason.entry((&x.source_table, &x.source_field, &x.reason)).or_default() += 1;
    }
    let _ = writeln!(s, "\n## Changed values\n\n| Table | Field | Reason | Rows |\n|---|---|---|---:|");
    for ((t, f, why), n) in by_reason {
        let _ = writeln!(s, "| {t} | {f} | {why} | {n} |");
    }
    let _ = writeln!(s, "\n## Recomputed\n\n| Column | Rows |\n|---|---:|");
    for (c, n) in &r.recomputed {
        let _ = writeln!(s, "| {c} | {n} |");
    }
    let _ =
        writeln!(s, "\n## Values not imported\n\n| Table | Field | Rows with a value | Decision |\n|---|---|---:|---|");
    for ((t, f), (n, d)) in &r.not_imported {
        let _ = writeln!(s, "| {t} | {f} | {n} | {d} |");
    }
    let _ = writeln!(s, "\n## Timings\n\n| Phase | ms |\n|---|---:|");
    for (p, ms) in &r.timings {
        let _ = writeln!(s, "| {p} | {ms} |");
    }
    if !r.warnings.is_empty() {
        let _ = writeln!(s, "\n## Warnings\n");
        for w in &r.warnings {
            let _ = writeln!(s, "- {w}");
        }
    }
    s
}
