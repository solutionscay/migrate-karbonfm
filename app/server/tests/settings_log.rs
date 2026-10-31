//! T-08-03, log half: a settings save that fails writes one log line (BR-ADMIN-03), and neither that line nor any
//! other has the SMTP password that was typed. Its own test binary, because the app logger is set once per process.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::db::{self, Seed};
use karbon_server::secrets::MemoryStore;
use karbon_server::session::Session;
use karbon_server::{applog, router, AppState};
use std::sync::Arc;
use tower::ServiceExt;

const PASSWORD: &str = "n3ver-in-the-log-5521";

#[tokio::test]
async fn a_failed_settings_save_logs_the_cause_but_never_the_password() {
    let logs = tempfile::tempdir().unwrap();
    applog::init(logs.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = db::database_path(dir.path());
    let (conn, _) = db::open(&path, Seed::Demo).unwrap();
    // The settings row cannot be written: the save fails after the form checks.
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER no_save BEFORE UPDATE ON system_setting BEGIN SELECT RAISE(ABORT, 'locked'); END;",
        )
        .unwrap();

    let session = Arc::new(Session::new(4323));
    let launch = session.launch_path().unwrap();
    let state = AppState::new(conn)
        .with_origin("http://127.0.0.1:4323")
        .with_session(session)
        .with_secrets(Arc::new(MemoryStore::default()));
    let app = router(state);
    let res = app
        .clone()
        .oneshot(Request::get(&launch).header(header::HOST, "127.0.0.1:4323").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookie = res.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();

    let form = format!(
        "owner_id=p-ellison&tax_rate=5.5&country_code=1&area_code=207&smtp_encryption=STARTTLS&smtp_authentication=Password\
         &email_user_name=billing&system_smtp_password={PASSWORD}"
    );
    let req = Request::put("/settings")
        .header(header::HOST, "127.0.0.1:4323")
        .header(header::ORIGIN, "http://127.0.0.1:4323")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("HX-Request", "true")
        .body(Body::from(form))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(!body.contains(PASSWORD) && !body.contains("locked"), "{body}");

    let text = std::fs::read_to_string(logs.path().join(applog::FILE_NAME)).unwrap();
    assert!(!text.contains(PASSWORD), "{text}");
    let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let line = lines.iter().find(|l| l["op"] == "PUT /settings").expect("one line for the failed save");
    assert_eq!(line["code"], "Internal");
    assert!(line["message"].as_str().unwrap().contains("locked"), "{line}");
}
