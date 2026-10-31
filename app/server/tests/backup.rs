//! Backup and restore (OP-ADMIN-06, OP-ADMIN-07, BR-ADMIN-06, BR-ADMIN-07): the command layer on a demo database
//! file in a temp folder. The routes and the dialog flow are in `tests/backup_routes.rs`.

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use karbon_server::backup::{self, BackupError};
use karbon_server::db::{self, Seed};
use karbon_server::{router, AppState};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

struct Install {
    conn: Connection,
    data: PathBuf,
    _tmp: tempfile::TempDir,
}

/// A demo install: karbonfm.db (migrated, seeded) and a file store with two attachments.
fn install() -> Install {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let (conn, _) = db::open(&db::database_path(&data), Seed::Demo).unwrap();
    let files = backup::files_dir(&data);
    fs::create_dir_all(files.join("2026/10")).unwrap();
    fs::write(files.join("2026/10/contract.pdf"), b"%PDF-1.7 demo").unwrap();
    fs::write(files.join("logo.png"), [0u8, 1, 2, 3]).unwrap();
    Install { conn, data, _tmp: tmp }
}

fn backups_folder(i: &Install) -> PathBuf {
    let dir = i.data.parent().unwrap().join("Backups");
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The customers page of a connection, through the router (the money figures as the user sees them).
async fn customers_page(conn: Connection) -> String {
    let res =
        router(AppState::new(conn)).oneshot(Request::get("/customers").body(Body::empty()).unwrap()).await.unwrap();
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

fn file_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

#[test]
fn backup_writes_a_consistent_copy_and_manifest() {
    let i = install();
    let b = backup::create(&i.conn, &backup::files_dir(&i.data), &backups_folder(&i)).unwrap();
    assert!(b.dir.file_name().unwrap().to_str().unwrap().starts_with("KarbonFM backup "));
    let m = &b.manifest;
    assert_eq!((m.format, m.app.as_str(), m.schema_version), (1, "KarbonFM", backup::app_schema_version()));
    assert_eq!(m.tables, backup::table_counts(&i.conn).unwrap(), "every table, same counts as the live database");
    assert_eq!(m.rows("party"), 30);
    assert!(karbon_server::audit::is_timestamp(&m.created_at));
    assert_eq!((m.files, m.file_bytes), (2, 13 + 4));
    assert_eq!(file_bytes(&b.dir.join("files/2026/10/contract.pdf")), b"%PDF-1.7 demo");
    // The manifest on disk is the one returned, and the folder passes the restore checks.
    assert_eq!(&backup::validate(&b.dir).unwrap().manifest, m);
    let copy = Connection::open(db::database_path(&b.dir)).unwrap();
    let ok: String = copy.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(ok, "ok");
    // No secret is in SQLite to begin with (AP-03); the copy has no secret-like column either.
    let n: i64 = copy
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master m, pragma_table_info(m.name) p WHERE m.type = 'table' \
             AND (p.name LIKE '%password%' OR p.name LIKE '%api_key%' OR p.name LIKE '%secret%')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    // A second backup in the same second gets its own folder.
    let again = backup::create(&i.conn, &backup::files_dir(&i.data), &backups_folder(&i)).unwrap();
    assert_ne!(again.dir, b.dir);
}

#[tokio::test]
async fn restore_brings_back_the_same_rows_money_and_files() {
    let mut i = install();
    let files = backup::files_dir(&i.data);
    let b = backup::create(&i.conn, &files, &backups_folder(&i)).unwrap();
    let before = backup::table_counts(&i.conn).unwrap();

    // Change the live data after the backup: a payment line gone, an attachment gone, a new file.
    i.conn.execute_batch("PRAGMA foreign_keys = OFF; DELETE FROM sales_line; PRAGMA foreign_keys = ON;").unwrap();
    fs::remove_file(files.join("logo.png")).unwrap();
    fs::write(files.join("new.txt"), b"after the backup").unwrap();
    assert_ne!(backup::table_counts(&i.conn).unwrap(), before);

    let restored = backup::restore(&mut i.conn, &i.data, &b.dir).unwrap();
    assert_eq!(restored.manifest, b.manifest);
    assert_eq!(backup::table_counts(&i.conn).unwrap(), before, "identical row counts");
    let ok: String = i.conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(ok, "ok");
    assert!(files.join("logo.png").is_file() && !files.join("new.txt").exists(), "the file store is the backup's");

    // The safety copy holds the data from before the restore, and is itself a restorable backup.
    assert!(restored.safety.starts_with(i.data.join(backup::SAFETY_DIR)));
    let safety = backup::validate(&restored.safety).unwrap();
    assert_eq!(safety.manifest.rows("sales_line"), 0);
    assert!(restored.safety.join("files/new.txt").is_file());

    // The connection works for writes, in WAL mode, and the pages show the seed's money figures.
    let mode: String = i.conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    let html = customers_page(i.conn).await;
    assert!(html.contains("$29,619.05"), "balance due of 04_demo_seed.sql");
    assert!(html.contains("$4,450.00") && html.contains("of 14"));
}

/// The live database and file store, to show a refused restore changed nothing.
fn snapshot(i: &Install) -> (std::collections::BTreeMap<String, i64>, Vec<u8>, bool) {
    i.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    (
        backup::table_counts(&i.conn).unwrap(),
        file_bytes(&db::database_path(&i.data)),
        backup::files_dir(&i.data).join("logo.png").is_file(),
    )
}

fn assert_refused_and_untouched(i: &mut Install, dir: &Path, expect: fn(&BackupError) -> bool) {
    let before = snapshot(i);
    let err = backup::restore(&mut i.conn, &i.data, dir).unwrap_err();
    assert!(expect(&err), "{err:?}");
    assert_eq!(snapshot(i), before, "the current database is untouched");
    assert!(!i.data.join(backup::SAFETY_DIR).exists(), "no safety copy for a refused backup");
}

#[test]
fn a_corrupt_backup_is_refused() {
    let mut i = install();
    let b = backup::create(&i.conn, &backup::files_dir(&i.data), &backups_folder(&i)).unwrap();
    // Overwrite a stretch of pages in the middle of the copy.
    let path = db::database_path(&b.dir);
    let mut bytes = file_bytes(&path);
    let mid = bytes.len() / 2;
    bytes[mid..mid + 8192].fill(0xA5);
    fs::write(&path, bytes).unwrap();
    assert!(matches!(backup::validate(&b.dir), Err(BackupError::Damaged(_))));
    assert_refused_and_untouched(&mut i, &b.dir, |e| matches!(e, BackupError::Damaged(_)));
}

#[test]
fn a_tampered_or_incomplete_backup_is_refused() {
    let mut i = install();
    let folder = backups_folder(&i);
    let b = backup::create(&i.conn, &backup::files_dir(&i.data), &folder).unwrap();
    // Rows removed after the manifest was written.
    let copy = Connection::open(db::database_path(&b.dir)).unwrap();
    copy.execute_batch("PRAGMA foreign_keys = OFF; DELETE FROM sales_line;").unwrap();
    drop(copy);
    assert_refused_and_untouched(&mut i, &b.dir, |e| matches!(e, BackupError::Damaged(_)));
    // A folder without a manifest (an unfinished backup, or any folder).
    assert_refused_and_untouched(&mut i, &folder, |e| matches!(e, BackupError::NotABackup(_)));
    // A missing attachment.
    let b2 = backup::create(&i.conn, &backup::files_dir(&i.data), &folder).unwrap();
    fs::remove_file(b2.dir.join("files/logo.png")).unwrap();
    assert_refused_and_untouched(&mut i, &b2.dir, |e| matches!(e, BackupError::Damaged(_)));
}

#[test]
fn a_backup_from_a_newer_schema_is_refused() {
    let mut i = install();
    let b = backup::create(&i.conn, &backup::files_dir(&i.data), &backups_folder(&i)).unwrap();
    // As a later KarbonFM would write it: one more migration applied, and its manifest says so.
    let copy = Connection::open(db::database_path(&b.dir)).unwrap();
    copy.execute(
        "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES (?1, 'future', '', '0')",
        [backup::app_schema_version() + 1],
    )
    .unwrap();
    let mut m = b.manifest.clone();
    m.schema_version = backup::app_schema_version() + 1;
    m.tables = backup::table_counts(&copy).unwrap();
    drop(copy);
    fs::write(b.dir.join(backup::MANIFEST), serde_json::to_string(&m).unwrap()).unwrap();
    assert_refused_and_untouched(&mut i, &b.dir, |e| matches!(e, BackupError::Newer { .. }));
    let msg = backup::validate(&b.dir).unwrap_err().to_string();
    assert!(msg.contains("newer version of KarbonFM"), "{msg}");
}

#[test]
fn backup_needs_an_existing_folder_and_leaves_no_half_backup() {
    let i = install();
    let missing = i.data.join("no-such-folder");
    assert!(matches!(backup::create(&i.conn, &backup::files_dir(&i.data), &missing), Err(BackupError::Io(_))));
    assert!(!missing.exists());
}

#[test]
fn backup_is_consistent_while_another_connection_writes() {
    let i = install();
    let path = db::database_path(&i.data);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    // A second writer (WAL) inserting pairs of rows in one transaction each: a torn copy would hold half a pair.
    let writer = std::thread::spawn(move || {
        let conn = Connection::open(path).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS pairs (n INTEGER, side TEXT)").unwrap();
        let mut n = 0;
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; INSERT INTO pairs VALUES ({n}, 'a'); INSERT INTO pairs VALUES ({n}, 'b'); COMMIT;"
            ))
            .unwrap();
            n += 1;
        }
        n
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    let mut backups = Vec::new();
    for _ in 0..5 {
        backups.push(backup::create(&i.conn, &backup::files_dir(&i.data), &backups_folder(&i)).unwrap());
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(writer.join().unwrap() > 0);
    for b in backups {
        backup::validate(&b.dir).unwrap();
        let copy = Connection::open(db::database_path(&b.dir)).unwrap();
        let odd: i64 = copy.query_row("SELECT COUNT(*) % 2 FROM pairs", [], |r| r.get(0)).unwrap();
        assert_eq!(odd, 0, "a transaction was split by the copy");
    }
}
