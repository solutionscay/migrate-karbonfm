//! The invoice view and PDF on the demo seed (S-02, S5). The totals printed come from karbon-money and must equal the
//! stored ones (03 I5 exit: "PDF matches totals").

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_money::Cents;
use karbon_server::invoice::{long_fixture, Invoice};
use karbon_server::pdf::{invoice_pdf, page_texts};
use karbon_server::{db, router, AppState, SaveHook};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

async fn get(uri: &str) -> (StatusCode, String, Vec<u8>) {
    let app = router(AppState::new(db::open_demo(None).unwrap()));
    let res = app.oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let ctype = res.headers().get(header::CONTENT_TYPE).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    (status, ctype, res.into_body().collect().await.unwrap().to_bytes().to_vec())
}

#[test]
fn inv_1048_totals_match_the_design_and_the_seed() {
    let conn = db::open_demo(None).unwrap();
    let inv = Invoice::load(&conn, "INV-1048").unwrap();
    assert_eq!(inv.item_lines().count(), 4);
    assert_eq!(inv.subtotal_before_retainer(), Cents(355_000), "$3,550.00");
    assert_eq!(inv.totals.taxable_subtotal, Cents(13_000));
    assert_eq!(inv.totals.total_tax, Cents(715), "$7.15");
    assert_eq!(inv.total_before_retainer(), Cents(355_715), "$3,557.15");
    assert_eq!(inv.retainer(), Cents(-50_000), "-$500.00");
    assert_eq!(inv.paid(), Cents(100_000), "$1,000.00");
    assert_eq!(inv.balance(), Cents(205_715), "$2,057.15");
    let (total_amt, balance): (i64, i64) = conn
        .query_row("SELECT total_amt, balance FROM sales_transaction WHERE doc_number = 'INV-1048'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((inv.totals.total_amt.0, inv.balance().0), (total_amt, balance));
}

#[test]
fn every_seed_invoice_recomputes_to_its_stored_totals() {
    let conn = db::open_demo(None).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT doc_number, subtotal, total_tax, total_amt, balance FROM sales_transaction WHERE invoice_ind = 1",
        )
        .unwrap();
    let rows: Vec<(String, i64, i64, i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(rows.len() >= 10);
    for (doc, subtotal, tax, total, balance) in rows {
        let t = Invoice::load(&conn, &doc).unwrap().totals;
        assert_eq!((t.subtotal.0, t.total_tax.0, t.total_amt.0, t.balance.0), (subtotal, tax, total, balance), "{doc}");
    }
}

#[test]
fn long_fixture_has_60_lines_and_exact_totals() {
    let conn = db::open_demo(None).unwrap();
    let inv = long_fixture(Invoice::load(&conn, "INV-1048").unwrap(), 60);
    assert_eq!(inv.item_lines().count(), 60);
    // Independent integer arithmetic: whole quantities times cents.
    let sum: i64 = inv.item_lines().map(|l| l.qty.unwrap().scaled() / 10_000 * l.rate.0).sum();
    let taxable: i64 = inv.item_lines().filter(|l| l.taxable).map(|l| l.amount.0).sum();
    assert_eq!(inv.subtotal_before_retainer().0, sum);
    assert_eq!(inv.totals.total_tax.0, (taxable * 550 + 5_000) / 10_000, "5.5%, half up on positive amounts");
    assert_eq!(inv.balance().0, sum + inv.totals.total_tax.0 - 50_000 - 100_000);
}

#[test]
fn the_pdf_is_a_small_file() {
    let conn = db::open_demo(None).unwrap();
    let short = Invoice::load(&conn, "INV-1048").unwrap();
    for inv in [long_fixture(short.clone(), 60), short] {
        let pdf = invoice_pdf(&inv);
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() < 60_000, "fonts are subset ({} bytes)", pdf.len());
    }
}

/// Every text run of the PDF.
fn printed(inv: &Invoice) -> Vec<String> {
    page_texts(inv).into_iter().flatten().collect()
}

/// `label` is printed with `value` right after it, the way a totals row draws.
fn row_shows(texts: &[String], label: &str, value: &str) -> bool {
    texts.windows(2).any(|w| w[0] == label && w[1] == value)
}

#[test]
fn pdf_matches_totals_inv_1048() {
    let conn = db::open_demo(None).unwrap();
    let t = printed(&Invoice::load(&conn, "INV-1048").unwrap());
    assert!(row_shows(&t, "Subtotal", "$3,550.00"), "{t:?}");
    assert!(row_shows(&t, "Sales tax (5.5% on taxable)", "$7.15"));
    assert!(row_shows(&t, "Total", "$3,557.15"));
    assert!(row_shows(&t, "Retainer applied", "−$500.00"));
    assert!(row_shows(&t, "Payments", "−$1,000.00"));
    assert!(row_shows(&t, "Balance due", "$2,057.15"));
    assert!(!t.iter().any(|s| s == "Shipping"), "no shipping, no row");
}

/// A router and a second handle on the same data.
fn app_and_conn() -> (axum::Router, rusqlite::Connection, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let path = db::database_path(tmp.path());
    let (conn, _) = db::open(&path, db::Seed::Demo).unwrap();
    (router(AppState::new(conn)), rusqlite::Connection::open(&path).unwrap(), tmp)
}

async fn send_form(app: &axum::Router, method: &str, uri: &str, body: &str) -> StatusCode {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("HX-Request", "true")
        .body(Body::from(body.to_owned()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

/// Percent-encodes a form value.
fn enc(v: &str) -> String {
    v.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Unposts INV-1048 and saves its editor form with line l-009 at quantity 8 (the edit that moves every total) and the
/// given shipping. Returns its id.
async fn edit_inv_1048(app: &axum::Router, check: &rusqlite::Connection, ship_amt: &str) -> String {
    let id: String =
        check.query_row("SELECT id FROM sales_transaction WHERE doc_number = 'INV-1048'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        send_form(app, "POST", &format!("/sales-transactions/{id}/lock"), "lock=false&confirm=true").await,
        StatusCode::OK
    );
    let i = karbon_server::sales::transaction_get(check, &id).unwrap().input;
    let mut body = format!(
        "doc_number={}&date={}&due_date={}&term={}&tax_rate={}&ship_amt={}",
        enc(&i.doc_number),
        enc(&i.date),
        enc(&i.due_date),
        enc(&i.term),
        enc(&i.tax_rate),
        enc(ship_amt)
    );
    for l in &i.lines {
        let qty = if l.id == "l-009" { "8" } else { l.qty.as_str() };
        body.push_str(&format!(
            "&line_id={}&line_item={}&line_description={}&line_qty={}&line_price={}",
            enc(&l.id),
            enc(&l.item_id),
            enc(&l.description),
            enc(qty),
            enc(&l.unit_price)
        ));
        if l.taxable {
            body.push_str(&format!("&line_taxable={}", enc(&l.id)));
        }
    }
    assert_eq!(send_form(app, "PUT", &format!("/invoices/{id}"), &body).await, StatusCode::OK, "{body}");
    id
}

/// The stored (subtotal, tax, total, retainer, balance).
fn stored(check: &rusqlite::Connection, id: &str) -> (i64, i64, i64, i64, i64) {
    check
        .query_row(
            "SELECT subtotal, total_tax, total_amt, retainer, balance FROM sales_transaction WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
}

/// The model and the PDF's rows equal what the commands stored.
fn assert_pdf_equals_stored(check: &rusqlite::Connection, id: &str, doc: &str) {
    let inv = Invoice::load(check, doc).unwrap();
    let (subtotal, tax, total, retainer, balance) = stored(check, id);
    let t = inv.totals;
    assert_eq!(
        (t.subtotal.0, t.total_tax.0, t.total_amt.0, t.retainer.0, t.balance.0),
        (subtotal, tax, total, retainer, balance)
    );
    let texts = printed(&inv);
    assert!(row_shows(&texts, "Total", &Cents(total - retainer).to_string()), "{texts:?}");
    assert!(row_shows(&texts, "Balance due", &Cents(balance).to_string()), "{texts:?}");
    assert!(texts.iter().any(|s| *s == Cents(tax).to_string()), "{texts:?}");
}

/// Edit, Apply Retainer, post, pay: after each step the PDF prints the stored totals (03 I5 exit).
#[tokio::test]
async fn pdf_matches_totals_after_edit() {
    let (app, check, _tmp) = app_and_conn();
    let before = stored(
        &check,
        &check
            .query_row("SELECT id FROM sales_transaction WHERE doc_number = 'INV-1048'", [], |r| r.get::<_, String>(0))
            .unwrap(),
    );
    let id = edit_inv_1048(&app, &check, "0").await;
    let after = stored(&check, &id);
    assert_ne!(before, after, "the edit moved the totals");
    assert_pdf_equals_stored(&check, &id, "INV-1048");

    // Apply Retainer (S4). Unposted, INV-1048's own -$500.00 line is already spoken for: $2,000.00 less it leaves $1,500.00.
    let uri = format!("/invoices/{id}/apply-retainer");
    assert_eq!(send_form(&app, "POST", &uri, "").await, StatusCode::OK);
    let applied = stored(&check, &id);
    assert_eq!(applied.3, after.3 - 150_000, "the credit line");
    assert_eq!(applied.4, after.4 - 150_000, "the balance due");
    assert_pdf_equals_stored(&check, &id, "INV-1048");
    let inv = Invoice::load(&check, "INV-1048").unwrap();
    assert!(row_shows(&printed(&inv), "Retainer applied", "−$2,000.00"), "{:?}", printed(&inv));

    // Post it, then a payment (S4): only the balance moves.
    let lock = format!("/sales-transactions/{id}/lock");
    assert_eq!(send_form(&app, "POST", &lock, "lock=true").await, StatusCode::OK);
    assert_pdf_equals_stored(&check, &id, "INV-1048");
    let body = format!("customer_id=r-hp&date=2026-11-06&amount=50&line_invoice={id}&line_amount=50");
    assert_eq!(send_form(&app, "POST", "/payments", &body).await, StatusCode::OK);
    let paid = stored(&check, &id);
    assert_eq!(paid.4, applied.4 - 5_000);
    assert_eq!((paid.0, paid.1, paid.2, paid.3), (applied.0, applied.1, applied.2, applied.3), "the rest stays");
    assert_pdf_equals_stored(&check, &id, "INV-1048");
    let inv = Invoice::load(&check, "INV-1048").unwrap();
    assert!(row_shows(&printed(&inv), "Payments", "−$1,050.00"), "{:?}", printed(&inv));
}

#[tokio::test]
async fn pdf_matches_totals_with_shipping() {
    let (app, check, _tmp) = app_and_conn();
    let id = edit_inv_1048(&app, &check, "25.50").await;
    let (_, tax, total, retainer, _) = stored(&check, &id);
    let inv = Invoice::load(&check, "INV-1048").unwrap();
    assert_eq!(inv.ship_amt, Cents(2550));
    assert_eq!(inv.total_before_retainer().0, inv.subtotal_before_retainer().0 + tax + 2550, "the total includes it");
    assert_eq!(total - retainer, inv.total_before_retainer().0);
    let texts = printed(&inv);
    assert!(row_shows(&texts, "Shipping", "$25.50"), "{texts:?}");
    assert_pdf_equals_stored(&check, &id, "INV-1048");
    // Posted, the page draws from the same model.
    assert_eq!(send_form(&app, "POST", &format!("/sales-transactions/{id}/lock"), "lock=true").await, StatusCode::OK);
    let (_, html) = page_html(&app, "/sales/INV-1048").await;
    assert!(html.contains(r#"<dt class="muted">Shipping</dt><dd class="num">$25.50</dd>"#), "the page's row");
}

async fn page_html(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let res = app.clone().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    (status, String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap())
}

#[test]
fn the_stored_address_wins_and_an_older_row_falls_back_to_the_party() {
    let conn = db::open_demo(None).unwrap();
    let party = Invoice::load(&conn, "INV-1048").unwrap().bill_to;
    assert!(!party.is_empty(), "the seed has no stored address: the party's");
    conn.execute(
        "UPDATE sales_transaction SET billing_address_display = '9 Stored St\nBoston, MA 02101',
                                      shipping_address_display = '1 Dock Rd' WHERE doc_number = 'INV-1048'",
        [],
    )
    .unwrap();
    let inv = Invoice::load(&conn, "INV-1048").unwrap();
    assert_eq!(inv.bill_to, ["9 Stored St", "Boston, MA 02101"]);
    assert_eq!(inv.ship_to, ["1 Dock Rd"]);
    let t = printed(&inv);
    assert!(t.iter().any(|s| s == "9 Stored St") && t.iter().any(|s| s == "1 Dock Rd"));
}

#[tokio::test]
async fn a_sales_receipt_is_named_so_has_no_balance_and_no_pdf() {
    let (status, _, body) = get("/sales/SR-0311").await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Sales receipt SR-0311"), "F2");
    assert!(!html.contains("Invoice SR-0311") && !html.contains("Invoice date") && !html.contains("Due date"));
    assert!(html.contains("Balance due</dt>"));
    assert!(html.contains(r#"inv-primary">$0.00</dd>"#), "X12: paid when sold");
    assert!(!html.contains("data-pdf-open") && !html.contains("pdf-preview"), "no button");
    assert_eq!(get("/sales/SR-0311/invoice.pdf").await.0, StatusCode::NOT_FOUND);

    let conn = db::open_demo(None).unwrap();
    assert_eq!(Invoice::load(&conn, "SR-0311").unwrap().balance(), Cents::ZERO);
    let shell = router(AppState::new(conn).with_save(Arc::new(|_, _| Ok(None))));
    let req = Request::post("/sales/SR-0311/save-pdf").body(Body::empty()).unwrap();
    assert_eq!(shell.oneshot(req).await.unwrap().status(), StatusCode::NOT_FOUND);
}

/// F4: the S-02 comparison and the webview print are gone.
#[tokio::test]
async fn the_webview_print_and_the_comparison_are_gone() {
    assert_eq!(get("/sales/FIXTURE-60").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get("/sales/FIXTURE-60/invoice.pdf").await.0, StatusCode::NOT_FOUND);
    let app = router(AppState::new(db::open_demo(None).unwrap()));
    let res = app.oneshot(Request::post("/print").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(!karbon_server::ROUTES.iter().any(|(_, p)| *p == "/print"));
    let (_, _, plain) = get("/sales/INV-1048/invoice.pdf").await;
    let (_, _, asked) = get("/sales/INV-1048/invoice.pdf?engine=printpdf").await;
    assert_eq!(plain.len(), asked.len(), "?engine= no longer picks a writer");
    let (_, _, page) = get("/sales/INV-1048").await;
    let html = String::from_utf8(page).unwrap();
    assert!(!html.contains("data-print") && !html.contains("Print (webview)"));
    assert!(!include_str!("../assets/app.js").contains("/print"));
}

#[tokio::test]
async fn invoice_page_shows_the_totals() {
    let (status, _, body) = get("/sales/INV-1048").await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    for s in ["$3,550.00", "$7.15", "$3,557.15", "−$500.00", "−$1,000.00", "$2,057.15", "Halden &#38; Pryce Architects"]
    {
        assert!(html.contains(s), "{s}");
    }
}

/// The PDF never replaces the app page: the PDF button opens the in-app preview, and in a browser (no desktop shell)
/// the only link to the file is a download link. A plain link would strand the desktop window on the bare PDF.
#[tokio::test]
async fn pdf_button_opens_the_preview_not_the_file() {
    let (_, _, body) = get("/sales/INV-1048").await;
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains(r#"data-pdf-open="pdf-preview""#));
    assert!(html.contains(r#"<dialog id="pdf-preview""#) && html.contains(r#"data-src="/sales/INV-1048/invoice.pdf""#));
    assert!(html.contains(r#"aria-label="Close""#));
    let link = r#"href="/sales/INV-1048/invoice.pdf""#;
    assert_eq!(html.matches(link).count(), 1);
    assert!(html.contains(&format!(r#"{link} download="INV-1048.pdf""#)), "browser: a download link");
    assert!(!html.contains("save-pdf"), "no shell, no save route");

    let shell = router(AppState::new(db::open_demo(None).unwrap()).with_save(Arc::new(|_, _| Ok(None))));
    let res = shell.oneshot(Request::get("/sales/INV-1048").body(Body::empty()).unwrap()).await.unwrap();
    let html = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(html.contains(r#"hx-post="/sales/INV-1048/save-pdf""#), "desktop: save through the shell");
    assert!(!html.contains(link), "desktop: no link to the file at all");
}

/// The preview frames the PDF, so the PDF lets pages of this server frame it; the app pages still refuse framing.
#[tokio::test]
async fn pdf_may_be_framed_by_the_app_only() {
    let app = router(AppState::new(db::open_demo(None).unwrap()));
    let csp =
        |res: &axum::response::Response| res.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap().to_owned();
    let res = app.clone().oneshot(Request::get("/sales/INV-1048/invoice.pdf").body(Body::empty()).unwrap()).await;
    assert_eq!(csp(&res.unwrap()), "frame-ancestors 'self'");
    let res = app.oneshot(Request::get("/sales/INV-1048").body(Body::empty()).unwrap()).await.unwrap();
    assert!(csp(&res).contains("frame-ancestors 'none'"));
}

#[tokio::test]
async fn pdf_routes() {
    let (status, ctype, body) = get("/sales/INV-1048/invoice.pdf").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ctype, "application/pdf");
    assert!(body.starts_with(b"%PDF-"));
    assert_eq!(get("/sales/INV-0000").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get("/sales/INV-0000/invoice.pdf").await.0, StatusCode::NOT_FOUND);
}

/// The Host and Origin guard (AP-13, S-01) covers the invoice routes: GET needs the server's own Host (no DNS
/// rebinding); POST also needs its own Origin (no cross-site writes). Save PDF has its own test below.
#[tokio::test]
async fn origin_guard_covers_the_invoice_routes() {
    const HOST: &str = "127.0.0.1:4321";
    const ORIGIN: &str = "http://127.0.0.1:4321";
    let guarded = || router(AppState::new(db::open_demo(None).unwrap()).with_origin(ORIGIN));
    let send = |req: Request<Body>| async { guarded().oneshot(req).await.unwrap().status() };
    let req = |method: &str, uri: &str, host: Option<&str>, origin: Option<&str>| {
        let mut r = Request::builder().method(method).uri(uri);
        if let Some(h) = host {
            r = r.header(header::HOST, h);
        }
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        r.body(Body::empty()).unwrap()
    };

    for uri in ["/sales/INV-1048", "/sales/INV-1048/invoice.pdf"] {
        assert_eq!(send(req("GET", uri, Some(HOST), None)).await, StatusCode::OK, "{uri}");
        // A link or the webview's own navigation may carry the page's Origin; it is the same origin.
        assert_eq!(send(req("GET", uri, Some(HOST), Some(ORIGIN))).await, StatusCode::OK, "{uri}");
        assert_eq!(send(req("GET", uri, Some("evil.example:4321"), None)).await, StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(send(req("GET", uri, Some("localhost:4321"), None)).await, StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(send(req("GET", uri, None, None)).await, StatusCode::MISDIRECTED_REQUEST, "no Host");
        // Writes to a read-only route are refused by the guard before the router answers 405.
        assert_eq!(send(req("POST", uri, Some(HOST), Some("https://evil.example"))).await, StatusCode::FORBIDDEN);
        assert_eq!(send(req("POST", uri, Some(HOST), Some(ORIGIN))).await, StatusCode::METHOD_NOT_ALLOWED);
    }
}

/// "Save PDF…" posts to the server, which hands the bytes to the desktop shell's save hook (native save dialog). Only
/// a same-origin POST reaches the hook: the guard refuses other origins, no Origin, and other hosts.
#[tokio::test]
async fn save_pdf_goes_through_the_shell_for_same_origin_posts_only() {
    const HOST: &str = "127.0.0.1:4321";
    const ORIGIN: &str = "http://127.0.0.1:4321";
    type Call = (String, Vec<u8>);
    let calls: Arc<Mutex<Vec<Call>>> = Arc::default();
    let app = |answer: Result<Option<PathBuf>, String>| {
        let calls = calls.clone();
        let save: SaveHook = Arc::new(move |name, bytes| {
            calls.lock().unwrap().push((name, bytes));
            answer.clone()
        });
        router(AppState::new(db::open_demo(None).unwrap()).with_origin(ORIGIN).with_save(save))
    };
    let req = |method: &str, uri: &str, host: &str, origin: Option<&str>| {
        let mut r = Request::builder().method(method).uri(uri).header(header::HOST, host);
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        r.body(Body::empty()).unwrap()
    };
    let uri = "/sales/INV-1048/save-pdf";
    let saved = || Ok(Some(PathBuf::from("/Users/me/Documents/Halden INV-1048.pdf")));

    for origin in [Some("https://evil.example"), Some("null"), None] {
        let res = app(saved()).oneshot(req("POST", uri, HOST, origin)).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{origin:?}");
    }
    let res = app(saved()).oneshot(req("POST", uri, "evil.example:4321", Some(ORIGIN))).await.unwrap();
    assert_eq!(res.status(), StatusCode::MISDIRECTED_REQUEST);
    let res = app(saved()).oneshot(req("GET", uri, HOST, None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "a link cannot trigger a save");
    let res = app(saved()).oneshot(req("POST", "/sales/INV-0000/save-pdf", HOST, Some(ORIGIN))).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(calls.lock().unwrap().is_empty(), "no refused request reached the shell");

    // Saved: the hook got the suggested name and the PDF; the page gets a toast with the name the user chose.
    let res = app(saved()).oneshot(req("POST", uri, HOST, Some(ORIGIN))).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let trigger = res.headers()["hx-trigger"].to_str().unwrap().to_owned();
    assert!(trigger.contains("karbon-toast") && trigger.contains("Saved Halden INV-1048.pdf"), "{trigger}");
    {
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "INV-1048.pdf");
        assert!(calls[0].1.starts_with(b"%PDF-"));
    }
    // Cancelled: nothing to show. A failed write is a server error.
    let res = app(Ok(None)).oneshot(req("POST", uri, HOST, Some(ORIGIN))).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app(Err("disk full".into())).oneshot(req("POST", uri, HOST, Some(ORIGIN))).await.unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // The dev server in a browser has no shell: 501 (its page uses a download link instead).
    let browser = router(AppState::new(db::open_demo(None).unwrap()).with_origin(ORIGIN));
    let res = browser.oneshot(req("POST", uri, HOST, Some(ORIGIN))).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_IMPLEMENTED);
}
