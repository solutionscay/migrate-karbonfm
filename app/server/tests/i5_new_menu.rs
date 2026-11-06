//! The header's New menu is the one place for a page's create action (user rule, 2026-11-06). The I5 screens repeat no
//! create button; an action the header cannot do, because it presets the record that is open, stays on the page.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::db::{self, Seed};
use karbon_server::{router, AppState};
use tower::ServiceExt;

/// What the header offers for I5: (label, attribute that opens it).
const CREATE: [(&str, &str); 4] = [
    ("Invoice", r#"hx-get="/invoices/new""#),
    ("Sales receipt", r#"hx-get="/sales-receipts/new""#),
    ("Opportunity", r#"hx-get="/opportunities/new""#),
    ("Item", r#"hx-get="/items/new""#),
];

fn app() -> axum::Router {
    router(AppState::new(db::open_in_memory(Seed::Demo).unwrap().0))
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let (s, h) = (res.status(), res.headers().clone());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (s, h, String::from_utf8_lossy(&body).into_owned())
}

async fn page(app: &axum::Router, uri: &str) -> String {
    let (s, _, html) = send(app, Request::get(uri).body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK, "{uri}");
    html
}

fn menu(html: &str) -> &str {
    let start = html.find(r#"aria-label="New""#).expect("the New menu");
    &html[start..start + html[start..].find("</ul>").unwrap()]
}

/// The page without its header and sidebar: what the page itself offers.
fn body(html: &str) -> &str {
    let start = html.find("</header>").expect("the header");
    &html[start..]
}

const I5_PAGES: [&str; 9] = [
    "/items",
    "/opportunities",
    "/opportunities/o-hp-fitout2",
    "/sales",
    "/sales/INV-1048",
    "/sales/SR-0311",
    "/payments",
    "/reports/sales-by-month",
    "/",
];

#[tokio::test]
async fn the_header_reaches_every_i5_create_action_from_every_i5_page() {
    let a = app();
    for uri in I5_PAGES {
        let html = page(&a, uri).await;
        let m = menu(&html);
        for (label, attr) in CREATE {
            assert!(m.contains(attr), "{uri}: no {label} in the menu");
            assert!(m.contains(&format!("</svg>{label}</button>")), "{uri}: {label} is not live");
        }
        assert!(!m.contains("data-soon"), "{uri}: a menu entry is still a placeholder");
    }
}

#[tokio::test]
async fn no_i5_page_repeats_a_create_action() {
    let a = app();
    for uri in I5_PAGES {
        let html = page(&a, uri).await;
        let page_only = body(&html);
        for (label, attr) in CREATE {
            assert!(!page_only.contains(attr), "{uri} repeats {label}");
            assert_eq!(html.matches(attr).count(), 1, "{uri}: {label} appears once");
        }
        for text in ["New invoice", "New sales receipt", "New opportunity", "New item"] {
            assert!(!page_only.contains(text), "{uri} still says {text}");
        }
    }
}

#[tokio::test]
async fn each_header_entry_opens_its_dialog_and_creates_from_any_page() {
    let a = app();
    for (uri, expect) in [
        ("/invoices/new", r#"hx-post="/invoices""#),
        ("/sales-receipts/new", r#"hx-post="/sales-receipts""#),
        ("/opportunities/new", r#"hx-post="/opportunities""#),
        ("/items/new", r#"hx-post="/items""#),
    ] {
        let req = Request::get(uri).header("HX-Request", "true").body(Body::empty()).unwrap();
        let (s, _, html) = send(&a, req).await;
        assert_eq!(s, StatusCode::OK, "{uri}");
        assert!(html.contains(expect), "{uri}");
        assert!(!html.contains("<html"), "{uri} is a dialog, not a page");
    }
    // Opened on another page, each create goes to its record or its list: nothing needs the page's own table.
    for (uri, form, to) in [
        ("/items", "name=Header+item&active=on", "/items"),
        ("/invoices", "customer_id=r-hp", "/sales/INV-1052"),
        ("/sales-receipts", "customer_id=r-hp", "/sales/SR-0312"),
    ] {
        let req = Request::post(uri)
            .header("HX-Request", "true")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .unwrap();
        let (s, h, text) = send(&a, req).await;
        assert_eq!(s, StatusCode::OK, "{uri}: {text}");
        assert_eq!(h.get("HX-Redirect").unwrap(), to, "{uri}");
    }
    let req = Request::post("/opportunities")
        .header("HX-Request", "true")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("party_id=p-hp&name=From+the+header"))
        .unwrap();
    let (s, h, text) = send(&a, req).await;
    assert_eq!(s, StatusCode::OK, "{text}");
    assert!(h.get("HX-Redirect").unwrap().to_str().unwrap().starts_with("/opportunities/"));
}

/// Kept: they preset what is open, which the header cannot do.
#[tokio::test]
async fn in_context_actions_stay() {
    let a = app();
    let party = page(&a, "/parties/p-hp").await;
    for attr in [
        r#"hx-get="/invoices/new?customer_id=r-hp""#,
        r#"hx-get="/sales-receipts/new?customer_id=r-hp""#,
        r#"hx-get="/opportunities/new?party_id=p-hp""#,
    ] {
        assert_eq!(party.matches(attr).count(), 1, "{attr}");
    }
    // A payment starts from its invoice (kontacts 221), and the header cannot name the invoice: once, on a posted one.
    let posted = page(&a, "/sales/INV-1048").await;
    assert_eq!(posted.matches("/payments/new?invoice_id=").count(), 1);
    assert!(!menu(&posted).contains("/payments/new"));
    // Row-level Edit on the list, one per posted row.
    let list = page(&a, "/sales").await;
    assert!(list.contains(r#"aria-label="Unpost INV-1051""#));
}

/// An empty list names no removed button.
#[tokio::test]
async fn empty_states_name_no_removed_button() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         DELETE FROM payment_line; DELETE FROM payment; DELETE FROM sales_line; DELETE FROM invoice; DELETE FROM sales_receipt;
         DELETE FROM sales_transaction; DELETE FROM item; DELETE FROM opportunity;",
    )
    .unwrap();
    let a = router(AppState::new(conn));
    for (uri, says) in [
        ("/items", "No items yet."),
        ("/opportunities", "No opportunities"),
        ("/sales", "No sales transactions match."),
        ("/payments", "No payments match."),
    ] {
        let html = page(&a, uri).await;
        let page_only = body(&html);
        assert!(page_only.contains(says), "{uri}");
        for removed in ["New invoice", "New sales receipt", "New opportunity", "New item", "button"] {
            let text: String = page_only.split('<').map(|p| p.split_once('>').map_or("", |x| x.1)).collect();
            assert!(!text.contains(removed), "{uri} names {removed}: {text}");
        }
    }
}
