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
    let (_, html) = get(&app, "/customers").await;
    assert!(html.contains(r#"href="/parties/p-bm""#), "the customer view links to the same detail");
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

// ---------------------------------------------------------------------------------------------------------------
// S2: UI-04 party detail (read)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn party_detail_of_a_customer_organization() {
    let app = demo();
    let (status, html) = get(&app, "/parties/p-hp").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains(r#"<h1 class="page-title" id="party-name">Halden &#38; Pryce Architects</h1>"#));
    assert!(html.contains("Current customer"));
    assert!(html.contains("Organization · Architecture &#38; interiors · Customer since March 2021"));
    assert!(html.contains("$3,057.15") && html.contains("$1,500.00"), "balance and retainer tiles");
    // Contacts tab: the people of the relation pairs, as each one is to the organization.
    for (name, what) in
        [("Elena Halden", "Principal"), ("Marcus Pryce", "Partner"), ("Joanna Whitfield", "Office manager")]
    {
        assert!(html.contains(name) && html.contains(what), "{name}");
    }
    assert!(html.contains("Billing contact"));
    // Sales tab: the six documents of the design, linked to the invoice page.
    assert!(html.contains(r#"href="/sales/INV-1048""#) && html.contains("RET-0042"));
    assert!(html.contains("Partially paid") || html.contains("Overdue"));
    // Tabs are labelled for the keyboard and screen readers (D-38).
    assert!(html.contains(r#"role="tablist""#) && html.matches(r#"role="tab""#).count() == 4);
    assert_eq!(html.matches(r#"role="tabpanel""#).count(), 4);
}

#[tokio::test]
async fn party_detail_of_a_person_and_of_nobody() {
    let app = demo();
    let (status, html) = get(&app, "/parties/p-mara").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Mara Ellison") && html.contains("Current employee"));
    assert!(html.contains("Ellison Studio") && html.contains("Employer"), "the relation, seen from the person");
    assert!(html.contains("Not a customer: no sales transactions."));
    assert!(html.contains("mara@ellisonstudio.example"));
    let (status, _) = get(&app, "/parties/nobody").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn archived_party_opens_by_id_marked_archived() {
    let (conn, _) = db::open_in_memory(db::Seed::Demo).unwrap();
    conn.execute(
        "UPDATE party SET deleted_ind = 1, deleted_reason = 'Closed the business', deleted_timestamp = '2026-10-30 09:00:00'
         WHERE id = 'p-hl'",
        [],
    )
    .unwrap();
    let app = router(AppState::new(conn));
    let (status, html) = get(&app, "/parties/p-hl").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Archived on Oct 30, 2026: Closed the business."));
    assert!(html.contains("INV-1046"), "its sales stay");
    let (_, html) = htmx_get(&app, "/parties?q=harbor").await;
    assert!(!html.contains("Harbor Lane Coffee"), "hidden from the list");
}
