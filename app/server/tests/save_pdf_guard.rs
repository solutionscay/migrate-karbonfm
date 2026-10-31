//! fix/pdf-view's `POST /sales/{doc_number}/save-pdf` under the I1 gates: the AP-13 guard (session cookie, Host,
//! Origin) and the setup gate (BR-ADMIN-01). A refused request never reaches the shell's save hook (no save dialog).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use karbon_server::db::{self, Seed};
use karbon_server::session::Session;
use karbon_server::{router, AppState, SaveHook};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:4324";
const ORIGIN: &str = "http://127.0.0.1:4324";
const URI: &str = "/sales/INV-1048/save-pdf";

/// A guarded router with a counting save hook, past the launch URL; returns the cookie.
async fn app(seed: Seed) -> (axum::Router, String, Arc<AtomicUsize>) {
    let (conn, _) = db::open_in_memory(seed).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let save: SaveHook = Arc::new(move |_, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(Some(PathBuf::from("/tmp/INV-1048.pdf")))
    });
    let session = Arc::new(Session::new(4324));
    let launch = session.launch_path().unwrap();
    let router = router(AppState::new(conn).with_origin(ORIGIN).with_session(session).with_save(save));
    let res = router.clone().oneshot(post_or_get("GET", &launch, None, None)).await.unwrap();
    let cookie = res.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
    (router, cookie, calls)
}

fn post_or_get(method: &str, uri: &str, cookie: Option<&str>, origin: Option<&str>) -> Request<Body> {
    let mut r = Request::builder().method(method).uri(uri).header(header::HOST, HOST).header("HX-Request", "true");
    if let Some(c) = cookie {
        r = r.header(header::COOKIE, c);
    }
    if let Some(o) = origin {
        r = r.header(header::ORIGIN, o);
    }
    r.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn the_guard_covers_save_pdf() {
    let (app, cookie, calls) = app(Seed::Demo).await;
    let status = |req: Request<Body>| {
        let app = app.clone();
        async move { app.oneshot(req).await.unwrap().status() }
    };
    assert_eq!(status(post_or_get("POST", URI, None, Some(ORIGIN))).await, StatusCode::UNAUTHORIZED, "no cookie");
    assert_eq!(
        status(post_or_get("POST", URI, Some("karbon_session_4324=bad"), Some(ORIGIN))).await,
        StatusCode::UNAUTHORIZED,
        "wrong cookie"
    );
    assert_eq!(
        status(post_or_get("POST", URI, Some(&cookie), Some("https://evil.example"))).await,
        StatusCode::FORBIDDEN,
        "foreign Origin"
    );
    assert_eq!(status(post_or_get("POST", URI, Some(&cookie), None)).await, StatusCode::FORBIDDEN, "no Origin");
    let rebind = Request::post(URI)
        .header(header::HOST, "evil.example:4324")
        .header(header::COOKIE, &cookie)
        .header(header::ORIGIN, ORIGIN)
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(rebind).await, StatusCode::MISDIRECTED_REQUEST, "foreign Host");
    assert_eq!(status(post_or_get("GET", URI, Some(&cookie), None)).await, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "no refused request opened the save dialog");

    // The window's own request reaches the hook once.
    assert!(status(post_or_get("POST", URI, Some(&cookie), Some(ORIGIN))).await.is_success());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_setup_gate_covers_save_pdf() {
    // A fresh install (no setup yet): every route but /setup goes to /setup, save-pdf included.
    let (app, cookie, calls) = app(Seed::Reference).await;
    let res = app.oneshot(post_or_get("POST", URI, Some(&cookie), Some(ORIGIN))).await.unwrap();
    assert_eq!(res.headers()["hx-redirect"], "/setup");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
