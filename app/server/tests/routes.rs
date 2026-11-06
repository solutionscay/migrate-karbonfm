//! Route tests: call the router in process (no socket) on the demo database and check the HTML.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::{db, router, AppState};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn app() -> axum::Router {
    router(AppState::new(db::open_demo(None).expect("demo database")))
}

async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

async fn get(uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app(), Request::get(uri).body(Body::empty()).unwrap()).await;
    (s, b)
}

async fn htmx_get(uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app(), Request::get(uri).header("HX-Request", "true").body(Body::empty()).unwrap()).await;
    (s, b)
}

#[tokio::test]
async fn customers_page_shows_the_seed_totals() {
    let (status, html) = get("/customers").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("$29,619.05"), "balance due of 04_demo_seed.sql");
    assert!(html.contains("$4,450.00"), "retainer balance of 04_demo_seed.sql");
    assert!(html.contains("1–8 of 14"));
    assert!(html.contains("Bluefin Marine Supply"));
}

#[tokio::test]
async fn customers_page_leaves_new_customer_to_the_header() {
    let (status, html) = get("/customers").await;
    assert_eq!(status, StatusCode::OK);
    // Only the top bar's New ▾ Customer: the wizard, never the dialog.
    assert_eq!(html.matches(r#"href="/parties/new?role=customer""#).count(), 1);
    assert!(!html.contains("/parties/organizations/new"), "no top-level button opens the dialog");
}

#[tokio::test]
async fn htmx_paging_returns_a_fragment() {
    let (status, html) = htmx_get("/customers?page=2").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("<html"), "a fragment, not a page");
    assert!(html.contains("9–14 of 14"));
    assert!(html.contains("Marigold Events Co."));
    assert!(!html.contains("Bluefin Marine Supply"), "page 1 rows are not on page 2");
}

#[tokio::test]
async fn filters_search_and_sort() {
    let (_, html) = htmx_get("/customers?status=inactive").await;
    assert!(html.contains("Marigold Events Co.") && html.contains("1–1 of 1"));
    let (_, html) = htmx_get("/customers?status=retainer").await;
    assert!(html.contains("1–4 of 4"));
    let (_, html) = htmx_get("/customers?q=HALDEN").await;
    assert!(html.contains("Halden &#38; Pryce Architects") && html.contains("1–1 of 1"));
    let (_, html) = htmx_get("/customers?q=%3Cscript%3E").await;
    assert!(html.contains("No customers match.") && !html.contains("<script>"), "the query is escaped");
    let (_, html) = htmx_get("/customers?sort=name").await;
    let atlas = html.find("Atlas Freight Partners").unwrap();
    let bluefin = html.find("Bluefin Marine Supply").unwrap();
    assert!(atlas < bluefin);
    let (status, html) = htmx_get("/customers?page=99").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Page 2 of 2"), "an out-of-range page is clamped");
}

#[tokio::test]
async fn dashboard_has_the_chart_data() {
    let (status, html) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("data-revenue-chart="));
    assert!(html.contains("/assets/vendor/chart-4.4.7.umd.min.js"));
    assert!(html.contains("Good day, Mara"));
}

#[tokio::test]
async fn an_unknown_page_says_so_in_the_app_layout() {
    // B13: a path no route serves answered 404 with an empty body, a blank white window.
    let (status, html) = get("/email").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("Page not found") && html.contains(r#"href="/""#), "{html}");
    assert!(html.contains("/assets/app.css"), "the app layout, with the sidebar");
}

#[tokio::test]
async fn invoice_lives_at_sales() {
    // S-01's /invoices/{doc} is replaced by /sales/{doc} (S-02; totals tested in tests/invoice_pdf.rs).
    let (status, _) = get("/invoices/INV-1048").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, html) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"href="/sales""#), "the nav links to the sales list (UI-13)");
    assert!(!html.contains("/invoices/"));
}

#[tokio::test]
async fn print_uses_the_window_hook_when_there_is_one() {
    let (status, _, _) = send(app(), Request::post("/print").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "no window: the page falls back to window.print()");

    let calls = Arc::new(Mutex::new(0));
    let counter = calls.clone();
    let state = AppState::new(db::open_demo(None).unwrap()).with_print(Arc::new(move || {
        *counter.lock().unwrap() += 1;
        Ok(())
    }));
    let (status, _, _) = send(router(state), Request::post("/print").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn assets_are_embedded_and_pages_carry_a_csp() {
    for (path, ctype) in [
        ("/assets/app.css", "text/css"),
        ("/assets/vendor/htmx-2.0.4.min.js", "text/javascript"),
        ("/assets/vendor/alpine-3.14.8.min.js", "text/javascript"),
        ("/assets/vendor/fonts/Geist-Variable.woff2", "font/woff2"),
    ] {
        let (status, headers, _) = send(app(), Request::get(path).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(headers[header::CONTENT_TYPE].to_str().unwrap().starts_with(ctype), "{path}");
    }
    let (status, _) = get("/assets/../Cargo.toml").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, headers, _) = send(app(), Request::get("/").body(Body::empty()).unwrap()).await;
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.contains("default-src 'self'") && !csp.contains("http"));
}

#[test]
fn built_css_has_the_daisyui_components_and_karbon_themes() {
    let css = include_str!("../assets/app.css");
    for needle in [".modal-box", "karbon-light", "karbon-dark", ".modal", ".btn", ".join", "@media print"] {
        assert!(css.contains(needle), "app.css is missing {needle}; run tools/build-css.sh");
    }
}

#[tokio::test]
async fn origin_guard_refuses_other_hosts_and_cross_site_writes() {
    const ORIGIN: &str = "http://127.0.0.1:4321";
    let guarded = || router(AppState::new(db::open_demo(None).unwrap()).with_origin(ORIGIN));
    let req = |method: &str, host: &str, origin: Option<&str>| {
        let mut r = Request::builder().method(method).uri("/print").header(header::HOST, host);
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        r.body(Body::empty()).unwrap()
    };

    let (status, _, _) =
        send(guarded(), Request::get("/").header(header::HOST, "127.0.0.1:4321").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) =
        send(guarded(), Request::get("/").header(header::HOST, "evil.example:4321").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST, "DNS rebinding");
    let (status, _, _) = send(guarded(), req("POST", "127.0.0.1:4321", Some("https://evil.example"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "cross-site POST");
    let (status, _, _) = send(guarded(), req("POST", "127.0.0.1:4321", None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "POST without Origin");
    let (status, _, _) = send(guarded(), req("POST", "127.0.0.1:4321", Some(ORIGIN))).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "same-origin POST reaches the handler");
}

/// A fresh install after setup: the reference rows and the user's organization and person, no customers.
fn fresh_app() -> axum::Router {
    let (mut conn, _) = db::open_in_memory(db::Seed::Reference).unwrap();
    let input = karbon_server::admin::SetupInput {
        organization_name: "Harbor & Pine Design".into(),
        first_name: "Rosa".into(),
        last_name: "Lindqvist".into(),
        tax_rate_bp: 550,
        country_code: "1".into(),
        account_name: "rosa".into(),
        ..Default::default()
    };
    karbon_server::admin::setup_welcome(&mut conn, &input).unwrap();
    router(AppState::new(conn))
}

#[tokio::test]
async fn pages_of_an_empty_database_render_zero_totals() {
    let (status, _, html) = send(fresh_app(), Request::get("/customers").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("0 customers"));
    assert!(html.contains("$0.00"));
    let (status, _, html) = send(fresh_app(), Request::get("/").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let (status, _, _) = send(fresh_app(), Request::get("/sales/INV-1048").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no such invoice yet");
}
