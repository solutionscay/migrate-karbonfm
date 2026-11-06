//! S6 Opportunities (UI-10, UI-11, the party tab; OP-SALES-01..03; BR-SALES-12..14). Small demo seed.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::{db, router, AppState};
use rusqlite::{Connection, OptionalExtension};
use tower::ServiceExt;

/// A router and a second handle on the same data.
fn demo() -> (axum::Router, Connection, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let path = db::database_path(tmp.path());
    let (conn, _) = db::open(&path, db::Seed::Demo).unwrap();
    let app = router(AppState::new(conn).with_data_dir(tmp.path()));
    let check = Connection::open(&path).unwrap();
    (app, check, tmp)
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

fn local_today() -> String {
    karbon_server::audit::local_now()[..10].to_owned()
}

fn id_from(headers: &HeaderMap) -> String {
    headers["HX-Redirect"].to_str().unwrap().rsplit('/').next().unwrap().to_owned()
}

type Row =
    (Option<String>, String, Option<String>, Option<String>, Option<String>, i64, Option<String>, Option<String>);

/// (lead_id, probability, start_date, end_date, status_cached, active_ind, type_display, salesperson_id)
fn row_of(check: &Connection, id: &str) -> Row {
    check
        .query_row(
            "SELECT lead_id, probability, start_date, end_date, status_cached, active_ind, type_display, salesperson_id
             FROM opportunity WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
        )
        .unwrap()
}

/// A person with no role at all.
async fn new_person(_app: &axum::Router, check: &Connection, first: &str) -> String {
    let id = format!("p-test-{first}");
    let name = format!("{first} Testperson");
    check
        .execute(
            "INSERT INTO party (id, name_cached, organization_ind, person_ind, deleted_ind) VALUES (?1, ?2, 0, 1, 0)",
            [&id, &name],
        )
        .unwrap();
    check
        .execute(
            "INSERT INTO person (id, first_name, last_name) VALUES (?1, ?2, 'Testperson')",
            [&id, &first.to_owned()],
        )
        .unwrap();
    id
}

// ---------------------------------------------------------------------------------------------------------------
// Create (BR-SALES-13, O1-O4)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn created_for_a_customer_the_lead_is_cleared_and_the_defaults_are_set() {
    let (app, check, _tmp) = demo();
    let (status, headers, _) = send(&app, form("POST", "/opportunities", "other_id=p-hp&name=New+wing")).await;
    assert_eq!(status, StatusCode::OK);
    let to = headers["HX-Redirect"].to_str().unwrap();
    assert!(to.starts_with("/opportunities/"), "{to}");
    let id = id_from(&headers);
    let r = row_of(&check, &id);
    assert_eq!(r.0, None, "602 s116-120: a customer has no lead link");
    assert_eq!(r.1, "unknown", "602 s56");
    assert_eq!(r.2.as_deref(), Some(local_today().as_str()), "602 s57");
    assert_eq!(r.3, None, "new is active: no date closed");
    assert_eq!(r.4.as_deref(), Some("New"), "602 s145-146: status 1");
    assert_eq!(r.5, 1, "active");
    assert_eq!(r.6.as_deref(), Some("Customer"), "field 62");
    assert_eq!(r.7.as_deref(), Some("r-mara-emp"), "O1: the current user's employee role");
    let (name, status_id, cached, by): (String, i64, String, String) = check
        .query_row(
            "SELECT name, opportunity_status_id, party_name_cached, creation_account_name FROM opportunity WHERE id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(name, "New wing");
    assert_eq!(status_id, 1);
    assert_eq!(cached, "Halden & Pryce Architects");
    assert!(!by.is_empty(), "stamped");
}

#[tokio::test]
async fn created_for_a_party_with_no_role_makes_it_a_lead() {
    let (app, check, _tmp) = demo();
    let person = new_person(&app, &check, "Nora").await;
    let (_, headers, _) = send(&app, form("POST", "/opportunities", &format!("other_id={person}"))).await;
    let id = id_from(&headers);
    let r = row_of(&check, &id);
    let lead = r.0.clone().expect("a lead link");
    assert_eq!(r.6.as_deref(), Some("Lead"));
    let (party, ind, current): (String, i64, i64) = check
        .query_row("SELECT party_id, lead_ind, current_ind FROM party_role WHERE id = ?1", [&lead], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((party.as_str(), ind, current), (person.as_str(), 1, 1));
    let row: Option<String> =
        check.query_row("SELECT id FROM lead WHERE id = ?1", [&lead], |r| r.get(0)).optional().unwrap();
    assert!(row.is_some(), "the subtype row");

    // A second one from the list reuses the current lead role.
    let (_, headers, _) = send(&app, form("POST", "/opportunities", &format!("other_id={person}"))).await;
    assert_eq!(row_of(&check, &id_from(&headers)).0.as_deref(), Some(lead.as_str()));
    let roles: i64 =
        check.query_row("SELECT COUNT(*) FROM party_role WHERE party_id = ?1", [&person], |r| r.get(0)).unwrap();
    assert_eq!(roles, 1);
}

#[tokio::test]
async fn created_from_a_party_page_adds_no_role_and_reads_customer() {
    let (app, check, _tmp) = demo();
    let person = new_person(&app, &check, "Omar").await;
    let (status, html) = htmx_get(&app, &format!("/opportunities/new?party_id={person}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Omar Testperson") && html.contains(r#"name="party_id""#));
    assert!(!html.contains(r#"id="contact-picker""#), "the party is given");
    let (_, headers, _) = send(&app, form("POST", "/opportunities", &format!("party_id={person}"))).await;
    let r = row_of(&check, &id_from(&headers));
    assert_eq!(r.0, None, "610 s3 passes no LeadId");
    assert_eq!(r.6.as_deref(), Some("Customer"), "the source's bug, built as intended (D-07)");
    let roles: i64 =
        check.query_row("SELECT COUNT(*) FROM party_role WHERE party_id = ?1", [&person], |r| r.get(0)).unwrap();
    assert_eq!(roles, 0, "O3: no role added");
}

#[tokio::test]
async fn no_party_is_refused() {
    let (app, check, _tmp) = demo();
    let before: i64 = check.query_row("SELECT COUNT(*) FROM opportunity", [], |r| r.get(0)).unwrap();
    for body in ["name=x", "other_id=nobody", "party_id=nobody"] {
        let (status, _, html) = send(&app, form("POST", "/opportunities", body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(html.contains("Choose a person or organization."), "{body}");
    }
    let after: i64 = check.query_row("SELECT COUNT(*) FROM opportunity", [], |r| r.get(0)).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        send(&app, form("POST", "/opportunities", "other_id=p-hp&active_ind=1")).await.0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown field"
    );
}

#[tokio::test]
async fn the_new_dialog_has_the_picker_and_the_wizard_link() {
    let (app, _check, _tmp) = demo();
    let (status, html) = htmx_get(&app, "/opportunities/new").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="contact-picker""#) && html.contains(r#"name="other_id""#));
    assert!(html.contains(r#"href="/parties/new?role=lead""#), "kontacts 602 s8");
    assert_eq!(htmx_get(&app, "/opportunities/new?party_id=nobody").await.0, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------------------------------------------
// Status (BR-SALES-12, O5)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn status_active_closed_active_on_the_opportunity_and_its_lead_role() {
    let (app, check, _tmp) = demo();
    let person = new_person(&app, &check, "Pia").await;
    let (_, headers, _) = send(&app, form("POST", "/opportunities", &format!("other_id={person}"))).await;
    let id = id_from(&headers);
    let lead = row_of(&check, &id).0.unwrap();
    let role = |c: &Connection| -> (i64, Option<String>) {
        c.query_row("SELECT current_ind, thru_date FROM party_role WHERE id = ?1", [&lead], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
    };
    let uri = format!("/opportunities/{id}/status");

    // Lost, with a typed date.
    let (status, headers, html) = send(&app, form("POST", &uri, "status_id=5&end_date=2026-03-04")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("karbon-toast"));
    assert!(html.contains(r#"id="opportunity-summary""#) && html.contains("Lost"));
    let r = row_of(&check, &id);
    assert_eq!((r.3.as_deref(), r.4.as_deref(), r.5), (Some("2026-03-04"), Some("Lost"), 0));
    assert_eq!(role(&check), (0, Some("2026-03-04".to_owned())));

    // Back to active: end date and lead role cleared (608 s107-113).
    assert_eq!(send(&app, form("POST", &uri, "status_id=2")).await.0, StatusCode::OK);
    let r = row_of(&check, &id);
    assert_eq!((r.3.as_deref(), r.4.as_deref(), r.5), (None, Some("In process"), 1));
    assert_eq!(role(&check), (1, None));

    // Closed without a date: today (608 s45).
    assert_eq!(send(&app, form("POST", &uri, "status_id=4")).await.0, StatusCode::OK);
    let r = row_of(&check, &id);
    assert_eq!((r.3, r.4.as_deref(), r.5), (Some(local_today()), Some("Won"), 0));
    assert_eq!(role(&check), (0, Some(local_today())));
}

#[tokio::test]
async fn a_status_change_clears_the_seeds_expected_close_and_refuses_what_is_unknown() {
    let (app, check, _tmp) = demo();
    let before = row_of(&check, "o-hp-fitout2");
    for body in ["status_id=99", "status_id=abc", "status_id=", "status_id=4&end_date=2026-02-31"] {
        let (status, _, _) = send(&app, form("POST", "/opportunities/o-hp-fitout2/status", body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    assert_eq!(row_of(&check, "o-hp-fitout2"), before, "refused: nothing changed");
    assert_eq!(send(&app, form("POST", "/opportunities/nobody/status", "status_id=1")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(send(&app, form("POST", "/opportunities/o-hp-fitout2/status", "status_id=3")).await.0, StatusCode::OK);
    assert_eq!(row_of(&check, "o-hp-fitout2").3, None, "608 s108");
}

// ---------------------------------------------------------------------------------------------------------------
// Edit (O6, O7)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn edit_changes_only_the_form_fields() {
    let (app, check, _tmp) = demo();
    let before = row_of(&check, "o-hp-fitout2");
    let (status, html) = htmx_get(&app, "/opportunities/o-hp-fitout2/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"hx-put="/opportunities/o-hp-fitout2""#));
    assert!(html.contains(r#"value="18500.00""#));
    for p in ["unknown", "unlikely", "likely", "definite", "60%"] {
        assert!(html.contains(&format!(r#"<option value="{p}""#)), "O7 offers {p}");
    }
    assert!(html.contains("Mara Ellison"), "value list 44");
    assert!(html.contains(r#"value="p-hp-eh""#), "value list 54: the party's contacts");

    let (status, headers, html) = send(
        &app,
        form(
            "PUT",
            "/opportunities/o-hp-fitout2",
            "name=Fit-out+3&value=%2425%2C000.50&probability=likely&start_date=2026-02-01&end_date=&salesperson_id=r-mara-emp\
             &notes=Call+first&primary_contact_party_id=p-hp-eh&other_id=p-cs",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let trigger = headers["HX-Trigger"].to_str().unwrap();
    assert!(trigger.contains("karbon-toast") && trigger.contains("karbon-close-dialog"), "{trigger}");
    assert!(html.contains(r#"hx-swap-oob="true""#) && html.contains("$25,000.50"));
    let after = row_of(&check, "o-hp-fitout2");
    assert_eq!((after.4.clone(), after.5), (before.4, before.5), "O6: status and active do not change here");
    let row: (String, i64, String, String, Option<String>, String, String) = check
        .query_row(
            "SELECT name, value, probability, start_date, end_date, notes, referring_party_id FROM opportunity
             WHERE id = 'o-hp-fitout2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        ("Fit-out 3".into(), 2500050, "likely".into(), "2026-02-01".into(), None, "Call first".into(), "p-cs".into())
    );
    let cached: String = check
        .query_row("SELECT referring_party_name_cached FROM opportunity WHERE id = 'o-hp-fitout2'", [], |r| r.get(0))
        .unwrap();
    let name: String = check.query_row("SELECT name_cached FROM party WHERE id = 'p-cs'", [], |r| r.get(0)).unwrap();
    assert_eq!(cached, name, "722 s38-39");
    let m: String = check
        .query_row("SELECT modification_timestamp FROM opportunity WHERE id = 'o-hp-fitout2'", [], |r| r.get(0))
        .unwrap();
    assert!(m.starts_with(&local_today()), "stamped: {m}");

    // Keep without a choice; clear on request.
    let same = "name=Fit-out+3&value=25000.50&probability=likely";
    assert_eq!(send(&app, form("PUT", "/opportunities/o-hp-fitout2", same)).await.0, StatusCode::OK);
    let referring: Option<String> = check
        .query_row("SELECT referring_party_id FROM opportunity WHERE id = 'o-hp-fitout2'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(referring.as_deref(), Some("p-cs"));
    let clear = format!("{same}&clear_referring=on");
    assert_eq!(send(&app, form("PUT", "/opportunities/o-hp-fitout2", &clear)).await.0, StatusCode::OK);
    let referring: (Option<String>, Option<String>) = check
        .query_row(
            "SELECT referring_party_id, referring_party_name_cached FROM opportunity WHERE id = 'o-hp-fitout2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(referring, (None, None));
}

#[tokio::test]
async fn edit_validates_and_keeps_a_stored_probability() {
    let (app, check, _tmp) = demo();
    let uri = "/opportunities/o-hp-fitout2";
    for (body, field) in [
        ("value=9.999", "value"),
        ("probability=maybe", "probability"),
        ("start_date=2026-13-01", "start_date"),
        ("end_date=soon", "end_date"),
        ("salesperson_id=nobody", "salesperson_id"),
        ("primary_contact_party_id=p-bm", "primary_contact_party_id"),
        ("other_id=nobody", "referring_party_id"),
    ] {
        let (status, _, html) = send(&app, form("PUT", uri, body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(html.contains(&format!(r#"id="err-{field}""#)), "{body}");
    }
    for extra in ["active_ind=0", "status_cached=Won", "opportunity_status_id=4", "party_id=p-bm"] {
        assert_eq!(send(&app, form("PUT", uri, extra)).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{extra}");
    }
    // O7: "60%" is outside value list 43 and stays.
    assert_eq!(send(&app, form("PUT", uri, "name=Office&probability=60%25")).await.0, StatusCode::OK);
    let p: String =
        check.query_row("SELECT probability FROM opportunity WHERE id = 'o-hp-fitout2'", [], |r| r.get(0)).unwrap();
    assert_eq!(p, "60%");
    assert_eq!(send(&app, form("PUT", "/opportunities/nobody", "name=x")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(htmx_get(&app, "/opportunities/nobody/edit").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, "/opportunities/nobody").await.0, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------------------------------------------
// Totals and the list (BR-SALES-14, O8, O9, O10)
// ---------------------------------------------------------------------------------------------------------------

/// The chart's JSON out of a fragment.
fn chart_json(html: &str) -> serde_json::Value {
    let raw = html.split("data-status-chart=\"").nth(1).expect("a chart").split('"').next().unwrap();
    let raw = raw.replace("&quot;", "\"").replace("&#34;", "\"").replace("&#38;", "&").replace("&#39;", "'");
    serde_json::from_str(&raw).unwrap()
}

#[tokio::test]
async fn status_totals_active() {
    let (app, _check, _tmp) = demo();
    let (status, html) = htmx_get(&app, "/opportunities/status-totals").await;
    assert_eq!(status, StatusCode::OK);
    let chart = chart_json(&html);
    assert_eq!(chart["labels"], serde_json::json!(["In process", "New", "Proposal sent"]), "O8: by name, with spaces");
    assert_eq!(chart["values"], serde_json::json!([52750.0, 6000.0, 54700.0]));
    assert!(html.contains("$113,450.00"), "the sum equals the dashboard's pipeline");
    let (_, dash) = get(&app, "/").await;
    assert!(dash.contains("$113,450.00"));

    let (_, html) = htmx_get(&app, "/opportunities/status-totals?active=0").await;
    let chart = chart_json(&html);
    assert_eq!(chart["labels"], serde_json::json!(["Won"]));
    assert_eq!(chart["values"], serde_json::json!([9380.0]));
    let (_, html) = htmx_get(&app, "/opportunities/status-totals?active=all").await;
    assert_eq!(chart_json(&html)["labels"].as_array().unwrap().len(), 4);
    assert_eq!(htmx_get(&app, "/opportunities/status-totals?active=maybe").await.0, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_list_shows_active_ones_by_status_then_name_with_subtotals() {
    let (app, _check, _tmp) = demo();
    let (status, html) = get(&app, "/opportunities").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains(r#"href="/opportunities" aria-current="page""#), "the sidebar link is live");
    assert!(html.contains("chart-4.4.7"), "the chart's script");
    assert!(html.contains(r#"hx-get="/opportunities/status-totals""#));
    for column in ["Name", "Party", "Status", "Value", "Probability", "Active", "Next Task"] {
        assert!(html.contains(&format!(">{column}</th>")), "{column}");
    }
    assert!(html.contains("1–6 of 6"));
    assert!(!html.contains("Brand refresh"), "Won is not active");
    let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("{s}"));
    // In process: Annual drawing archive digitising, Fleet livery redesign; New: Annual support plan; Proposal sent: ...
    assert!(at("Annual drawing archive") < at("Fleet livery"));
    assert!(at("Fleet livery") < at("In process subtotal"));
    assert!(at("In process subtotal") < at("Annual support plan"));
    assert!(at("New subtotal") < at("Office fit-out"));
    assert!(at("Office fit-out") < at("Storefront signage"));
    assert!(at("Warehouse wayfinding") < at("Proposal sent subtotal"));
    assert!(html.contains("$52,750.00") && html.contains("$54,700.00"), "subtotals");
    assert!(html.contains("60%"), "a stored probability is shown");

    let (_, html) = htmx_get(&app, "/opportunities?active=all").await;
    assert!(!html.contains("<html"), "a fragment");
    assert!(html.contains("Brand refresh") && html.contains("1–7 of 7") && html.contains("Won subtotal"));
    assert_eq!(htmx_get(&app, "/opportunities?active=maybe").await.0, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn search_covers_name_party_and_notes() {
    let (app, check, _tmp) = demo();
    let (_, html) = htmx_get(&app, "/opportunities?q=LIVERY").await;
    assert!(html.contains("Fleet livery") && html.contains("1–1 of 1"));
    let (_, html) = htmx_get(&app, "/opportunities?q=bluefin").await;
    assert!(html.contains("Warehouse wayfinding") && html.contains("1–1 of 1"), "the party's name");
    check.execute("UPDATE opportunity SET notes = 'Ask about zebra stripes' WHERE id = 'o-jr-sign'", []).unwrap();
    let (_, html) = htmx_get(&app, "/opportunities?q=zebra").await;
    assert!(html.contains("Storefront signage") && html.contains("1–1 of 1"), "the notes");
    let (_, html) = htmx_get(&app, "/opportunities?q=no-such-thing").await;
    assert!(html.contains("No opportunities match."));
    let (_, html) = htmx_get(&app, "/opportunities?q=%3Cscript%3E").await;
    assert!(!html.contains("<script>"));
}

#[tokio::test]
async fn next_task_is_the_earliest_incomplete_one() {
    let (app, _check, _tmp) = demo();
    let about = "entity_type_id=13&entity_id=o-jr-sign&regarding_id=p-jr";
    let mut ids = Vec::new();
    for (subject, due) in [("Later+task", "2027-03-01"), ("Earlier+task", "2027-01-15")] {
        let body = format!("kind=task&subject={subject}&date={due}&due_date={due}&{about}");
        let (status, headers, html) = send(&app, form("POST", "/activities", &body)).await;
        assert_eq!(status, StatusCode::OK, "{html}");
        ids.push(headers["HX-Redirect"].to_str().unwrap().to_owned());
    }
    let (_, html) = get(&app, "/opportunities").await;
    let row = html.split("Storefront signage").nth(1).unwrap().split("</tr>").next().unwrap();
    assert!(row.contains("Earlier task"), "{row}");
    assert!(!row.contains("Later task"), "Q3: the earliest date");

    // Completing it brings the next one up (rel 214: AlertInd, D-29).
    let earlier = ids[1].rsplit('/').next().unwrap();
    let (status, _, _) = send(&app, form("POST", &format!("/tasks/{earlier}/complete"), "")).await;
    assert!(status.is_success(), "{status}");
    let (_, html) = get(&app, "/opportunities").await;
    let row = html.split("Storefront signage").nth(1).unwrap().split("</tr>").next().unwrap();
    assert!(row.contains("Later task") && !row.contains("Earlier task"), "{row}");
}

// ---------------------------------------------------------------------------------------------------------------
// The detail page and the party tab (O11, O12)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_detail_page_shows_the_layout() {
    let (app, _check, _tmp) = demo();
    let (status, html) = get(&app, "/opportunities/o-hp-brand").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"href="/opportunities" aria-current="page""#));
    for text in [
        "Brand refresh",
        "Won",
        "$9,380.00",
        "100%",
        "Mara Ellison",
        "Halden &#38; Pryce Architects",
        "Created by",
        "$3,057.15",
        "$1,500.00",
    ] {
        assert!(html.contains(text), "{text}");
    }
    assert!(html.contains(r#"href="/parties/p-cs""#), "the referring party links to its page");
    for status_name in ["New", "In process", "Proposal sent", "Won", "Lost", "Unqualified", "Closed (other)", "On Hold"]
    {
        assert!(html.contains(&format!(">{status_name}</button>")), "the status menu offers {status_name}");
    }
    assert!(html.contains(r#"id="party-relations""#), "the party's contacts");
    assert!(html.contains(r#"hx-get="/parties/p-hp/relations/new""#), "Add Contact opens the relation dialog");
    assert!(html.contains("/activities/new?kind=task&amp;party_id=p-hp&amp;entity_type_id=13&amp;entity_id=o-hp-brand"));
    assert!(html.contains("/email-messages/new?party_id=p-hp&amp;entity_type_id=13&amp;entity_id=o-hp-brand"));
    assert!(html.contains("/discussions/new?entity_type_id=13&amp;entity_id=o-hp-brand"));
    assert!(html.contains(r#"id="party-files""#) && html.contains(r#"name="entity_type_id" value="13""#));
}

#[tokio::test]
async fn activity_on_an_opportunity_shows_there_and_links_back() {
    let (app, _check, _tmp) = demo();
    let (status, html) =
        htmx_get(&app, "/activities/new?kind=meeting&party_id=p-hp&entity_type_id=13&entity_id=o-hp-brand").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains(r#"name="entity_id" value="o-hp-brand""#) && html.contains(r#"name="regarding_id" value="p-hp""#)
    );

    let body = "kind=note&subject=Budget+talk&message=x&entity_type_id=13&entity_id=o-hp-brand&regarding_id=p-hp";
    let (status, headers, html) = send(&app, form("POST", "/activities", body)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let to = headers["HX-Redirect"].to_str().unwrap().to_owned();
    let (_, page) = get(&app, &to).await;
    assert!(page.contains(r#"href="/opportunities/o-hp-brand""#), "the activity links back");
    let (_, detail) = get(&app, "/opportunities/o-hp-brand").await;
    assert!(detail.contains("Budget talk"));
    let (_, other) = get(&app, "/opportunities/o-hp-fitout2").await;
    assert!(!other.contains("Budget talk"));
    let (_, party) = get(&app, "/parties/p-hp").await;
    assert!(party.contains("Budget talk"), "regarding the party too");
    let (_, list) = htmx_get(&app, "/activities?kind=all&entity_type_id=13&entity_id=o-hp-brand").await;
    assert!(list.contains("Budget talk"));
}

#[tokio::test]
async fn a_discussion_on_an_opportunity_is_titled_by_its_party() {
    let (app, _check, _tmp) = demo();
    let (status, html) = htmx_get(&app, "/discussions/new?entity_type_id=13&entity_id=o-hp-brand").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Halden &#38; Pryce Architects"), "D2: its party's name");
    let (status, headers, html) =
        send(&app, form("POST", "/discussions", "entity_type_id=13&entity_id=o-hp-brand&notes=Thoughts%3F")).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(headers["HX-Redirect"].to_str().unwrap().starts_with("/discussions/"));
    let (_, detail) = get(&app, "/opportunities/o-hp-brand").await;
    assert!(detail.contains(headers["HX-Redirect"].to_str().unwrap().rsplit('/').next().unwrap()), "on its timeline");
}

#[tokio::test]
async fn a_file_attached_to_an_opportunity_also_links_its_party() {
    let (app, check, _tmp) = demo();
    let boundary = "XBOUNDARYX";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"entity_type_id\"\r\n\r\n13\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"entity_id\"\r\n\r\no-hp-brand\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"brief.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n\
         --{boundary}--\r\n"
    );
    let req = Request::post("/files")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .header("HX-Request", "true")
        .body(Body::from(body))
        .unwrap();
    let (status, _, html) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("brief.txt") && html.contains(r#"value="13""#), "the opportunity's panel comes back");
    let links: Vec<(i64, String)> = check
        .prepare("SELECT entity_type_id, entity_id FROM file_attachment fa JOIN file f ON f.id = fa.attachment_id WHERE f.file_name = 'brief.txt' ORDER BY entity_type_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(links, vec![(10, "p-hp".to_owned()), (13, "o-hp-brand".to_owned())], "F7");
    let (_, detail) = get(&app, "/opportunities/o-hp-brand").await;
    assert!(detail.contains("brief.txt"));
    let (_, party) = get(&app, "/parties/p-hp").await;
    assert!(party.contains("brief.txt"));

    // Remove from this record: the opportunity's link goes, its panel answers, the party's stays.
    let attachment: String = check
        .query_row("SELECT id FROM file_attachment WHERE entity_type_id = 13 AND entity_id = 'o-hp-brand'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let (status, _, html) =
        send(&app, Request::delete(format!("/attachments/{attachment}")).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="party-files""#) && !html.contains("brief.txt"), "{html}");
    let (_, party) = get(&app, "/parties/p-hp").await;
    assert!(party.contains("brief.txt"));
}

#[tokio::test]
async fn the_party_tab_shows_its_opportunities() {
    let (app, _check, _tmp) = demo();
    let (status, html) = get(&app, "/parties/p-hp").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("Opportunities come in a later release."));
    for name in ["Office fit-out, phase 2", "Annual drawing archive digitising", "Brand refresh"] {
        assert!(html.contains(name), "{name}");
    }
    assert!(html.contains(r#"href="/opportunities/o-hp-brand""#));
    assert!(html.contains(r#"hx-get="/opportunities/new?party_id=p-hp""#), "New from the party page");
    let tab = html.split(r#"id="tab-opps""#).nth(1).unwrap().split("</button>").next().unwrap();
    assert!(tab.contains(r#"<span class="n">3</span>"#), "{tab}");

    let (_, html) = get(&app, "/parties/p-mara").await;
    assert!(html.contains("No opportunities yet."));
}
