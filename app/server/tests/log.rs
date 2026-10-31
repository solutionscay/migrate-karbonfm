//! OP-ADMIN-05 through the router: a failing route answers with a generic message and writes one log line with the
//! route, the error code and the cause to the app log file (BR-ADMIN-03, D-25, D-30). Its own test binary, because
//! the app logger is set once per process.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::db::{self, Seed};
use karbon_server::{applog, router, AppState};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn a_failing_route_writes_a_log_line_and_shows_a_generic_message() {
    let dir = tempfile::tempdir().unwrap();
    applog::init(dir.path()).unwrap();

    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    conn.execute_batch("DROP VIEW sales_summary_by_month;").unwrap();
    let res = router(AppState::new(conn)).oneshot(Request::get("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert_eq!(body, "Something went wrong");

    let text = std::fs::read_to_string(dir.path().join(applog::FILE_NAME)).unwrap();
    let line: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(line["level"], "error");
    assert_eq!(line["op"], "GET /");
    assert_eq!(line["code"], "Internal");
    assert!(line["message"].as_str().unwrap().contains("sales_summary_by_month"), "{line}");
    assert!(karbon_server::audit::is_timestamp(line["ts"].as_str().unwrap()));

    // log_write masks secrets on the way to the file (AP-03).
    let payload = serde_json::json!({ "smtp_user_name": "mara", "smtp_password": "hunter2" });
    applog::log_write(&applog::Entry::error("POST /settings", "Validation", "api_key=k-123").with_payload(&payload));
    let text = std::fs::read_to_string(dir.path().join(applog::FILE_NAME)).unwrap();
    assert!(!text.contains("hunter2") && !text.contains("k-123"), "{text}");
    assert!(text.contains("\"smtp_user_name\":\"mara\""));
}
