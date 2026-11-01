//! UI-17 Backup panel through the router with the full AP-13 guard and the setup gate on (OP-ADMIN-06, OP-ADMIN-07).
//! The shell's native folder dialog is replaced by a queue of answers, as a test double.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::backup;
use karbon_server::db::{self, Seed};
use karbon_server::session::Session;
use karbon_server::{router, AppState, FolderHook, FolderPick};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:4323";
const ORIGIN: &str = "http://127.0.0.1:4323";

type Answers = Arc<Mutex<VecDeque<(FolderPick, Option<PathBuf>)>>>;

struct App {
    router: axum::Router,
    cookie: String,
    answers: Answers,
    data: PathBuf,
    backups: PathBuf,
    _tmp: tempfile::TempDir,
}

async fn start(seed: Seed) -> App {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let backups = tmp.path().join("Backups");
    std::fs::create_dir_all(&backups).unwrap();
    let (conn, _) = db::open(&db::database_path(&data), seed).unwrap();
    let answers: Answers = Arc::default();
    let queue = answers.clone();
    let hook: FolderHook = Arc::new(move |what| {
        let (expected, answer) = queue.lock().unwrap().pop_front().expect("an unexpected folder dialog");
        assert_eq!(what, expected, "the dialog asks for the right folder");
        Ok(answer)
    });
    let session = Arc::new(Session::new(4323));
    let launch = session.launch_path().unwrap();
    let state =
        AppState::new(conn).with_origin(ORIGIN).with_session(session).with_folder_picker(hook).with_data_dir(&data);
    let router = router(state);
    let (_, headers, _) = send(&router, req("GET", &launch, None, None, "")).await;
    let cookie = headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
    App { router, cookie, answers, data, backups, _tmp: tmp }
}

fn req(method: &str, uri: &str, cookie: Option<&str>, origin: Option<&str>, body: &str) -> Request<Body> {
    let mut r = Request::builder().method(method).uri(uri).header(header::HOST, HOST).header("HX-Request", "true");
    if let Some(c) = cookie {
        r = r.header(header::COOKIE, c);
    }
    if let Some(o) = origin {
        r = r.header(header::ORIGIN, o);
    }
    if method == "POST" {
        r = r.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    r.body(Body::from(body.to_owned())).unwrap()
}

async fn send(router: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = router.clone().oneshot(req).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

impl App {
    fn answer(&self, what: FolderPick, folder: Option<PathBuf>) {
        self.answers.lock().unwrap().push_back((what, folder));
    }
    async fn post(&self, uri: &str, body: &str) -> (StatusCode, HeaderMap, String) {
        send(&self.router, req("POST", uri, Some(&self.cookie), Some(ORIGIN), body)).await
    }
    async fn page(&self, uri: &str) -> String {
        let mut r = Request::get(uri).header(header::HOST, HOST).header(header::COOKIE, &self.cookie);
        r = r.header("Accept", "text/html");
        send(&self.router, r.body(Body::empty()).unwrap()).await.2
    }
    fn only_backup(&self) -> PathBuf {
        let mut dirs: Vec<_> = std::fs::read_dir(&self.backups).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(dirs.len(), 1, "{dirs:?}");
        dirs.pop().unwrap()
    }
    fn counts(&self) -> std::collections::BTreeMap<String, i64> {
        backup::table_counts(&rusqlite::Connection::open(db::database_path(&self.data)).unwrap()).unwrap()
    }
}

fn pending_id(html: &str) -> String {
    let at = html.find(r#"name="pending" value=""#).expect("the confirm dialog") + r#"name="pending" value=""#.len();
    html[at..at + 36].to_owned()
}

#[tokio::test]
async fn the_panel_says_secrets_are_not_in_a_backup() {
    let app = start(Seed::Demo).await;
    let html = app.page("/settings").await;
    assert!(html.contains("Backup and restore") && html.contains(r#"id="backup-panel""#));
    assert!(html.contains("Passwords and API keys are not in a backup"));
    assert!(html.contains(r#"hx-post="/settings/backup""#) && html.contains(r#"hx-post="/settings/restore/check""#));
}

#[tokio::test]
async fn backup_then_restore_through_the_panel() {
    let app = start(Seed::Demo).await;
    let before = app.counts();

    // Back up: the dialog asks for a destination, the panel answers with the result.
    app.answer(FolderPick::BackupDestination, Some(app.backups.clone()));
    let (status, headers, html) = app.post("/settings/backup", "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("Backed up") && html.contains(r#"id="backup-panel""#), "{html}");
    assert!(headers["hx-trigger"].to_str().unwrap().contains("Backup saved"));
    let folder = app.only_backup();
    assert_eq!(backup::validate(&folder).unwrap().manifest.tables, before);

    // Cancelling the dialog changes nothing: 204, htmx swaps nothing.
    app.answer(FolderPick::BackupDestination, None);
    assert_eq!(app.post("/settings/backup", "").await.0, StatusCode::NO_CONTENT);
    app.answer(FolderPick::RestoreSource, None);
    assert_eq!(app.post("/settings/restore/check", "").await.0, StatusCode::NO_CONTENT);

    // Change the data, through the app.
    let body = format!("id={}&organization_name=Acme", karbon_server::ids::new_id());
    let (status, _, _) = app.post("/parties/organizations", &body).await;
    assert!(status == StatusCode::OK || status == StatusCode::SEE_OTHER, "{status}");
    {
        let conn = rusqlite::Connection::open(db::database_path(&app.data)).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF; DELETE FROM sales_line;").unwrap();
    }
    assert_ne!(app.counts(), before);

    // Restore, step 1: pick the backup; it is checked and the confirm dialog shows what is in it. Nothing changed yet.
    app.answer(FolderPick::RestoreSource, Some(folder.clone()));
    let (status, _, html) = app.post("/settings/restore/check", "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("Restore this backup?") && !html.contains("<dialog"), "a dialog body, not a page");
    assert!(!html.contains(folder.parent().unwrap().to_str().unwrap()), "the page gets no path, only the folder name");
    let id = pending_id(&html);
    assert_ne!(app.counts(), before, "checking does not restore");

    // A made-up id is refused.
    let (status, _, _) = app.post("/settings/restore", "pending=00000000-0000-4000-8000-000000000000").await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Step 2: the user confirms.
    let (status, headers, _) = app.post("/settings/restore", &format!("pending={id}")).await;
    assert_eq!(status, StatusCode::OK);
    let to = headers["hx-redirect"].to_str().unwrap().to_owned();
    assert!(to.starts_with("/settings?restored="), "{to}");
    assert_eq!(app.counts(), before, "identical row counts after the restore");

    // The id works once.
    let (status, _, _) = app.post("/settings/restore", &format!("pending={id}")).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // The app reads the restored database at once: the seed's money figures, and the notice with the safety copy.
    let customers = app.page("/customers").await;
    assert!(customers.contains("$29,619.05") && customers.contains("of 14"), "balance due of the demo seed");
    let settings = app.page(&to).await;
    assert!(settings.contains("Restored the backup made"), "{settings}");
    assert!(settings.contains("before-restore/KarbonFM backup "));
    let safety: Vec<_> = std::fs::read_dir(app.data.join(backup::SAFETY_DIR)).unwrap().collect();
    assert_eq!(safety.len(), 1, "one safety copy");
}

#[tokio::test]
async fn a_damaged_backup_is_refused_on_the_panel() {
    let app = start(Seed::Demo).await;
    app.answer(FolderPick::BackupDestination, Some(app.backups.clone()));
    app.post("/settings/backup", "").await;
    let folder = app.only_backup();
    let path = db::database_path(&folder);
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid..mid + 8192].fill(0x5A);
    std::fs::write(&path, bytes).unwrap();
    let before = app.counts();

    app.answer(FolderPick::RestoreSource, Some(folder));
    let (status, headers, html) = app.post("/settings/restore/check", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(headers["hx-retarget"], "#backup-panel");
    assert!(html.contains("damaged") && html.contains(r#"role="alert""#), "{html}");
    assert!(!html.contains(r#"name="pending""#), "no confirm dialog");
    assert_eq!(app.counts(), before);
    assert!(!app.data.join(backup::SAFETY_DIR).exists());

    // Any folder that is not a backup.
    app.answer(FolderPick::RestoreSource, Some(app.backups.clone()));
    let (status, _, html) = app.post("/settings/restore/check", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("not a KarbonFM backup"));
}

#[tokio::test]
async fn same_origin_posts_only_and_the_gates_apply() {
    let app = start(Seed::Demo).await;
    for uri in ["/settings/backup", "/settings/restore/check", "/settings/restore"] {
        // No cookie: 401. Foreign or missing Origin: 403. No dialog is shown (the queue is empty: it would panic).
        let (s, _, _) = send(&app.router, req("POST", uri, None, Some(ORIGIN), "")).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri}");
        let (s, _, _) = send(&app.router, req("POST", uri, Some(&app.cookie), Some("https://evil.example"), "")).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{uri}");
        let (s, _, _) = send(&app.router, req("POST", uri, Some(&app.cookie), None, "")).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{uri}");
        let (s, _, _) = send(&app.router, req("GET", uri, Some(&app.cookie), None, "")).await;
        assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED, "{uri}: POST only");
    }
    assert!(std::fs::read_dir(&app.backups).unwrap().next().is_none());

    // Before setup, the setup gate sends every backup route to /setup.
    let fresh = start(Seed::Reference).await;
    let (s, headers, _) = fresh.post("/settings/backup", "").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(headers["hx-redirect"], "/setup");
}

#[tokio::test]
async fn without_a_folder_dialog_the_panel_says_desktop_app() {
    // The dev server in a browser: no shell, so no dialog; nothing is written.
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let app = router(AppState::new(conn));
    let res = app.clone().oneshot(Request::get("/settings").body(Body::empty()).unwrap()).await.unwrap();
    let html = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(html.contains("available in the desktop app") && !html.contains(r#"hx-post="/settings/backup""#));
    let (status, _, html) = send(&app, req("POST", "/settings/backup", None, None, "")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("available in the desktop app"));
}
