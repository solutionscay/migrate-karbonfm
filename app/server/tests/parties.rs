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

// ---------------------------------------------------------------------------------------------------------------
// S5: relations (OP-PARTY-07..09), wizard (OP-PARTY-11, UI-05), pickers (UI-06, OP-PARTY-06)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn relate_two_parties_from_the_contacts_tab() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/parties/p-bm/relations/new").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Relate to Bluefin Marine Supply") && html.contains(r#"role="radiogroup""#));
    assert!(!html.contains(r#"name="other_id" value="p-bm""#), "the party itself is not offered");
    assert!(html.contains(r#"hx-get="/pickers/contact?exclude=p-bm""#), "and not while searching");
    // The picker narrows as the user types.
    let (_, html) = htmx_get(&app, "/pickers/contact?q=ferr&exclude=p-bm").await;
    assert!(html.contains("Tomás Ferreira") && html.contains(r#"name="other_id" value="p-tf""#));
    let (status, _, html) = send(&app, form("POST", "/parties/p-bm/relations", "q=&relationship=Consultant")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Choose the related person or organization."));
    let body = "q=ferr&other_id=p-tf&relationship=Consultant&reciprocal=Client&billing=on";
    let (status, headers, html) = send(&app, form("POST", "/parties/p-bm/relations", body)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(
        html.contains(r#"id="party-relations" hx-swap-oob="true""#) && html.contains("Consultant · Billing contact")
    );
    assert!(headers.get("hx-trigger").unwrap().to_str().unwrap().contains("karbon-close-dialog"));
    let (_, html) = get(&app, "/parties/p-tf").await;
    assert!(
        html.contains("Bluefin Marine Supply") && html.contains("Client"),
        "the reciprocal row, seen from the person"
    );
    // The same pair again is refused, with the reason in the dialog.
    let (status, _, html) = send(&app, form("POST", "/parties/p-bm/relations", body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("related already"));
}

#[tokio::test]
async fn edit_end_restore_and_delete_a_relation() {
    let app = demo();
    // rel-p-hp-jw is Joanna Whitfield -> Halden & Pryce (the row the organization's Contacts tab shows).
    let (status, html) = htmx_get(&app, "/relations/rel-p-hp-jw/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Relation with Joanna Whitfield") && html.contains(r#"value="Office manager""#));
    let (status, _, html) = send(
        &app,
        form("PATCH", "/relations/rel-p-hp-jw", "relationship=Studio+manager&reciprocal=Employer&billing=on"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Studio manager · Billing contact"));
    let (_, _, html) = send(&app, form("PATCH", "/relations/rel-p-hp-jw", "change=end")).await;
    assert!(html.contains("· Ended") && html.contains(r#"{"change": "restore"}"#));
    let (_, _, html) = send(&app, form("PATCH", "/relations/rel-p-hp-jw", "change=restore")).await;
    assert!(!html.contains("· Ended"));
    let req = Request::delete("/relations/rel-p-hp-jw").header("HX-Request", "true").body(Body::empty()).unwrap();
    let (status, _, html) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains(r#"role="alertdialog""#) && html.contains("Joanna Whitfield and Halden &#38; Pryce Architects")
    );
    let req = Request::delete("/relations/rel-p-hp-jw?confirm=true")
        .header("HX-Request", "true")
        .body(Body::empty())
        .unwrap();
    let (status, _, html) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("Joanna Whitfield"), "both rows are gone from the organization's tab");
    let (_, html) = get(&app, "/parties/p-hp-jw").await;
    assert!(html.contains("No related people or organizations yet."), "and from the person's");
}

#[tokio::test]
async fn wizard_creates_an_organization_customer_and_its_contact() {
    let app = demo();
    let (status, html) = get(&app, "/parties/new").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("New contact") && html.contains(r#"aria-current="step""#));
    let org_id = {
        let at = html.find(r#"name="org_id" value=""#).unwrap() + r#"name="org_id" value=""#.len();
        html[at..at + 36].to_owned()
    };
    let (status, _, html) = send(&app, form("POST", "/parties/wizard", &format!("step=names&org_id={org_id}"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter an organization, a person, or both."));
    // Names that look like existing parties show them first (the duplicate check of BR-PARTY-02).
    let names =
        format!("step=names&org_id={org_id}&person_id=&org_name=Halden&person_first_name=Elena&person_last_name=Hal");
    let (status, _, html) = send(&app, form("POST", "/parties/wizard", &names)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(
        html.contains("These may be the same")
            && html.contains(r#"href="/parties/p-hp""#)
            && html.contains(r#"href="/parties/p-hp-eh""#)
    );
    assert!(html.contains(r#"value="employee" class="radio radio-xs" checked"#));
    // Back keeps the values.
    let (_, _, html) = send(
        &app,
        form("POST", "/parties/wizard", &format!("{}&go=back", names.replace("step=names", "step=details"))),
    )
    .await;
    assert!(html.contains(r#"value="Halden""#) && html.contains("1. Names"));
    // Save: one transaction for the organization, the person, their roles and the pair.
    let save = format!(
        "step=details&org_id={org_id}&person_id={}&org_name=Tidewater+Kayak&person_first_name=Nell&person_last_name=Barrow\
         &org_email=hello%40tidewater.example&org_phone=&org_role=customer&person_email=&person_phone=&person_job_title=Owner\
         &person_role=employee&relationship=Owner&billing_contact=on",
        karbon_server::ids::new_id()
    );
    let (status, headers, html) = send(&app, form("POST", "/parties/wizard", &save)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert_eq!(headers.get("hx-redirect").unwrap(), &format!("/parties/{org_id}"));
    assert!(headers.get("hx-trigger").unwrap().to_str().unwrap().contains(r#""karbon-toast":"Created""#));
    let (_, html) = get(&app, &format!("/parties/{org_id}")).await;
    assert!(html.contains("Tidewater Kayak") && html.contains("Current customer") && html.contains("Nell Barrow"));
    assert!(html.contains("Owner · Billing contact"));
    // A new customer is taxable, as in the source (kontroller 229 step 154); Ep03 saw "Not taxable".
    assert!(html.contains(r#"<span class="chip">Taxable</span>"#) && !html.contains("Not taxable"));
    // A resubmitted save opens the same party and creates nothing.
    let (_, headers, _) = send(&app, form("POST", "/parties/wizard", &save)).await;
    assert_eq!(headers.get("hx-redirect").unwrap(), &format!("/parties/{org_id}"));
    let (_, html) = htmx_get(&app, "/parties?q=tidewater").await;
    assert!(html.contains("1–1 of 1"));
}

#[tokio::test]
async fn pickers_and_find_by_contact() {
    let app = demo();
    let (status, html) = htmx_get(&app, "/pickers/email?party_id=p-hp-eh").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("elena@haldenpryce.example") && html.contains(r#"name="email_id""#));
    let (_, html) = htmx_get(&app, "/pickers/phone?party_id=p-hp-mp").await;
    assert!(html.contains("(207) 555-0163"));
    let (_, html) = htmx_get(&app, "/pickers/phone?party_id=p-bm-c").await;
    assert!(html.contains("Owen Castellano has no phone number."));
    let (status, _) = htmx_get(&app, "/pickers/fax?party_id=p-hp").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, html) = htmx_get(&app, "/parties/by-contact?email=elena%40haldenpryce.example").await;
    assert_eq!(status, StatusCode::OK);
    let org = html.find("Halden &#38; Pryce Architects").expect("the employer");
    let person = html.find("Elena Halden").expect("the owner");
    assert!(org < person, "the organization first (BR-PARTY-11)");
    let (_, html) = htmx_get(&app, "/parties/by-contact?email=nobody%40x.example").await;
    assert!(html.contains("Nobody has that email or phone."));
}

// ---------------------------------------------------------------------------------------------------------------
// S6: UI-13 sales and payments lists (read only)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn sales_list_shows_every_document_with_its_status() {
    let app = demo();
    let (status, html) = get(&app, "/sales").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Invoices &amp; receipts</h1>"));
    assert!(html.contains(r#"href="/sales" aria-current="page""#), "the sidebar links the list now");
    assert!(
        html.contains("1–16 of 16") && html.contains("$29,619.05 due"),
        "the posted open balance, as on /customers"
    );
    assert!(html.contains(r#"href="/sales/INV-1048""#) && html.contains(r#"href="/parties/p-hp""#));
    let (_, html) = htmx_get(&app, "/sales?status=paid").await;
    assert!(!html.contains("<html") && html.contains("INV-1027") && !html.contains("INV-1051"));
    let (_, html) = htmx_get(&app, "/sales?kind=receipt").await;
    assert!(html.contains("1–5 of 5"));
    let (_, html) = htmx_get(&app, "/sales?q=bluefin").await;
    assert!(html.contains("INV-1051") && html.contains("1–1 of 1"));
    let (_, html) = htmx_get(&app, "/sales?q=%3Cb%3E").await;
    assert!(html.contains("No sales transactions match.") && !html.contains("<b>"));
}

#[tokio::test]
async fn payments_list_shows_what_each_payment_pays() {
    let app = demo();
    let (status, html) = get(&app, "/payments").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Payments</h1>") && html.contains(r#"href="/payments" aria-current="page""#));
    assert!(html.contains("1–4 of 4"));
    assert!(html.contains(r#"href="/sales/INV-10"#), "each payment links the documents it pays");
    let (_, html) = htmx_get(&app, "/payments?q=nobody").await;
    assert!(html.contains("No payments match."));
}

#[tokio::test]
async fn sales_lists_of_a_fresh_install_are_empty() {
    let app = fresh();
    let (status, html) = get(&app, "/sales").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("0 sales transactions"));
    let (status, html) = get(&app, "/payments").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("0 payments"));
}

/// Ep03: the "Created" toast of the wizard (and "Archived") never showed, because htmx loads the page of HX-Redirect
/// right after the HX-Trigger events. app.js keeps such a toast for the next page. The page has no JS test runner, so
/// this checks that the handler is served; the headless check is in the Ep03 bug-check report.
#[tokio::test]
async fn a_toast_sent_with_a_redirect_waits_for_the_next_page() {
    let app = demo();
    let (status, js) = get(&app, "/assets/app.js").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        js.contains(r#"addEventListener("htmx:beforeOnLoad""#) && js.contains(r#"getResponseHeader("HX-Redirect")"#)
    );
    assert!(js.contains(r#"sessionStorage.setItem("karbon-toast-next""#));
    assert!(js.contains(r#"sessionStorage.getItem("karbon-toast-next")"#));
}

/// Ep03: at 1440 px the header cut the email and the address ("studio…", "48 Wh…") because four action buttons took
/// the width. As design/customer.html: Edit, an Add menu and a More menu; a long contact method wraps, never cut.
#[tokio::test]
async fn the_party_header_keeps_contact_methods_whole() {
    let app = demo();
    let (_, html) = get(&app, "/parties/p-hp").await;
    let header = &html[html.find(r#"id="party-header""#).unwrap()..];
    let header = &header[..header.find("</section>").unwrap()];
    assert!(!header.contains("truncate"), "no contact method is cut");
    assert!(header.contains("studio@haldenpryce.example") && header.contains("48 Wharf Street, Suite 3"));
    let actions = &header[header.find(r#"aria-label="Actions""#).unwrap()..];
    assert_eq!(actions.matches("<summary").count(), 2, "Add and More actions");
    assert!(actions.contains(">Add</summary>") && actions.contains(r#"aria-label="More actions""#));
    assert!(actions.contains("Add contact method") && actions.contains("Add role") && actions.contains("Archive…"));
}

/// Item 4 of the Ep03 check: one word for one state. A role with current_ind = 0 reads "Inactive customer" on the
/// party page, as the /customers chip, KPI and filter (design/customers.html:175, 192, 333); it said "Former".
#[tokio::test]
async fn an_inactive_role_reads_inactive_on_the_page_and_in_the_list() {
    let app = demo();
    // Marigold Events Co.: customer role with current_ind 0 (04_demo_seed.sql:263).
    let (_, html) = get(&app, "/parties/p-me").await;
    let header = &html[html.find(r#"id="party-header""#).unwrap()..];
    let header = &header[..header.find("</section>").unwrap()];
    assert!(header.contains(r#"<span class="chip">Inactive customer</span>"#), "{header}");
    assert!(!html.contains("Former"));
    let (_, html) = htmx_get(&app, "/customers?status=inactive").await;
    assert!(html.contains("Marigold Events Co.") && html.contains("Inactive</span>"));
    // A current role keeps its chip.
    let (_, html) = get(&app, "/parties/p-hp").await;
    assert!(html.contains("Current customer") && !html.contains("Inactive customer"));
}

/// Item 5 of the Ep03 check: the top bar is the design's New ▾ menu (design/partials/topbar.html:13-19). Customer
/// opens the wizard with Customer preselected; the items of later releases are buttons with the "not part of the demo
/// yet" toast, not links.
#[tokio::test]
async fn the_top_bar_new_menu_opens_the_wizard_for_a_customer() {
    let app = demo();
    let (_, html) = get(&app, "/parties/p-hp").await;
    let topbar = &html[html.find(r#"<header class="topbar">"#).unwrap()..];
    let topbar = &topbar[..topbar.find("</header>").unwrap()];
    assert!(topbar.contains(r#"<details class="dropdown dropdown-end">"#) && topbar.contains("</svg>New<svg"));
    let menu = &topbar[topbar.find(r#"aria-label="New""#).unwrap()..topbar.find("</ul>").unwrap()];
    assert_eq!(menu.matches("<li>").count(), 7, "Invoice, Sales receipt, Customer, Contact, Task, Note, Template");
    assert_eq!(menu.matches("<a ").count(), 2, "two real links, no fake ones");
    assert!(menu.contains(r#"<a href="/parties/new?role=customer">"#));
    assert!(menu.contains(r#"<a href="/parties/new">"#));
    for soon in ["New invoice", "New sales receipt"] {
        assert!(menu.contains(&format!(r#"<button type="button" class="muted" data-soon="{soon}">"#)), "{soon}");
    }
    // I4: Task and Note open the activity dialog.
    for kind in ["task", "note"] {
        assert!(menu.contains(&format!(r##"hx-get="/activities/new?kind={kind}" hx-target="#modal-body""##)), "{kind}");
    }
    assert_eq!(topbar.matches(r#"hx-get="/activities/new"#).count(), 2, "only Task and Note open a dialog");
}

#[tokio::test]
async fn the_wizard_role_preset() {
    let app = demo();
    let org_role = |html: &str| {
        let at = html.find(r#"name="org_role" value=""#).unwrap() + r#"name="org_role" value=""#.len();
        html[at..at + html[at..].find('"').unwrap()].to_owned()
    };
    let (status, html) = get(&app, "/parties/new?role=customer").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("<title>New customer · KarbonFM</title>")
            && html.contains(r#"<h1 class="page-title mb-5">New customer</h1>"#)
    );
    assert_eq!(org_role(&html), "customer");
    for (role, want, title) in
        [("lead", "lead", "New contact"), ("none", "", "New contact"), ("bogus", "customer", "New contact")]
    {
        let (status, html) = get(&app, &format!("/parties/new?role={role}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(org_role(&html), want, "{role}");
        assert!(html.contains(&format!("<h1 class=\"page-title mb-5\">{title}</h1>")), "{role}");
    }
    // The preset rides along to the details step, where Customer is checked.
    let (_, html) = get(&app, "/parties/new?role=customer").await;
    let id = |name: &str| {
        let key = format!(r#"name="{name}" value=""#);
        let at = html.find(&key).unwrap() + key.len();
        html[at..at + 36].to_owned()
    };
    let names = format!(
        "step=names&org_id={}&person_id={}&org_role=customer&org_name=Gull+Rock+Ferry",
        id("org_id"),
        id("person_id")
    );
    let (status, _, step) = send(&app, form("POST", "/parties/wizard", &names)).await;
    assert_eq!(status, StatusCode::OK, "{step}");
    assert!(step.contains(r#"name="org_role" value="customer" class="radio radio-xs" checked"#));
}

/// Every create entry point is the wizard (BR-PARTY-02's duplicate check); the New person and New organization
/// dialogs (05 OP-PARTY-01/02) stay as routes, with no top-level button.
#[tokio::test]
async fn every_create_entry_point_lands_on_the_wizard() {
    let app = demo();
    for page in ["/", "/customers", "/parties", "/parties/p-hp", "/sales", "/payments", "/settings"] {
        let (status, html) = get(&app, page).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(!html.contains("/parties/people/new") && !html.contains("/parties/organizations/new"), "{page}");
        assert!(html.contains(r#"href="/parties/new?role=customer""#), "{page}: the top bar's Customer");
    }
    let (_, html) = get(&app, "/parties").await;
    assert!(!html.contains(r#"<a class="btn btn-sm btn-primary" href="/parties/new">"#), "New contact is in the header");
    for dialog in ["/parties/people/new", "/parties/organizations/new?role=customer"] {
        let (status, html) = htmx_get(&app, dialog).await;
        assert_eq!(status, StatusCode::OK, "{dialog} is kept");
        assert!(html.contains("<form"));
    }
}
