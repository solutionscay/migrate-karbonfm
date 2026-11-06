//! I4 S4: templates and merge (UI-16, OP-TEMPLATE-01/02, BR-TEMPLATE-01/02). The commands run on the small demo
//! database; the routes run in process.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::admin::{current_user, CommandError};
use karbon_server::audit::Actor;
use karbon_server::templates::{self, TemplateInput, FIELDS};
use karbon_server::{db, router, AppState};
use rusqlite::Connection;
use tower::ServiceExt;

fn demo() -> (Connection, Actor) {
    let conn = db::open_demo(None).expect("demo database");
    let actor = current_user(&conn).unwrap().expect("the demo has a user");
    (conn, actor)
}

fn input(subject: &str, message: &str) -> TemplateInput {
    TemplateInput {
        name: "Invoice email".into(),
        subject: subject.into(),
        message: message.into(),
        activity_types: vec![1],
    }
}

fn field_errors(e: CommandError) -> Vec<(&'static str, String)> {
    match e {
        CommandError::Validation(v) => v.into_iter().map(|f| (f.field, f.message)).collect(),
        other => panic!("expected Validation, got {other:?}"),
    }
}

fn merge_fields(conn: &Connection, id: i64) -> Vec<(String, bool, bool)> {
    templates::template_get(conn, id)
        .unwrap()
        .merge_fields
        .into_iter()
        .map(|m| (m.name, m.in_use, m.required))
        .collect()
}

/// An email on the demo with a To person (Joanna Wells of H&P), a From (Mara) with an address on the row, and the
/// invoice INV-1035 linked as entity type 12.
fn fixture_email(conn: &Connection) -> &'static str {
    conn.execute_batch(
        "INSERT INTO activity (id, activity_type_id, email_message_ind, subject) VALUES ('a-fx', 1, 1, 'Fixture');
         INSERT INTO activity_party (id, activity_id, party_id, activity_party_role_id, creation_timestamp)
           VALUES ('ap-fx-to', 'a-fx', 'p-hp-jw', 55, '2026-10-01 09:00:00');
         INSERT INTO activity_party (id, activity_id, party_id, activity_party_role_id, creation_timestamp)
           VALUES ('ap-fx-to2', 'a-fx', 'p-mara', 55, '2026-10-01 09:05:00');
         INSERT INTO activity_party (id, activity_id, party_id, activity_party_role_id, creation_timestamp)
           VALUES ('ap-fx-from', 'a-fx', 'p-mara', 54, '2026-10-01 09:00:00');
         INSERT INTO activity_party_email (id, email_address) VALUES ('ap-fx-from', 'billing@ellisonstudio.example');
         INSERT INTO activity_entity (id, activity_id, entity_type_id, entity_id) VALUES ('ae-fx', 'a-fx', 12, 't-inv-1035');",
    )
    .unwrap();
    "a-fx"
}

// ---------------------------------------------------------------------------------------------------------------
// The parser and the save rules (BR-TEMPLATE-01, 478)
// ---------------------------------------------------------------------------------------------------------------

/// An email with a To person (Joanna Wells of H&P) and nothing else: no From and no invoice link.
fn unlinked_email(conn: &Connection) -> &'static str {
    conn.execute_batch(
        "INSERT INTO activity (id, activity_type_id, email_message_ind, subject) VALUES ('a-nolink', 1, 1, 'No link');
         INSERT INTO activity_party (id, activity_id, party_id, activity_party_role_id)
         VALUES ('ap-nolink', 'a-nolink', 'p-hp-jw', 55);",
    )
    .unwrap();
    "a-nolink"
}

#[test]
fn a_token_mismatch_is_refused_and_nothing_is_saved() {
    let (mut conn, actor) = demo();
    let e = templates::template_create(&mut conn, &input("Invoice {{InvoiceNumber", "Hi"), &actor).unwrap_err();
    let errors = field_errors(e);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, "subject");
    assert!(errors[0].1.contains("token symbol mismatch"), "{errors:?}");
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM template", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn an_unknown_token_is_refused_on_save() {
    let (mut conn, actor) = demo();
    let id = templates::template_create(&mut conn, &input("Hello", "Hi {{FirstName}}"), &actor).unwrap();
    let e =
        templates::template_save(&mut conn, id, &input("Hello", "Hi {{FirstName}} {{Project}}"), &actor).unwrap_err();
    let errors = field_errors(e);
    assert_eq!(errors[0].0, "message");
    assert!(errors[0].1.contains("{{Project}}"), "{errors:?}");
    assert_eq!(templates::template_get(&conn, id).unwrap().message, "Hi {{FirstName}}", "nothing saved");
    let e =
        templates::template_save(&mut conn, id, &TemplateInput { activity_types: vec![99], ..input("", "") }, &actor)
            .unwrap_err();
    assert_eq!(field_errors(e)[0].0, "activity_types");
}

#[test]
fn every_field_of_the_list_saves() {
    let (mut conn, actor) = demo();
    let all: String = FIELDS.iter().map(|f| templates::token(f.name)).collect::<Vec<_>>().join(" ");
    let id = templates::template_create(&mut conn, &input("", &all), &actor).unwrap();
    assert_eq!(merge_fields(&conn, id).len(), FIELDS.len());
}

// ---------------------------------------------------------------------------------------------------------------
// Merge fields (BR-TEMPLATE-02, 480)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn merge_fields_are_created_by_name_and_a_removed_one_is_kept_not_in_use() {
    let (mut conn, actor) = demo();
    let id =
        templates::template_create(&mut conn, &input("Invoice {{InvoiceNumber}}", "Hi {{FirstName}}"), &actor).unwrap();
    assert_eq!(
        merge_fields(&conn, id),
        [("{{InvoiceNumber}}".to_string(), true, false), ("{{FirstName}}".to_string(), true, false)]
    );
    let ids: Vec<i64> = templates::template_get(&conn, id).unwrap().merge_fields.iter().map(|m| m.id).collect();
    templates::merge_fields_save(&mut conn, id, &[ids[0]], &actor).unwrap();
    // FirstName gone, DueDate new; InvoiceNumber kept with its required flag (matched on template_id + name).
    templates::template_save(&mut conn, id, &input("Invoice {{InvoiceNumber}}", "Due {{DueDate}}"), &actor).unwrap();
    assert_eq!(
        merge_fields(&conn, id),
        [
            ("{{InvoiceNumber}}".to_string(), true, true),
            ("{{FirstName}}".to_string(), false, false),
            ("{{DueDate}}".to_string(), true, false)
        ]
    );
    let stamped: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM merge_field WHERE creation_timestamp IS NOT NULL AND creation_user_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stamped, 3, "every merge field row is stamped (BR-PARTY-07)");
}

#[test]
fn required_round_trips_and_an_other_templates_row_is_refused() {
    let (mut conn, actor) = demo();
    let a = templates::template_create(&mut conn, &input("", "{{FirstName}} {{LastName}}"), &actor).unwrap();
    let b = templates::template_create(&mut conn, &input("", "{{Date}}"), &actor).unwrap();
    let rows = templates::template_get(&conn, a).unwrap().merge_fields;
    templates::merge_fields_save(&mut conn, a, &[rows[1].id], &actor).unwrap();
    let req: Vec<bool> = templates::template_get(&conn, a).unwrap().merge_fields.iter().map(|m| m.required).collect();
    assert_eq!(req, [false, true]);
    templates::merge_fields_save(&mut conn, a, &[], &actor).unwrap();
    let req: Vec<bool> = templates::template_get(&conn, a).unwrap().merge_fields.iter().map(|m| m.required).collect();
    assert_eq!(req, [false, false], "an unchecked box clears required_ind (481 s85)");
    let other = templates::template_get(&conn, b).unwrap().merge_fields[0].id;
    assert!(matches!(templates::merge_fields_save(&mut conn, a, &[other], &actor), Err(CommandError::Validation(_))));
    assert!(matches!(templates::merge_fields_save(&mut conn, 999, &[], &actor), Err(CommandError::NotFound)));
}

// ---------------------------------------------------------------------------------------------------------------
// Merge (OP-TEMPLATE-01)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn every_field_merges_on_a_fixture_email() {
    let (mut conn, actor) = demo();
    let activity = fixture_email(&conn);
    let all: String =
        FIELDS.iter().map(|f| format!("{}={}", f.name, templates::token(f.name))).collect::<Vec<_>>().join("\n");
    let id = templates::template_create(&mut conn, &input("Invoice {{InvoiceNumber}} for {{FirstName}}", &all), &actor)
        .unwrap();
    let before: i64 = conn.query_row("SELECT total_changes()", [], |r| r.get(0)).unwrap();
    let merged = templates::template_merge(&conn, id, activity).unwrap();
    let after: i64 = conn.query_row("SELECT total_changes()", [], |r| r.get(0)).unwrap();
    assert_eq!(before, after, "a merge writes nothing");
    assert_eq!(merged.subject, "Invoice INV-1035 for Joanna");
    let jw: (String, String, String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT p.name_cached, pe.first_name, pe.last_name, p.email_address_cached, p.phone_number_cached \
             FROM party p JOIN person pe ON pe.id = p.id WHERE p.id = 'p-hp-jw'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    let lines: std::collections::HashMap<&str, &str> =
        merged.message.lines().filter_map(|l| l.split_once('=')).collect();
    assert_eq!(lines["RecipientName"], jw.0, "the first To party (by creation), not Mara");
    assert_eq!(lines["FirstName"], jw.1);
    assert_eq!(lines["LastName"], jw.2);
    assert_eq!(lines["EmailAddress"], jw.3.unwrap_or_default());
    assert_eq!(lines["PhoneNumber"], jw.4.unwrap_or_default());
    assert_eq!(lines["SenderName"], "Mara Ellison");
    assert_eq!(lines["SenderEmail"], "billing@ellisonstudio.example", "the From row's address wins");
    assert_eq!(lines["InvoiceNumber"], "INV-1035");
    assert_eq!(lines["InvoiceTotal"], "$5,200.00", "the page's Total, before the $1,000.00 retainer credit");
    assert_eq!(lines["BalanceDue"], "$0.00");
    assert_eq!(lines["DueDate"], "Sep 13, 2026");
    assert!(!lines["Date"].is_empty() && lines["Date"].contains(", 20"), "today: {}", lines["Date"]);
    assert!(!merged.message.contains("{{"), "every token replaced: {}", merged.message);
}

#[test]
fn a_missing_value_is_empty_never_the_previous_fields() {
    let (mut conn, actor) = demo();
    // A To person, no From, no invoice link.
    let email = unlinked_email(&conn);
    let id = templates::template_create(
        &mut conn,
        &input("", "[{{FirstName}}][{{SenderName}}][{{InvoiceNumber}}][{{DueDate}}]"),
        &actor,
    )
    .unwrap();
    let merged = templates::template_merge(&conn, id, email).unwrap();
    assert_eq!(merged.message, "[Joanna][][][]");
}

#[test]
fn a_template_with_no_merge_fields_still_merges_and_unknown_ids_are_not_found() {
    let (mut conn, actor) = demo();
    let id = templates::template_create(&mut conn, &input("Thanks", "Thank you."), &actor).unwrap();
    let merged = templates::template_merge(&conn, id, "a-mail-1048").unwrap();
    assert_eq!((merged.subject.as_str(), merged.message.as_str()), ("Thanks", "Thank you."));
    assert!(matches!(templates::template_merge(&conn, id, "nope"), Err(CommandError::NotFound)));
    assert!(matches!(templates::template_merge(&conn, 999, "a-mail-1048"), Err(CommandError::NotFound)));
}

// ---------------------------------------------------------------------------------------------------------------
// Routes (UI-16)
// ---------------------------------------------------------------------------------------------------------------

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
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

/// The demo in a temp file: the router owns one connection, the test reads with a second one.
fn file_demo() -> (tempfile::TempDir, axum::Router, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("karbonfm.db");
    let app = router(AppState::new(db::open_demo(Some(&path)).unwrap()));
    let check = Connection::open(&path).unwrap();
    (dir, app, check)
}

#[tokio::test]
async fn the_list_page_is_linked_from_the_sidebar_and_has_an_empty_state() {
    let (conn, _) = demo();
    let app = router(AppState::new(conn));
    let (status, _, html) = send(&app, get("/templates")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"href="/templates" aria-current="page""#), "the sidebar marks the page");
    assert!(html.contains("No templates yet. Choose New, then Template, to make one."));
    let (_, _, html) = send(&app, get("/")).await;
    assert!(html.contains(r#"href="/templates""#) && !html.contains(r#"data-soon="Templates""#));
}

#[tokio::test]
async fn create_save_merge_fields_and_preview_through_the_routes() {
    let (_dir, app, check) = file_demo();
    fixture_email(&check);

    let (status, _, html) = send(&app, get("/templates/new")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"hx-post="/templates""#));
    let (status, _, html) = send(&app, form("POST", "/templates", "name=")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Name the template."));
    let (status, headers, _) = send(&app, form("POST", "/templates", "name=Invoice+email")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers["HX-Redirect"], "/templates/1");

    let (status, _, html) = send(&app, get("/templates/1")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Invoice email</h1>") && html.contains("No merge fields."));
    assert!(html.contains("&#123;&#123;RecipientName&#125;&#125;"), "the list of available fields");
    assert!(html.contains(r#"<option value="a-fx">"#), "the preview offers the demo's emails");

    let body = "name=Invoice+email&subject=Invoice+%7B%7BInvoiceNumber%7D%7D&message=Hi+%7B%7BFirstName%7D%7D%2C&activity_type=1&activity_type=2";
    let (status, headers, html) = send(&app, form("PUT", "/templates/1", body)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("Template saved"));
    assert!(html.contains(r#"id="template-editor""#) && html.contains(r#"hx-swap-oob="true""#));
    assert!(html.contains("{{InvoiceNumber}}") && html.contains("{{FirstName}}"), "the new rows out of band");
    let types: String =
        check.query_row("SELECT activity_type_id FROM template WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(types, "1\n2");

    let (status, _, html) = send(&app, form("PUT", "/templates/1", "subject=%7B%7BOops&message=x")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("token symbol mismatch") && html.contains(r#"aria-invalid="true""#));
    let (status, _, _) = send(&app, form("PUT", "/templates/1", "subject=a&color=red")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "an unknown form field");

    let mf: i64 = check
        .query_row("SELECT id FROM merge_field WHERE template_id = 1 AND name = '{{FirstName}}'", [], |r| r.get(0))
        .unwrap();
    let (status, headers, html) = send(&app, form("PUT", "/templates/1/merge-fields", &format!("required={mf}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("Merge fields saved"));
    assert!(html.contains(&format!(
        r#"value="{mf}" class="checkbox checkbox-xs" aria-label="{{{{FirstName}}}} is required" checked"#
    )));
    let (status, _, _) = send(&app, form("PUT", "/templates/1/merge-fields", "required=9999")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _, html) = send(&app, form("POST", "/templates/1/merge", "activity_id=a-fx")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Invoice INV-1035") && html.contains("Hi Joanna,"), "{html}");
    let (status, _, html) = send(&app, form("POST", "/templates/1/merge", "activity_id=nope")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Choose an email"));
    let (status, _, _) = send(&app, form("POST", "/templates/9/merge", "activity_id=a-fx")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(&app, get("/templates/9")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, _, html) = send(&app, get("/templates")).await;
    assert!(html.contains(r#"href="/templates/1""#) && html.contains("Email, Note"));
}

#[tokio::test]
async fn template_text_is_escaped() {
    let (mut conn, actor) = demo();
    let id = templates::template_create(
        &mut conn,
        &TemplateInput { name: "<script>x</script>".into(), ..input("", "<b>{{FirstName}}</b>") },
        &actor,
    )
    .unwrap();
    let app = router(AppState::new(conn));
    let (_, _, html) = send(&app, get(&format!("/templates/{id}"))).await;
    assert!(!html.contains("<script>x") && !html.contains("<b>{{"));
    let (_, _, html) = send(&app, form("POST", &format!("/templates/{id}/merge"), "activity_id=a-mail-1048")).await;
    assert!(html.contains("&lt;b&gt;Joanna&lt;/b&gt;") || html.contains("&#60;b&#62;Joanna&#60;/b&#62;"), "{html}");
}

// ---------------------------------------------------------------------------------------------------------------
// The big demo (karbon-synth through the import, OD-07-03)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn the_big_demo_templates_use_known_braces_and_merge() {
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("export");
    let data = dir.path().join("data");
    karbon_synth::generate(karbon_synth::Config { scale: 0.1, dirty: false, ..Default::default() }, &export).unwrap();
    karbon_server::import::import_export(&export, &data, &Default::default()).unwrap();
    let conn = Connection::open(db::database_path(&data)).unwrap();
    let list = templates::template_list(&conn).unwrap();
    assert_eq!(list.len(), 6);
    // An email with a To party and a sales transaction link, if the demo has one; else any email.
    let email: String = conn
        .query_row(
            "SELECT a.id FROM activity a WHERE a.email_message_ind = 1 \
             ORDER BY EXISTS (SELECT 1 FROM activity_party ap WHERE ap.activity_id = a.id AND ap.activity_party_role_id = 55) DESC, \
             EXISTS (SELECT 1 FROM activity_entity ae WHERE ae.activity_id = a.id AND ae.entity_type_id = 12) DESC, a.id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for t in &list {
        let d = templates::template_get(&conn, t.id).unwrap();
        for text in [&d.subject, &d.message] {
            assert!(!text.contains("<<") && !text.contains("[First Name]"), "{}: {text}", d.name);
            for name in templates::parse_tokens(text).unwrap() {
                assert!(FIELDS.iter().any(|f| f.name == name), "{}: unknown {name}", d.name);
            }
        }
        assert!(
            d.merge_fields.iter().all(|m| m.in_use && m.name.starts_with("{{")),
            "{}: {:?}",
            d.name,
            d.merge_fields
        );
        let merged = templates::template_merge(&conn, t.id, &email).unwrap();
        assert!(!merged.subject.contains("{{") && !merged.message.contains("{{"), "{merged:?}");
    }
    let invoice = list.iter().find(|t| t.name == "Invoice email").unwrap();
    let d = templates::template_get(&conn, invoice.id).unwrap();
    assert_eq!(d.subject, "Invoice {{InvoiceNumber}} from Ellison Studio");
    let required: Vec<&str> = d.merge_fields.iter().filter(|m| m.required).map(|m| m.name.as_str()).collect();
    assert_eq!(required, ["{{InvoiceNumber}}"]);
}

// ---------------------------------------------------------------------------------------------------------------
// The preview never leaves a blank box (user report: Body {{InvoiceTotal}}, email INV-1048, labels and nothing else)
// ---------------------------------------------------------------------------------------------------------------

const TOTAL: &str = "%7B%7BInvoiceTotal%7D%7D";

/// A new template that has nothing saved but its name.
async fn blank_template(app: &axum::Router) {
    let (status, _, _) = send(app, form("POST", "/templates", "name=Preview")).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn a_saved_invoice_total_fills_in_for_the_seed_invoice_email() {
    let (_dir, app, _check) = file_demo();
    blank_template(&app).await;
    let body = format!("name=Preview&subject=&message={TOTAL}&activity_type=1");
    let (status, _, html) = send(&app, form("PUT", "/templates/1", &body)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let (status, _, html) = send(&app, form("POST", "/templates/1/merge", "activity_id=a-mail-1048")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("$3,557.15"), "the Total of the INV-1048 page: {html}");
}

#[tokio::test]
async fn the_preview_merges_the_text_on_screen_and_saves_nothing() {
    let (_dir, app, check) = file_demo();
    blank_template(&app).await;
    let body = "activity_id=a-mail-1048&subject=Hi+%7B%7BFirstName%7D%7D&message=Hi+%7B%7BFirstName%7D%7D";
    let (status, _, html) = send(&app, form("POST", "/templates/1/merge", body)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert_eq!(html.matches("Hi Joanna").count(), 2, "{html}");
    let saved: Option<String> = check.query_row("SELECT message FROM template WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(saved, None, "a preview writes nothing");
    // The page sends the editor's text with the preview.
    let (_, _, page) = send(&app, get("/templates/1")).await;
    assert!(page.contains("hx-include=\"#template-editor [name=subject], #template-editor [name=message]\""), "{page}");
    assert!(!page.contains("Merges the saved template"), "the page no longer says it merges the saved text");
}

#[tokio::test]
async fn unsaved_text_with_a_bad_token_says_so_in_the_preview() {
    let (_dir, app, _check) = file_demo();
    blank_template(&app).await;
    let (status, _, html) =
        send(&app, form("POST", "/templates/1/merge", "activity_id=a-mail-1048&subject=&message=%7B%7BNope%7D%7D"))
            .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Body:") && html.contains("Unknown merge field"), "{html}");
    let (status, _, html) =
        send(&app, form("POST", "/templates/1/merge", "activity_id=a-mail-1048&subject=%7B%7BOops&message=")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Subject:") && html.contains("token symbol mismatch"), "{html}");
}

#[tokio::test]
async fn an_empty_template_says_there_is_nothing_to_merge() {
    let (_dir, app, _check) = file_demo();
    blank_template(&app).await;
    let (status, _, html) = send(&app, form("POST", "/templates/1/merge", "activity_id=a-mail-1048")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Nothing to merge"), "{html}");
    assert!(!html.contains("data-merged-message"), "no box of empty labels: {html}");
}

#[tokio::test]
async fn a_field_with_no_value_is_named_in_the_preview() {
    let (_dir, app, check) = file_demo();
    let email = unlinked_email(&check);
    blank_template(&app).await;
    let body = format!("activity_id={email}&subject=&message={TOTAL}");
    let (status, _, html) = send(&app, form("POST", "/templates/1/merge", &body)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("data-merge-missing"), "{html}");
    assert!(html.contains("{{InvoiceTotal}}") && html.contains("linked invoice"), "{html}");
    assert!(html.contains("(no subject)") && html.contains("(empty)"), "an empty value is not a blank line: {html}");
}

#[tokio::test]
async fn the_available_fields_are_buttons_for_the_subject_and_body() {
    let (_dir, app, _check) = file_demo();
    blank_template(&app).await;
    let (_, _, page) = send(&app, get("/templates/1")).await;
    assert!(page.contains(r#"x-data="mergeFieldPicker""#), "{page}");
    assert_eq!(page.matches("data-merge-token=").count(), FIELDS.len());
    assert_eq!(page.matches(r#"<button type="button" class="merge-token""#).count(), FIELDS.len());
    assert_eq!(page.matches("data-merge-target").count(), 2, "the Subject and the Body");
}

// ---------------------------------------------------------------------------------------------------------------
// {{InvoiceTotal}} is the figure the invoice page labels "Total" (owner, 2026-11-06; BR-TEMPLATE-01)
// ---------------------------------------------------------------------------------------------------------------

/// A new email about a sales transaction.
fn email_about(conn: &Connection, transaction: &str) -> String {
    let id = format!("a-about-{transaction}");
    conn.execute_batch(&format!(
        "INSERT INTO activity (id, activity_type_id, email_message_ind, subject) VALUES ('{id}', 1, 1, 'About');
         INSERT INTO activity_entity (id, activity_id, entity_type_id, entity_id)
         VALUES ('ae-{id}', '{id}', 12, '{transaction}');"
    ))
    .unwrap();
    id
}

fn merged_value(conn: &Connection, email: &str, field: &str) -> Option<String> {
    templates::merge_values(conn, email, "2026-11-06").unwrap().get(field).cloned()
}

/// The figure in the Total row of the invoice page, as the page draws it.
async fn page_total(app: &axum::Router, doc_number: &str) -> String {
    let (status, _, html) = send(app, get(&format!("/sales/{doc_number}"))).await;
    assert_eq!(status, StatusCode::OK, "{doc_number}");
    let (_, rest) = html.split_once(r#">Total</dt><dd class="num inv-rule inv-bold">"#).expect("the Total row");
    rest.split_once("</dd>").expect("the end of the row").0.to_string()
}

#[test]
fn invoice_total_is_before_the_retainer_credit_and_balance_due_is_what_is_owed() {
    let (conn, _) = demo();
    assert_eq!(merged_value(&conn, "a-mail-1048", "InvoiceTotal").as_deref(), Some("$3,557.15"));
    assert_eq!(merged_value(&conn, "a-mail-1048", "BalanceDue").as_deref(), Some("$2,057.15"));
}

#[test]
fn invoice_total_with_no_retainer_credit_is_the_plain_total() {
    let (conn, _) = demo();
    let (id, stored): (String, i64) = conn
        .query_row(
            "SELECT st.id, st.total_amt FROM sales_transaction st WHERE COALESCE(st.invoice_ind, 0) = 1
             AND NOT EXISTS (SELECT 1 FROM sales_line sl JOIN item it ON it.id = sl.item_id
                             WHERE sl.sales_transaction_id = st.id AND it.id = 'i-retainer')
             ORDER BY st.doc_number LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let email = email_about(&conn, &id);
    assert_eq!(merged_value(&conn, &email, "InvoiceTotal"), Some(karbon_server::money::usd(stored)), "{id}");
}

#[tokio::test]
async fn invoice_total_equals_the_total_on_every_invoice_page() {
    let (_dir, app, check) = file_demo();
    let docs: Vec<(String, String)> = {
        let mut stmt = check.prepare("SELECT id, doc_number FROM sales_transaction ORDER BY doc_number").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert!(docs.len() >= 5, "{docs:?}");
    for (id, doc) in docs {
        let email = email_about(&check, &id);
        assert_eq!(merged_value(&check, &email, "InvoiceTotal"), Some(page_total(&app, &doc).await), "{doc}");
    }
}

#[tokio::test]
async fn invoice_total_follows_the_page_when_the_invoice_has_shipping() {
    // No figure here on purpose: whether shipping is in the Total is the page's rule (I5 decision F1).
    let (_dir, app, check) = file_demo();
    check.execute("UPDATE sales_transaction SET ship_amt = 2550 WHERE id = 't-inv-1048'", []).unwrap();
    let page = page_total(&app, "INV-1048").await;
    assert_eq!(merged_value(&check, "a-mail-1048", "InvoiceTotal"), Some(page));
}

#[test]
fn invoice_total_of_a_deleted_invoice_has_no_value() {
    let (conn, _) = demo();
    conn.execute("UPDATE sales_transaction SET deleted = 1 WHERE id = 't-inv-1048'", []).unwrap();
    assert_eq!(merged_value(&conn, "a-mail-1048", "InvoiceTotal"), None, "the page cannot show it either");
}
