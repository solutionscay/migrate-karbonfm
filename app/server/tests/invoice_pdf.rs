//! S-02: the invoice view and PDF on the demo seed. The totals printed come from karbon-money and must equal the
//! seed (03 I5 exit: "PDF matches totals").

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_money::Cents;
use karbon_server::invoice::{Invoice, LONG_FIXTURE};
use karbon_server::pdf::{invoice_pdf, Engine};
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
    let inv = Invoice::load(&conn, LONG_FIXTURE).unwrap();
    assert_eq!(inv.item_lines().count(), 60);
    // Independent integer arithmetic: whole quantities times cents.
    let sum: i64 = inv.item_lines().map(|l| l.qty.unwrap().scaled() / 10_000 * l.rate.0).sum();
    let taxable: i64 = inv.item_lines().filter(|l| l.taxable).map(|l| l.amount.0).sum();
    assert_eq!(inv.subtotal_before_retainer().0, sum);
    assert_eq!(inv.totals.total_tax.0, (taxable * 550 + 5_000) / 10_000, "5.5%, half up on positive amounts");
    assert_eq!(inv.balance().0, sum + inv.totals.total_tax.0 - 50_000 - 100_000);
}

#[test]
fn both_engines_write_a_pdf() {
    let conn = db::open_demo(None).unwrap();
    for doc in ["INV-1048", LONG_FIXTURE] {
        let inv = Invoice::load(&conn, doc).unwrap();
        for engine in [Engine::Krilla, Engine::Printpdf] {
            let pdf = invoice_pdf(&inv, engine);
            assert!(pdf.starts_with(b"%PDF-"), "{doc} {engine:?}");
            assert!(pdf.len() < 60_000, "{doc} {engine:?}: fonts are subset ({} bytes)", pdf.len());
        }
    }
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
    assert!(html.contains("data-print"));
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
    for uri in
        ["/sales/INV-1048/invoice.pdf", "/sales/INV-1048/invoice.pdf?engine=printpdf", "/sales/FIXTURE-60/invoice.pdf"]
    {
        let (status, ctype, body) = get(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(ctype, "application/pdf");
        assert!(body.starts_with(b"%PDF-"));
    }
    assert_eq!(get("/sales/INV-0000").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get("/sales/INV-0000/invoice.pdf").await.0, StatusCode::NOT_FOUND);
}

/// The Host and Origin guard (AP-13, S-01) covers the invoice routes and the print hook: GET needs the server's
/// own Host (no DNS rebinding); POST also needs its own Origin (no cross-site writes).
#[tokio::test]
async fn origin_guard_covers_the_invoice_routes_and_the_print_hook() {
    const HOST: &str = "127.0.0.1:4321";
    const ORIGIN: &str = "http://127.0.0.1:4321";
    let calls = Arc::new(Mutex::new(0));
    let guarded = || {
        let counter = calls.clone();
        router(AppState::new(db::open_demo(None).unwrap()).with_origin(ORIGIN).with_print(Arc::new(move || {
            *counter.lock().unwrap() += 1;
            Ok(())
        })))
    };
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

    for uri in ["/sales/INV-1048", "/sales/INV-1048/invoice.pdf", "/sales/FIXTURE-60/invoice.pdf?engine=printpdf"] {
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

    assert_eq!(send(req("POST", "/print", Some(HOST), Some("https://evil.example"))).await, StatusCode::FORBIDDEN);
    assert_eq!(send(req("POST", "/print", Some(HOST), Some("null"))).await, StatusCode::FORBIDDEN, "sandboxed page");
    assert_eq!(send(req("POST", "/print", Some(HOST), None)).await, StatusCode::FORBIDDEN, "no Origin");
    assert_eq!(
        send(req("POST", "/print", Some("evil.example:4321"), Some(ORIGIN))).await,
        StatusCode::MISDIRECTED_REQUEST
    );
    assert_eq!(*calls.lock().unwrap(), 0, "no refused request reached the window");
    assert_eq!(send(req("POST", "/print", Some(HOST), Some(ORIGIN))).await, StatusCode::NO_CONTENT);
    assert_eq!(*calls.lock().unwrap(), 1);
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
