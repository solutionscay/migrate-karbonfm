//! The guard and the outbound policy against the real app log file (OP-ADMIN-05, AP-02, AP-03, AP-13). Its own test
//! binary, because the app logger is set once per process.
//!
//! - An outbound refusal is one warn line, code Integration, with the host and without the URL's query.
//! - A request the guard refuses (401, 403, 421) is not an error of the app: it writes no log line, so the log never
//!   holds a launch token or a cookie value, and a scan of the port cannot fill it.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use karbon_server::db::{self, Seed};
use karbon_server::net::OutboundPolicy;
use karbon_server::session::Session;
use karbon_server::{applog, router, AppState};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:4321";
const ORIGIN: &str = "http://127.0.0.1:4321";

fn lines(dir: &std::path::Path) -> Vec<Value> {
    let text = std::fs::read_to_string(dir.join(applog::FILE_NAME)).unwrap_or_default();
    text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[tokio::test]
async fn refusals_are_logged_right_and_guard_denials_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    applog::init(dir.path()).unwrap();

    // AP-02: one warn line per refused outbound call, host only.
    let policy = OutboundPolicy::deny_all();
    assert!(policy.check_https("https://api.example.com/v1/lookup?key=SECRET-KEY-42").is_err());
    let log = lines(dir.path());
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0]["level"], "warn");
    assert_eq!(log[0]["op"], "outbound https");
    assert_eq!(log[0]["code"], "Integration");
    assert_eq!(log[0]["message"], "outbound call refused: api.example.com is not enabled");
    assert!(policy.check_smtp("smtp.example.com").is_err());
    assert_eq!(lines(dir.path()).len(), 2);

    // AP-13 on the migrated database: refused requests answer 4xx and write nothing.
    let session = Arc::new(Session::new(4321));
    let launch = session.launch_path().unwrap();
    let token = launch.strip_prefix("/session/").unwrap().to_owned();
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let app = router(AppState::new(conn).with_origin(ORIGIN).with_session(session));
    let send = |req: Request<Body>| {
        let app = app.clone();
        async move { app.oneshot(req).await.unwrap() }
    };
    let get = |uri: &str, host: &str, cookie: Option<&str>| {
        let mut r = Request::get(uri).header(header::HOST, host);
        if let Some(c) = cookie {
            r = r.header(header::COOKIE, c);
        }
        r.body(Body::empty()).unwrap()
    };

    let res = send(get(&launch, HOST, None)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let cookie = res.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
    let cookie_value = cookie.split('=').nth(1).unwrap().to_owned();

    assert_eq!(send(get("/", HOST, None)).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(send(get("/customers", HOST, Some("karbon_session_4321=bad"))).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(send(get(&launch, HOST, None)).await.status(), StatusCode::FORBIDDEN, "spent token");
    assert_eq!(send(get("/", "evil.example", Some(&cookie))).await.status(), StatusCode::MISDIRECTED_REQUEST);
    let post = Request::post("/sales/INV-1048/save-pdf")
        .header(header::HOST, HOST)
        .header(header::COOKIE, &cookie)
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(post).await.status(), StatusCode::FORBIDDEN);
    // The window's own requests still work on the migrated database.
    assert_eq!(send(get("/customers", HOST, Some(&cookie))).await.status(), StatusCode::OK);

    let log = lines(dir.path());
    assert_eq!(log.len(), 2, "guard denials wrote log lines: {log:?}");
    let text = std::fs::read_to_string(dir.path().join(applog::FILE_NAME)).unwrap();
    for secret in [token.as_str(), cookie_value.as_str(), "SECRET-KEY-42"] {
        assert!(!text.contains(secret), "the log holds a secret: {text}");
    }
}
