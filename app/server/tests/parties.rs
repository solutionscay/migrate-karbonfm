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

// ---------------------------------------------------------------------------------------------------------------
// S3: create, edit, archive, roles (OP-PARTY-01..04, 10; T-08-07)
// ---------------------------------------------------------------------------------------------------------------

/// The hidden id of a create dialog.
fn form_id(html: &str) -> String {
    let at = html.find(r#"name="id" value=""#).expect("the dialog has an id") + r#"name="id" value=""#.len();
    html[at..at + 36].to_owned()
}

#[tokio::test]
async fn create_a_person_from_the_dialog() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/parties/people/new").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("New person") && html.contains(r#"hx-post="/parties/people""#));
    assert!(html.contains("<span class=\"text-[12px] muted\">First name</span>"), "labelled fields");
    let id = form_id(&html);
    // A field error re-renders the dialog with 422 and the message next to the field.
    let (status, _, html) =
        send(&app, form("POST", "/parties/people", &format!("id={id}&role=&first_name=&last_name=&email=nope"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter a first or last name.") && html.contains(r#"aria-invalid="true""#));
    assert!(html.contains("Enter an email address like name@example.com"));
    // The save sends the page to the new party.
    let body = format!("id={id}&role=lead&first_name=Ada&last_name=Quinn&email=ada%40example.com&phone=207+555+0199");
    let (status, headers, _) = send(&app, form("POST", "/parties/people", &body)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("hx-redirect").unwrap(), &format!("/parties/{id}"));
    let (_, html) = get(&app, &format!("/parties/{id}")).await;
    assert!(html.contains("Ada Quinn") && html.contains("Current lead") && html.contains("ada@example.com"));
    // The same form again (a double submit) opens the same party and creates nothing (BR-PARTY-10).
    let (_, headers, _) = send(&app, form("POST", "/parties/people", &body)).await;
    assert_eq!(headers.get("hx-redirect").unwrap(), &format!("/parties/{id}"));
    let (_, html) = htmx_get(&app, "/parties?q=ada+quinn").await;
    assert!(html.contains("1–1 of 1"));
    // An unknown field is refused (deny_unknown_fields).
    let (status, _, _) = send(&app, form("POST", "/parties/people", &format!("{body}&organization_name=x"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn new_customer_from_the_top_bar_creates_an_organization_customer() {
    let app = demo();
    let (_, html) = htmx_get(&app, "/parties/organizations/new?role=customer").await;
    assert!(html.contains("New customer") && html.contains(r#"name="role" value="customer""#));
    let id = form_id(&html);
    let (status, headers, _) = send(
        &app,
        form("POST", "/parties/organizations", &format!("id={id}&role=customer&organization_name=Pier+Nine+Ropes")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("hx-redirect").is_some());
    let (_, html) = get(&app, "/customers?q=pier").await;
    assert!(html.contains("Pier Nine Ropes"), "it is on the customer list");
    // A plain form post (no htmx) gets a 303.
    let req = Request::post("/parties/organizations")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("id={}&organization_name=Plain", karbon_server::ids::new_id())))
        .unwrap();
    let (status, headers, _) = send(&app, req).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(headers.get(header::LOCATION).unwrap().to_str().unwrap().starts_with("/parties/"));
}

#[tokio::test]
async fn edit_swaps_the_header_and_closes_the_dialog() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/parties/p-hp/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"hx-patch="/parties/p-hp""#) && html.contains(r#"value="Halden &#38; Pryce Architects""#));
    let (status, _, html) =
        send(&app, form("PATCH", "/parties/p-hp", "organization_name=&company_type=&federal_tax_id_number=&comments="))
            .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        html.contains("Enter the organization&#39;s name.") || html.contains("Enter the organization&#x27;s name."),
        "{html}"
    );
    let (status, headers, html) = send(
        &app,
        form(
            "PATCH",
            "/parties/p-hp",
            "organization_name=Halden+%26+Pryce&company_type=Architects&federal_tax_id_number=&comments=+hi+",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="party-header""#) && html.contains(r#"hx-swap-oob="true""#));
    assert!(html.contains("Halden &#38; Pryce</h1>"));
    let trigger = headers.get("hx-trigger").unwrap().to_str().unwrap();
    assert!(trigger.contains("karbon-close-dialog") && trigger.contains("Saved"));
    let (_, html) = get(&app, "/parties/p-hp").await;
    assert!(html.contains("Architects") && html.contains(">hi</dd>"));
}

#[tokio::test]
async fn archive_goes_back_to_the_list_without_the_party() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/parties/p-hl/archive").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Archive Harbor Lane Coffee?") && html.contains("1 sales transaction"));
    let (status, headers, _) = send(&app, form("POST", "/parties/p-hl/archive", "reason=Closed")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("hx-redirect").unwrap(), "/parties");
    let (_, html) = htmx_get(&app, "/parties?q=harbor").await;
    assert!(!html.contains("Harbor Lane Coffee"));
    let (_, html) = get(&app, "/parties/p-hl").await;
    assert!(html.contains("Archived on") && !html.contains(r#"hx-get="/parties/p-hl/edit""#), "no edit actions");
    let (status, _) = htmx_get(&app, "/parties/p-hl/edit").await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// T-08-07: a party cannot be deleted for good: there is no such route, and nothing changes.
#[tokio::test]
async fn t_08_07_no_hard_delete_of_a_party() {
    let app = demo();
    for (method, uri) in
        [("DELETE", "/parties/p-hp"), ("POST", "/parties/p-hp/delete"), ("DELETE", "/parties/p-hp/archive")]
    {
        let (status, _, _) = send(&app, form(method, uri, "confirm=true")).await;
        assert!(matches!(status, StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED), "{method} {uri}: {status}");
    }
    let (status, _, _) = send(&app, form("DELETE", "/parties/p-hp", "confirm=true")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, html) = get(&app, "/parties/p-hp").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("Archived"));
}

#[tokio::test]
async fn add_role_answers_with_the_header() {
    let app = demo();
    let (_, html) = get(&app, "/parties/p-tf").await;
    assert!(html.contains("Add role") && html.contains(r#"{"role": "lead"}"#));
    assert!(!html.contains(r#"{"role": "customer"}"#), "the customer role is held already");
    let (status, headers, html) = send(&app, form("POST", "/parties/p-tf/roles", "role=lead")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Current lead") && html.contains(r#"id="party-header""#));
    let trigger = headers.get("hx-trigger").unwrap().to_str().expect("an ASCII header");
    assert!(trigger.contains(r"Tom\u00e1s Ferreira is now a lead"), "{trigger}");
    let (status, _, _) = send(&app, form("POST", "/parties/p-tf/roles", "role=owner")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = send(&app, form("POST", "/parties/nobody/roles", "role=lead")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Before setup there is no user, so a write is sent to setup (the setup gate and the CurrentUser extractor).
#[tokio::test]
async fn writes_need_setup() {
    let (conn, _) = db::open_in_memory(db::Seed::Reference).unwrap();
    let app = router(AppState::new(conn));
    let (status, headers, _) = send(&app, form("POST", "/parties/people", "id=x&first_name=A")).await;
    assert!(status == StatusCode::OK || status.is_redirection(), "{status}");
    assert_eq!(headers.get("hx-redirect").map(|v| v.to_str().unwrap()), Some("/setup"));
}

// ---------------------------------------------------------------------------------------------------------------
// S4: contact methods (OP-CONTACT-01..04, AP-07)
// ---------------------------------------------------------------------------------------------------------------

/// The demo database with automatic US phone formatting on (BR-CONTACT-04).
fn demo_formatting() -> axum::Router {
    let (conn, _) = db::open_in_memory(db::Seed::Demo).unwrap();
    conn.execute("UPDATE system_setting SET automatically_format_phone_numbers_ind = 1, country_code = '1'", [])
        .unwrap();
    router(AppState::new(conn))
}

#[tokio::test]
async fn add_a_phone_from_the_dialog() {
    let app = demo_formatting();
    let (status, html) = htmx_get(&app, "/parties/p-tf/contact-methods/new?kind=phone").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Add phone") && html.contains(r#"hx-post="/parties/p-tf/contact-methods""#));
    let (status, _, html) = send(&app, form("POST", "/parties/p-tf/contact-methods", "kind=phone&number=")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter a phone number."));
    let (status, _, html) =
        send(&app, form("POST", "/parties/p-tf/contact-methods", "kind=phone&number=1&country_code=abc")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter a country code like 1 or 44."));
    let (status, headers, html) = send(
        &app,
        form(
            "POST",
            "/parties/p-tf/contact-methods",
            "kind=phone&number=207.555.0177&label=Mobile&billing=on&primary=on",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains(r#"hx-swap-oob="true""#) && html.contains("(207) 555-0177"));
    assert!(headers.get("hx-trigger").unwrap().to_str().unwrap().contains("karbon-close-dialog"));
    let (_, html) = htmx_get(&app, "/parties?q=ferreira").await;
    assert!(html.contains("(207) 555-0177"), "the list shows the new primary phone (BR-PARTY-06)");
}

#[tokio::test]
async fn edit_make_primary_and_toggle_from_the_header() {
    let app = demo_formatting();
    let (status, html) = htmx_get(&app, "/contact-methods/cm-001/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"hx-put="/contact-methods/cm-001""#) && html.contains("(207) 555-0100"));
    let (status, _, html) =
        send(&app, form("PUT", "/contact-methods/cm-001", "kind=phone&number=2075550109&label=Main")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("(207) 555-0109") && html.contains(">Main</span>"));
    // Toggling a purpose answers with the header; twice returns to the start (BR-CONTACT-02).
    let (status, headers, html) = send(&app, form("POST", "/contact-methods/cm-001/purposes/2", "")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(">Shipping</span>"));
    assert!(headers.get("hx-trigger").unwrap().to_str().unwrap().contains("Shipping set"));
    let (_, _, html) = send(&app, form("POST", "/contact-methods/cm-001/purposes/2", "")).await;
    assert!(!html.contains(">Shipping</span>"));
    let (status, _, _) = send(&app, form("POST", "/contact-methods/cm-001/purposes/9", "")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A second phone made primary takes the Primary mark and the cache.
    send(&app, form("POST", "/parties/p-ellison/contact-methods", "kind=phone&number=2075550188")).await;
    let (_, html) = get(&app, "/parties/p-ellison").await;
    // The new phone is the only method with a generated (36-character) id and a Make primary button.
    let second = html
        .split(r#"hx-post="/contact-methods/"#)
        .skip(1)
        .find(|s| s.len() > 44 && &s[36..44] == "/primary")
        .map(|s| s[..36].to_owned())
        .expect("the new phone has a Make primary button");
    let (status, _, html) = send(&app, form("POST", &format!("/contact-methods/{second}/primary"), "")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(html.matches(">Primary</span>").count(), 3, "one primary phone, one email, one address");
    let (_, html) = htmx_get(&app, "/parties?q=ellison+studio").await;
    assert!(html.contains("(207) 555-0188"));
}

#[tokio::test]
async fn delete_a_contact_method_asks_first() {
    let app = demo();
    let (status, _, html) = send(
        &app,
        Request::delete("/contact-methods/cm-001").header("HX-Request", "true").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"role="alertdialog""#), "AP-07: a confirmation dialog");
    assert!(html.contains("Deletes the phone (207) 555-0100 of Ellison Studio."));
    assert!(html.contains(r#"hx-delete="/contact-methods/cm-001?confirm=true""#));
    let (_, html) = get(&app, "/parties/p-ellison").await;
    assert!(html.contains("(207) 555-0100"), "nothing deleted yet");
    let req = Request::delete("/contact-methods/cm-001?confirm=true")
        .header("HX-Request", "true")
        .body(Body::empty())
        .unwrap();
    let (status, headers, html) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"hx-swap-oob="true""#) && !html.contains("(207) 555-0100"));
    assert!(headers.get("hx-trigger").unwrap().to_str().unwrap().contains("Deleted"));
    let req = Request::delete("/contact-methods/cm-001?confirm=true").body(Body::empty()).unwrap();
    assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn contact_methods_of_an_archived_party_are_read_only() {
    let (conn, _) = db::open_in_memory(db::Seed::Demo).unwrap();
    conn.execute("UPDATE party SET deleted_ind = 1 WHERE id = 'p-ellison'", []).unwrap();
    let app = router(AppState::new(conn));
    let (_, html) = get(&app, "/parties/p-ellison").await;
    assert!(!html.contains("Actions for") && !html.contains("Add contact method"));
    let (status, _, _) = send(&app, form("POST", "/contact-methods/cm-001/primary", "")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, html) = send(&app, form("POST", "/parties/p-ellison/contact-methods", "kind=phone&number=1")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("archived party"));
}
