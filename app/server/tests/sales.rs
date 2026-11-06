//! I5 S3 route and command tests: the invoice and sales receipt editor (UI-12), post and unpost, delete, remove lines,
//! T-08-05. In process, on `Seed::Demo`.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::admin::CommandError;
use karbon_server::db::{self, Seed};
use karbon_server::sales::{self, Kind};
use karbon_server::{admin, router, AppState};
use rusqlite::Connection;
use tower::ServiceExt;

const TODAY: &str = "2026-11-06";

fn demo() -> Connection {
    db::open_in_memory(Seed::Demo).unwrap().0
}

fn app(conn: Connection) -> axum::Router {
    router(AppState::new(conn).with_today(TODAY))
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let (s, h) = (res.status(), res.headers().clone());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (s, h, String::from_utf8_lossy(&body).into_owned())
}

fn form(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("HX-Request", "true")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).header("HX-Request", "true").body(Body::empty()).unwrap()
}

fn id_of(c: &Connection, doc: &str) -> String {
    sales::id_of_doc(c, doc).unwrap().unwrap()
}

/// INV-1048's rows, for "nothing changed".
fn snapshot(c: &Connection, id: &str) -> String {
    c.query_row(
        "SELECT st.doc_number || st.date || st.total_amt || st.balance || st.locked_ind ||
                (SELECT COUNT(*) || COALESCE(SUM(amount), 0) FROM sales_line WHERE sales_transaction_id = st.id)
         FROM sales_transaction st WHERE st.id = ?1",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

#[tokio::test]
async fn t_08_05_locked_transaction_refuses_edit_and_delete() {
    for (doc, kind, path) in [("INV-1048", Kind::Invoice, "/invoices"), ("SR-0311", Kind::Receipt, "/sales-receipts")] {
        // The command layer.
        let mut c = demo();
        let actor = admin::current_user(&c).unwrap().unwrap();
        let id = id_of(&c, doc);
        let before = snapshot(&c, &id);
        let input = sales::transaction_get(&c, &id).unwrap().input;
        let line = input.lines[0].id.clone();
        assert!(matches!(sales::transaction_save(&mut c, kind, &id, &input, false, &actor), Err(CommandError::Locked)));
        assert!(matches!(sales::transaction_delete(&mut c, &id, true), Err(CommandError::Locked)));
        assert!(matches!(sales::transaction_delete(&mut c, &id, false), Err(CommandError::Locked)));
        assert!(matches!(
            sales::lines_remove(&mut c, &id, std::slice::from_ref(&line), true, &actor),
            Err(CommandError::Locked)
        ));
        assert_eq!(snapshot(&c, &id), before);

        // The routes: a hand-made request hits the same refusal (08: "a hand-made request with the session cookie").
        let a = app(c);
        let uri = format!("{path}/{id}");
        let (s, _, text) = send(&a, form("PUT", &uri, "doc_number=X&date=2026-01-01&tax_rate=1")).await;
        assert_eq!(s, StatusCode::CONFLICT, "{doc} save");
        assert!(text.contains("posted"), "{text}");
        let (s, _, _) = send(&a, form("DELETE", &format!("/sales-transactions/{id}?confirm=true"), "")).await;
        assert_eq!(s, StatusCode::CONFLICT, "{doc} delete");
        let lines = format!("/sales-transactions/{id}/lines?line_id={line}&confirm=true");
        let (s, _, _) = send(&a, form("DELETE", &lines, "")).await;
        assert_eq!(s, StatusCode::CONFLICT, "{doc} remove lines");

        // Unpost, edit, post works.
        let (s, h, _) = send(&a, form("POST", &format!("/sales-transactions/{id}/lock"), "lock=false")).await;
        assert_eq!(s, StatusCode::OK);
        assert!(h.get("HX-Redirect").is_some());
    }
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let id = id_of(&c, "INV-1048");
    sales::transaction_lock(&mut c, &id, false, &actor).unwrap();
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    input.customer_memo = "Edited while unposted".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    sales::transaction_lock(&mut c, &id, true, &actor).unwrap();
    let locked: i64 =
        c.query_row("SELECT locked_ind FROM sales_transaction WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(locked, 1);
}

#[tokio::test]
async fn new_invoice_opens_the_editor_unposted() {
    let c = demo();
    let a = app(c);
    let (s, h, _) = send(&a, form("POST", "/invoices", "customer_id=r-hp")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.get("HX-Redirect").unwrap(), "/sales/INV-1052");
    let page = Request::get("/sales/INV-1052").body(Body::empty()).unwrap();
    let (s, _, html) = send(&a, page).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains("id=\"txn-editor\""), "the editor");
    assert!(html.contains(r#"name="due_date" value="2026-11-21""#), "Net 15 from {TODAY}: {html}");
    assert!(html.contains(r#"name="tax_rate" value="5.5""#));
    assert!(html.contains("48 Wharf Street"), "the customer's billing address");

    // A receipt: its own number, no due date, a ship date.
    let (_, h, _) = send(&a, form("POST", "/sales-receipts", "customer_id=r-hp")).await;
    assert_eq!(h.get("HX-Redirect").unwrap(), "/sales/SR-0312");
    let (_, _, html) = send(&a, Request::get("/sales/SR-0312").body(Body::empty()).unwrap()).await;
    assert!(!html.contains(r#"name="due_date""#));
    assert!(html.contains(&format!(r#"name="ship_date" value="{TODAY}""#)));
}

#[tokio::test]
async fn a_customer_is_required() {
    let a = app(demo());
    let (s, _, html) = send(&a, form("POST", "/invoices", "customer_id=")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Choose a customer."));
    let (s, _, _) = send(&a, form("POST", "/invoices", "customer_id=r-nobody")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _, _) = send(&a, form("POST", "/invoices", "customer_id=r-hp&total_amt=5")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "an unknown field");
}

#[tokio::test]
async fn pickers_and_dialogs() {
    let a = app(demo());
    let (s, _, html) = send(&a, get("/pickers/customer?q=halden")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains(r#"name="customer_id" value="r-hp""#) && !html.contains("Bluefin"));
    let (_, _, html) = send(&a, get("/invoices/new?customer_id=r-hp")).await;
    assert!(html.contains(r#"value="r-hp" class="radio radio-xs" checked"#), "{html}");
    assert!(html.contains(r#"hx-post="/invoices""#));
    let (_, _, html) = send(&a, get("/sales-receipts/new")).await;
    assert!(html.contains(r#"hx-post="/sales-receipts""#));
    // The top bar's New menu opens them.
    let page = Request::get("/customers").body(Body::empty()).unwrap();
    let (_, _, html) = send(&a, page).await;
    assert!(html.contains(r#"hx-get="/invoices/new""#) && html.contains(r#"hx-get="/sales-receipts/new""#));
}

/// The editor's own form, as htmx sends it: the page's fields, in the form's order.
fn editor_body(c: &Connection, id: &str, edit: impl FnOnce(&mut sales::TxnInput)) -> String {
    let mut i = sales::transaction_get(c, id).unwrap().input;
    edit(&mut i);
    let mut b = vec![
        ("doc_number", i.doc_number.clone()),
        ("date", i.date.clone()),
        ("due_date", i.due_date.clone()),
        ("term", i.term.clone()),
        ("tax_rate", i.tax_rate.clone()),
        ("ship_amt", i.ship_amt.clone()),
    ];
    for l in &i.lines {
        b.push(("line_id", l.id.clone()));
        b.push(("line_item", l.item_id.clone()));
        b.push(("line_description", l.description.clone()));
        b.push(("line_qty", l.qty.clone()));
        b.push(("line_price", l.unit_price.clone()));
        if l.taxable {
            b.push(("line_taxable", l.id.clone()));
        }
    }
    b.iter().map(|(k, v)| format!("{k}={}", v.replace(' ', "+").replace('$', "%24"))).collect::<Vec<_>>().join("&")
}

#[tokio::test]
async fn editing_through_the_route_answers_the_editor_with_totals() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let input = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, true, &actor).unwrap();

    // Choosing an item fills rate, description and taxable from it (fields 5, 7, 11).
    let body = editor_body(&c, &id, |i| i.lines[0].item_id = "i-prd-print-a1".into());
    let path = format!("/invoices/{id}");
    let a = app(c);
    let (s, h, html) = send(&a, form("PUT", &path, &body)).await;
    assert_eq!(s, StatusCode::OK, "{html}");
    assert_eq!(h.get("HX-Replace-Url").unwrap(), "/sales/INV-1052");
    assert!(html.contains(r#"value="32.50""#), "the item's rate: {html}");
    assert!(html.contains("Large-format prints (A1)"));
    assert!(html.contains("$32.50") && html.contains("$1.79"), "tax on $32.50 at 5.5%: {html}");
    assert!(!html.contains("<html"), "a fragment");

    // A bad value re-renders with the error and writes nothing.
    let (s, _, html) = send(&a, form("PUT", &path, "doc_number=INV-1048&date=nope&tax_rate=5.5")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("INV-1048 is used by another transaction."));
    assert!(html.contains("Enter a date as YYYY-MM-DD."));

    // Add line, and Lock from the editor: it saves first, then posts, and the page opens read only.
    let (s, _, html) =
        send(&a, form("PUT", &path, "doc_number=INV-1052&date=2026-11-06&tax_rate=5.5&add_line=true")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains(r#"name="line_id""#), "the new line");
    let (s, h, _) =
        send(&a, form("PUT", &path, "doc_number=INV-1052&date=2026-11-06&tax_rate=5.5&then_lock=true")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.get("HX-Redirect").unwrap(), "/sales/INV-1052");
    let (_, _, html) = send(&a, Request::get("/sales/INV-1052").body(Body::empty()).unwrap()).await;
    assert!(!html.contains("txn-editor") && html.contains("Unlock"));
}

#[tokio::test]
async fn totals_and_payments_cannot_be_passed_in() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let a = app(c);
    for field in ["total_amt", "balance", "subtotal", "total_tax", "retainer", "locked_ind"] {
        let body = format!("doc_number=INV-1052&date=2026-11-06&tax_rate=5.5&{field}=1");
        let (s, _, text) = send(&a, form("PUT", &format!("/invoices/{id}"), &body)).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{field}");
        assert!(text.contains("unknown field"), "{text}");
    }
}

#[tokio::test]
async fn an_invoice_is_not_a_receipt() {
    let c = demo();
    let id = id_of(&c, "INV-1048");
    let a = app(c);
    let (s, _, _) = send(&a, form("PUT", &format!("/sales-receipts/{id}"), "doc_number=X&tax_rate=1")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = send(&a, form("PUT", "/invoices/nope", "doc_number=X&tax_rate=1")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = send(&a, form("POST", "/sales-transactions/nope/lock", "lock=true")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = send(&a, form("POST", &format!("/sales-transactions/{id}/lock"), "lock=maybe")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn save_never_changes_locked_ind() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let input = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    let locked: i64 =
        c.query_row("SELECT locked_ind FROM sales_transaction WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(locked, 0, "OD-07-05: never posted by a save");
}

#[test]
fn lines_keep_their_ids_and_entry_order_and_removed_ones_go() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    for _ in 0..6 {
        let input = sales::transaction_get(&c, &id).unwrap().input;
        sales::transaction_save(&mut c, Kind::Invoice, &id, &input, true, &actor).unwrap();
    }
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    for (i, l) in input.lines.iter_mut().enumerate() {
        l.description = format!("Line {i}");
    }
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    let names: Vec<_> = input.lines.iter().map(|l| l.description.clone()).collect();
    assert_eq!(names, ["Line 0", "Line 1", "Line 2", "Line 3", "Line 4", "Line 5"], "UUID ids, entry order");
    let kept = input.lines[0].id.clone();
    input.lines.remove(2);
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    let after = sales::transaction_get(&c, &id).unwrap().input;
    assert_eq!(after.lines.len(), 5);
    assert_eq!(after.lines[0].id, kept);
    assert!(after.lines.iter().all(|l| l.description != "Line 2"));
}

#[test]
fn a_line_of_another_transaction_is_refused() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    input.lines.push(sales::LineInput { id: "l-007".into(), ..Default::default() });
    assert!(matches!(
        sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor),
        Err(CommandError::Validation(_))
    ));
}

#[test]
fn an_inactive_item_cannot_be_chosen_and_stays_where_it_is() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let input = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, true, &actor).unwrap();
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    input.lines[0].item_id = "i-svc-survey".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    c.execute("UPDATE item SET active = 0 WHERE id IN ('i-svc-survey', 'i-svc-hourly')", []).unwrap();
    let input = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).expect("the line keeps its item");
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    input.lines[0].item_id = "i-svc-hourly".into();
    assert!(matches!(
        sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor),
        Err(CommandError::Validation(_))
    ));
}

#[test]
fn document_numbers_are_checked() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    for bad in ["INV-1048", "", "   ", "INV-0123456789012345678"] {
        input.doc_number = bad.into();
        assert!(
            matches!(
                sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor),
                Err(CommandError::Validation(_))
            ),
            "{bad:?}"
        );
    }
    input.doc_number = "INV-2000".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    assert_eq!(sales::next_doc_number(&c, Kind::Invoice).unwrap(), "INV-2001");
}

#[test]
fn due_date_comes_from_the_term_when_empty() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-bm", TODAY, &actor).unwrap();
    let due = |c: &Connection| sales::transaction_get(c, &id).unwrap().input.due_date;
    assert_eq!(due(&c), "2026-12-06", "Net 30");
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    input.due_date = "2027-02-01".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    assert_eq!(due(&c), "2027-02-01", "a typed date is kept");
    input.due_date = String::new();
    input.term = "Net 15".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    assert_eq!(due(&c), "2026-11-21");
    input.due_date = String::new();
    input.term = "Due on receipt".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    assert_eq!(due(&c), "");
}

#[tokio::test]
async fn delete_is_two_step_and_removes_the_children() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let input = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, true, &actor).unwrap();
    c.execute(
        "INSERT INTO file_attachment (id, attachment_id, entity_type_id, entity_id) VALUES ('fa-x', 'f-001', 12, ?1)",
        [&id],
    )
    .unwrap();
    let uri = format!("/sales-transactions/{id}");
    let state = AppState::new(c).with_today(TODAY);
    let a = router(state.clone());
    let (s, _, html) = send(&a, form("DELETE", &uri, "")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains("Deletes invoice INV-1052, 1 line and 1 attachment link."), "{html}");
    assert!(html.contains("confirm=true"));
    let (s, h, _) = send(&a, form("DELETE", &format!("{uri}?confirm=true"), "")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.get("HX-Redirect").unwrap(), "/sales");
    let (_, _, html) = send(&a, get("/sales?q=INV-1052")).await;
    assert!(html.contains("0 sales transacti"), "gone: {html}");
}

#[test]
fn delete_leaves_files_and_foreign_keys_whole() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let input = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, true, &actor).unwrap();
    c.execute(
        "INSERT INTO file_attachment (id, attachment_id, entity_type_id, entity_id) VALUES ('fa-x', 'f-001', 12, ?1)",
        [&id],
    )
    .unwrap();
    assert!(matches!(sales::transaction_delete(&mut c, &id, false), Err(CommandError::ConfirmationRequired(_))));
    sales::transaction_delete(&mut c, &id, true).unwrap();
    let n = |sql: &str| c.query_row(sql, [&id], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(n("SELECT COUNT(*) FROM sales_transaction WHERE id = ?1"), 0);
    assert_eq!(n("SELECT COUNT(*) FROM sales_line WHERE sales_transaction_id = ?1"), 0);
    assert_eq!(n("SELECT COUNT(*) FROM invoice WHERE id = ?1"), 0);
    assert_eq!(n("SELECT COUNT(*) FROM file_attachment WHERE entity_id = ?1"), 0);
    let files: i64 = c.query_row("SELECT COUNT(*) FROM file WHERE id = 'f-001'", [], |r| r.get(0)).unwrap();
    assert_eq!(files, 1, "the file stays");
    assert!(db::foreign_key_check(&c).unwrap().is_empty());
}

#[test]
fn a_transaction_with_payments_is_not_deleted() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let id = id_of(&c, "INV-1048");
    sales::transaction_lock(&mut c, &id, false, &actor).unwrap();
    match sales::transaction_delete(&mut c, &id, true) {
        Err(CommandError::Refused(m)) => assert!(m.contains("payments"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn remove_lines_is_two_step_and_stays_on_one_transaction() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let id = id_of(&c, "INV-1048");
    sales::transaction_lock(&mut c, &id, false, &actor).unwrap();
    let other = id_of(&c, "INV-1051");
    sales::transaction_lock(&mut c, &other, false, &actor).unwrap();
    let a = app(c);
    let uri = format!("/sales-transactions/{id}/lines");
    let (s, _, html) = send(&a, form("DELETE", &format!("{uri}?line_id=l-010"), "")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains("Removes 1 line and recomputes the totals."), "{html}");
    let (s, _, _) = send(&a, form("DELETE", &format!("{uri}?line_id=l-010&line_id=l-012&confirm=true"), "")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "a line of another transaction");
    let (s, _, _) = send(&a, form("DELETE", &format!("{uri}?confirm=true"), "")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "no line");
    let (s, h, _) = send(&a, form("DELETE", &format!("{uri}?line_id=l-010&confirm=true"), "")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.get("HX-Redirect").unwrap(), "/sales/INV-1048");
    // The taxable line is gone: no tax, $3,050.00 - $130.00, less the $1,000.00 payment.
    let (_, _, html) = send(&a, Request::get("/sales/INV-1048").body(Body::empty()).unwrap()).await;
    assert!(html.contains("$1,920.00"), "{html}");
}

#[tokio::test]
async fn a_draft_page_shows_status_from_the_data_not_the_column() {
    let c = demo();
    // Stale column: the seed says "Partially paid"; on this date INV-1048 is past due.
    let a = app(c);
    let (_, _, html) = send(&a, Request::get("/sales/INV-1048").body(Body::empty()).unwrap()).await;
    assert!(html.contains("Overdue"), "{html}");
}

#[tokio::test]
async fn the_list_offers_edit_and_the_header_offers_new() {
    let a = app(demo());
    let (_, _, html) = send(&a, Request::get("/sales").body(Body::empty()).unwrap()).await;
    assert!(!html.contains("New invoice") && !html.contains("New sales receipt"), "the page repeats no create button");
    assert!(html.contains(r#"aria-label="Unpost INV-1051""#));
    assert!(html.contains(r#"aria-label="Posted""#));
}

// ---------------------------------------------------------------------------------------------------------------
// S4: T-08-06, AP-06, the payment card, Apply Retainer
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn t_08_06_invoice_save_cannot_write_payments() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-hp", TODAY, &actor).unwrap();
    let count = |c: &Connection| -> (i64, i64) {
        c.query_row("SELECT (SELECT COUNT(*) FROM payment), (SELECT COUNT(*) FROM payment_line)", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
    };
    let before = count(&c);
    let state = AppState::new(c).with_today(TODAY);
    let a = router(state.clone());
    let pre = before;
    for field in ["payment_amount", "payment_line", "id_payment", "amount_paid", "line_invoice"] {
        let body = format!("doc_number=INV-1052&date={TODAY}&tax_rate=5.5&{field}=100");
        let (s, _, text) = send(&a, form("PUT", &format!("/invoices/{id}"), &body)).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "PUT {field}");
        assert!(text.contains("unknown field"), "{text}");
        let (s, _, _) = send(&a, form("POST", "/invoices", &format!("customer_id=r-hp&{field}=100"))).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "POST {field}");
    }
    // The list still counts the seed's four payments: nothing was written.
    let (s, _, html) = send(&a, get("/payments")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(pre.0, 4);
    assert!(html.contains("of 4"), "{html}");
}

/// AP-06: no other command writes the payment tables. The importer and the seed load exports; they are not commands.
#[test]
fn ap_06_only_payments_rs_writes_payment_tables() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(files.len() > 20);
    for f in files {
        let name = f.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
        if name == "payments.rs" || name.starts_with("import") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap().to_lowercase();
        for table in ["payment", "payment_line"] {
            for verb in ["insert into", "update", "delete from", "insert or replace into"] {
                assert!(!text.contains(&format!("{verb} {table} ")), "{name} writes {table}");
            }
        }
    }
}

#[tokio::test]
async fn the_payment_card_from_an_invoice() {
    let c = demo();
    let inv = id_of(&c, "INV-1048");
    let a = app(c);
    // Y3: Record payment shows on the posted invoice.
    let (_, _, html) = send(&a, Request::get("/sales/INV-1048").body(Body::empty()).unwrap()).await;
    assert!(html.contains(&format!("/payments/new?invoice_id={inv}")), "{html}");
    assert!(html.contains("Check #2214"), "its payment is listed");
    // Y1: amount = balance, date today, one line at the balance.
    let (s, _, html) = send(&a, get(&format!("/payments/new?invoice_id={inv}"))).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains("Payment from Halden &#38; Pryce Architects"), "{html}");
    assert!(html.contains(r#"name="amount" value="2057.15""#), "{html}");
    assert!(html.contains(&format!(r#"name="date" value="{TODAY}""#)));
    assert!(html.contains(r#"name="line_amount" value="2057.15""#));
    // Y2: the customer's other unpaid invoice is offered, a paid one is not, and neither is another customer's.
    assert!(html.contains("INV-1042") && !html.contains("INV-1027") && !html.contains("INV-1051"));

    let body = format!("customer_id=r-hp&date={TODAY}&amount=100&method=Cash&line_invoice={inv}&line_amount=100");
    let (s, h, _) = send(&a, form("POST", "/payments", &body)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.get("HX-Refresh").unwrap(), "true");
    let (_, _, html) = send(&a, Request::get("/sales/INV-1048").body(Body::empty()).unwrap()).await;
    assert!(html.contains("$1,957.15"), "{html}");

    // Field errors come back in the card; another customer's invoice and an unknown field are refused.
    let over = format!("customer_id=r-hp&date={TODAY}&amount=1&line_invoice={inv}&line_amount=2");
    let (s, _, html) = send(&a, form("POST", "/payments", &over)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("The total allocated cannot exceed the Payment amount."));
    let wrong = format!("customer_id=r-bm&date={TODAY}&amount=1&line_invoice={inv}&line_amount=1");
    let (s, _, html) = send(&a, form("POST", "/payments", &wrong)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Only this customer"), "{html}");
    let (s, _, _) = send(&a, form("POST", "/payments", "customer_id=r-hp&amount=1&total=9")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _, _) = send(&a, form("POST", "/payments", "customer_id=&amount=1")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "BR-SALES-09: a customer");
    let (s, _, _) = send(&a, get("/payments/new?invoice_id=nope")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_invoice_with_no_customer_takes_no_payment() {
    let c = demo();
    let inv = id_of(&c, "INV-1048");
    c.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    c.execute("UPDATE sales_transaction SET id_customer = NULL WHERE id = ?1", [&inv]).unwrap();
    let a = app(c);
    let (s, _, text) = send(&a, get(&format!("/payments/new?invoice_id={inv}"))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text.contains("doesn't have a customer"), "{text}");
}

#[tokio::test]
async fn a_payment_opens_again_and_a_sales_receipt_is_not_a_line() {
    let c = demo();
    let sr = id_of(&c, "SR-0311");
    let a = app(c);
    let (s, _, html) = send(&a, get("/payments/pay-004/edit")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains(r#"hx-put="/payments/pay-004""#) && html.contains(r#"value="1000.00""#), "{html}");
    assert!(html.contains("Check"), "a stored method off the list stays");
    let (s, _, _) = send(&a, get("/payments/nope/edit")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let body = format!("customer_id=r-hp&amount=1&line_invoice={sr}&line_amount=1");
    let (s, _, html) = send(&a, form("POST", "/payments", &body)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Only this customer"));
    let (s, _, _) = send(&a, form("PUT", "/payments/nope", "customer_id=r-hp&amount=1")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // A draft has no Record payment (Y3); a receipt has no payments panel.
    let (_, _, html) = send(&a, Request::get("/sales/SR-0311").body(Body::empty()).unwrap()).await;
    assert!(!html.contains("Record payment"));
}

#[tokio::test]
async fn apply_retainer_through_the_route() {
    let mut c = demo();
    let actor = admin::current_user(&c).unwrap().unwrap();
    let (id, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-cs", TODAY, &actor).unwrap();
    let input = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, true, &actor).unwrap();
    let mut input = sales::transaction_get(&c, &id).unwrap().input;
    input.lines[0].item_id = "i-svc-design".into();
    input.lines[0].qty = "1".into();
    input.lines[0].unit_price = "300.00".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &input, false, &actor).unwrap();
    let inv = id_of(&c, "INV-1048");
    let a = app(c);
    let uri = format!("/invoices/{id}/apply-retainer");
    let (_, _, html) = send(&a, Request::get("/sales/INV-1052").body(Body::empty()).unwrap()).await;
    assert!(html.contains("Retainer available") && html.contains("$2,000.00"), "{html}");
    assert!(!html.contains("Record payment"), "Y3: a draft takes no payment from its page");
    let (s, h, html) = send(&a, form("POST", &uri, "")).await;
    assert_eq!(s, StatusCode::OK, "{html}");
    assert!(h.get("HX-Trigger").is_some());
    assert!(html.contains("Retainer applied") && html.contains("−$300.00"), "{html}");
    let (s, _, text) = send(&a, form("POST", &uri, "")).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(text.contains("Nothing is due"), "{text}");
    let (s, _, _) = send(&a, form("POST", &format!("/invoices/{inv}/apply-retainer"), "")).await;
    assert_eq!(s, StatusCode::CONFLICT, "AP-05");
    let (s, _, _) = send(&a, form("POST", "/invoices/nope/apply-retainer", "")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = send(&a, form("POST", &uri, "payment_amount=1")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}
