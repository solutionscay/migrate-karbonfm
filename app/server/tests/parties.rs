//! I2 route tests: people and organizations (UI-03..06), contact methods, relations. The router runs in process on
//! the demo database (or a fresh one after setup) and the tests check status, HTML and htmx headers.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::{db, router, AppState};
use tower::ServiceExt;

fn demo() -> axum::Router {
    router(AppState::new(db::open_demo(None).expect("demo database")))
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app, Request::get(uri).body(Body::empty()).unwrap()).await;
    (s, b)
}

async fn htmx_get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app, Request::get(uri).header("HX-Request", "true").body(Body::empty()).unwrap()).await;
    (s, b)
}

#[allow(dead_code)]
fn form(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("HX-Request", "true")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

// ---------------------------------------------------------------------------------------------------------------
// S1: UI-03 list and search (OP-PARTY-05)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn party_list_is_a_page_with_every_party() {
    let app = demo();
    let (status, html) = get(&app, "/parties").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("People &amp; organizations</h1>"));
    assert!(html.contains("1–30 of 30"), "the 30 parties of the demo seed on one page of 50");
    assert!(html.contains(r#"href="/parties" aria-current="page""#), "the sidebar marks the page");
    assert!(html.contains(r#"aria-label="Search people and organizations""#));
    assert!(html.contains(r#"href="/parties/p-hp""#), "rows link to the detail");
}

#[tokio::test]
async fn party_search_filters_and_pages_as_a_fragment() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/parties?q=hal+pry").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("<html"), "a fragment for htmx");
    assert!(html.contains("Halden &#38; Pryce Architects") && html.contains("1–1 of 1"));
    let (_, html) = htmx_get(&app, "/parties?kind=org").await;
    assert!(html.contains("1–13 of 13"));
    let (_, html) = htmx_get(&app, "/parties?kind=person&role=employee").await;
    assert!(html.contains("Mara Ellison") && html.contains("1–1 of 1"));
    let (_, html) = htmx_get(&app, "/parties?q=hald&full=1").await;
    assert!(html.contains("No people or organizations match."), "whole words only");
    let (_, html) = htmx_get(&app, "/parties?q=%3Cscript%3E").await;
    assert!(!html.contains("<script>"), "the query is escaped");
    let (status, html) = htmx_get(&app, "/parties?page=99").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Page 1 of 1"), "an out-of-range page is clamped");
}

#[tokio::test]
async fn party_list_of_a_fresh_install_has_the_setup_parties() {
    let app = fresh();
    let (status, html) = get(&app, "/parties").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("Harbor &#38; Pine Design") && html.contains("Rosa Lindqvist"));
    let (_, html) = htmx_get(&app, "/parties?role=customer").await;
    assert!(html.contains("0 people or organizations"));
}

/// A fresh install after setup: the reference rows and the user's organization and person, no customers.
fn fresh() -> axum::Router {
    let (mut conn, _) = db::open_in_memory(db::Seed::Reference).unwrap();
    let input = karbon_server::admin::SetupInput {
        organization_name: "Harbor & Pine Design".into(),
        first_name: "Rosa".into(),
        last_name: "Lindqvist".into(),
        tax_rate_bp: 550,
        country_code: "1".into(),
        format_phone_numbers: true,
        account_name: "rosa".into(),
        ..Default::default()
    };
    karbon_server::admin::setup_welcome(&mut conn, &input).unwrap();
    router(AppState::new(conn))
}
