//! The big demo database the desktop app opens on (owner decision 2026-11-01, 03 "Demo seed data"): a new data
//! folder gets it and opens straight to the app; an existing database is never touched.
#![cfg(feature = "big-demo")]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::big_demo::{self, Outcome};
use karbon_server::db::{self, Seed};
use karbon_server::{admin, router, AppState};
use std::path::Path;
use tower::ServiceExt;

const BADGE: &str = r#"<span class="brand-env">DEMO</span>"#;

async fn get(state: &AppState, uri: &str) -> (StatusCode, String) {
    let res = router(state.clone()).oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// Every file under `dir` with its bytes, for "nothing changed" checks.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.strip_prefix(dir).unwrap().display().to_string(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn new_data_folder_gets_the_big_demo_and_opens_without_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    std::fs::create_dir_all(dir.join("logs")).unwrap(); // the desktop app starts its log first

    let Outcome::Created { rows, .. } = big_demo::create(&dir).unwrap() else { panic!("not created") };
    assert!(rows > 40_000, "{rows} rows");
    assert!(!dir.join(big_demo::STAGING_DIR).exists(), "the staging folder is removed");
    assert!(dir.join("files").is_dir(), "the containers are in the file store");
    for suffix in ["-wal", "-shm"] {
        assert!(!dir.join(format!("karbonfm.db{suffix}")).exists(), "one self-contained file");
    }

    // What the desktop app does next: open it as a real install would (reference rows only).
    let (conn, report) = db::open(&db::database_path(&dir), Seed::Reference).unwrap();
    assert!(report.migrations_applied.is_empty(), "the import migrated it already");
    assert!(admin::setup_done(&conn).unwrap());
    assert_eq!(admin::current_user(&conn).unwrap().and_then(|a| a.user_id).as_deref(), Some("u-mara"));
    assert!(db::has_demo_data(&conn).unwrap());
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert!(count("SELECT COUNT(*) FROM party") > 400);
    assert!(count("SELECT COUNT(*) FROM sales_transaction") > 3_000);
    let (tax, phones): (i64, i64) = conn
        .query_row(
            "SELECT tax_rate_bp, automatically_format_phone_numbers_ind FROM system_setting WHERE singleton = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((tax, phones), (550, 1));
    // B2: a client contact's EmployerName is the organization it was made with (kontroller 229 s229).
    let employer: Option<String> =
        conn.query_row("SELECT employer_name FROM person WHERE id = 'p-hp-eh'", [], |r| r.get(0)).unwrap();
    assert_eq!(employer.as_deref(), Some("Halden & Pryce Architects"));
    assert!(count("SELECT COUNT(*) FROM person WHERE employer_name IS NOT NULL") > 100);
    drop(conn);

    // No secret of the export reached the database (D-09, OD-05-02).
    let bytes = std::fs::read(db::database_path(&dir)).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    for secret in ["synthetic-secret-not-imported", "synthetic-token-not-imported", "synthetic-twilio-not-imported"] {
        assert!(!text.contains(secret), "{secret} is in the database");
    }
    assert!(!text.contains("900-00-000"), "an SSN is in the database");

    // Every page opens on it, with the DEMO badge, and none sends the window to /setup.
    let state = AppState::new(db::open(&db::database_path(&dir), Seed::Reference).unwrap().0).with_data_dir(&dir);
    let (status, html) = get(&state, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Good day, Mara"), "the greeting names the current user");
    assert!(html.contains(BADGE));
    for uri in [
        "/customers",
        "/customers?page=5",
        "/parties",
        "/parties/p-hp",
        "/parties/new",
        "/sales",
        "/payments",
        "/sales/INV-1048",
        "/sales/INV-12000",
        "/settings",
        "/activities",
        "/activities?kind=task&status=overdue",
        "/activities?page=3",
        "/items",
    ] {
        let (status, html) = get(&state, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains(BADGE), "{uri} shows DEMO");
    }
    assert_eq!(get(&state, "/setup").await.0, StatusCode::SEE_OTHER, "setup is done");

    // S2: the item list and the picker, under the same limit as the other long lists.
    for (uri, expect) in [("/items", "of "), ("/items?q=a", "of "), ("/pickers/item", "item-picker")] {
        let start = std::time::Instant::now();
        let (status, html) = get(&state, uri).await;
        let took = start.elapsed();
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains(expect), "{uri}");
        assert!(took.as_millis() < 600, "{uri} took {took:?}");
    }

    // 06 "Data-density": a page of a long list renders fast. Some 3,000 activities here; the debug build gets a
    // generous margin over the 200 ms of 06 (a release build is several times faster).
    for uri in ["/activities?page=2", "/activities?kind=task&status=incomplete"] {
        let start = std::time::Instant::now();
        let (status, html) = get(&state, uri).await;
        let took = start.elapsed();
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("tl-item"), "{uri} lists activities");
        assert!(took.as_millis() < 600, "{uri} took {took:?}");
    }
}

#[test]
fn existing_database_is_never_touched() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // The user's own data: here the small seed, closed so its WAL is folded in.
    drop(db::open(&db::database_path(dir), Seed::Demo).unwrap());
    let before = snapshot(dir);
    assert_eq!(big_demo::create(dir).unwrap(), Outcome::Existing);
    assert_eq!(snapshot(dir), before, "no file added, changed or removed");
    let (conn, _) = db::open(&db::database_path(dir), Seed::Reference).unwrap();
    let parties: i64 = conn.query_row("SELECT COUNT(*) FROM party", [], |r| r.get(0)).unwrap();
    assert!(parties < 100, "still the small seed: {parties} parties");
}

#[test]
fn a_file_store_without_a_database_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("files/2024")).unwrap();
    std::fs::write(dir.join("files/2024/a.pdf"), b"user file").unwrap();
    let before = snapshot(dir);
    assert_eq!(big_demo::create(dir).unwrap(), Outcome::FilesWithoutDatabase);
    assert_eq!(snapshot(dir), before);
}

#[test]
fn a_half_built_staging_folder_is_rebuilt() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let staging = dir.join(big_demo::STAGING_DIR);
    std::fs::create_dir_all(staging.join("data")).unwrap();
    std::fs::write(staging.join("data/karbonfm.db"), b"not a database").unwrap();
    assert!(matches!(big_demo::create(dir).unwrap(), Outcome::Created { .. }));
    assert!(!staging.exists());
    let (conn, _) = db::open(&db::database_path(dir), Seed::Reference).unwrap();
    assert!(db::has_demo_data(&conn).unwrap());
}
