//! The header's New menu is the one place for a page's create action (AGENTS.md): it offers each create action, the
//! pages do not repeat them, and an action the header cannot do (it presets the record that is open) stays in place.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::db::{self, Seed};
use karbon_server::{router, AppState};
use tower::ServiceExt;

/// Each page of the app with a header; the menu must be the same on all of them.
const PAGES: [&str; 19] = [
    "/",
    "/activities",
    "/activities?party_id=p-hp",
    "/chat",
    "/customers",
    "/parties",
    "/parties/p-hp",
    "/parties/new",
    "/items",
    "/opportunities",
    "/opportunities/o-hp-fitout2",
    "/sales",
    "/sales/INV-1048",
    "/sales/SR-0311",
    "/payments",
    "/reports/sales-by-month",
    "/templates",
    "/files",
    "/settings",
];

/// What the menu's create items open: (label, attribute that starts it).
const CREATE: [(&str, &str); 13] = [
    ("Invoice", r#"hx-get="/invoices/new""#),
    ("Sales receipt", r#"hx-get="/sales-receipts/new""#),
    ("Opportunity", r#"hx-get="/opportunities/new""#),
    ("Item", r#"hx-get="/items/new""#),
    ("Customer", r#"href="/parties/new?role=customer""#),
    ("Contact", r#"href="/parties/new""#),
    ("Task", r#"hx-get="/activities/new?kind=task""#),
    ("Note", r#"hx-get="/activities/new?kind=note""#),
    ("Meeting", r#"hx-get="/activities/new?kind=meeting""#),
    ("Call", r#"hx-get="/activities/new?kind=call""#),
    ("Email", r#"hx-get="/email-messages/new""#),
    ("Template", r#"hx-get="/templates/new""#),
    ("File", r#"hx-post="/files""#),
];

async fn get(uri: &str, htmx: bool) -> (StatusCode, String) {
    let app = router(AppState::new(db::open_demo(None).expect("demo database")));
    let mut req = Request::get(uri);
    if htmx {
        req = req.header("HX-Request", "true");
    }
    let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// The header's New menu, and the page without its shell (sidebar, header, shared dialog).
fn split(html: &str) -> (&str, &str) {
    let menu = &html[html.find(r#"aria-label="New""#).expect("New menu")..];
    let menu = &menu[..menu.find("</ul>").unwrap()];
    let content = &html[html.find("</header>").unwrap()..];
    let content = &content[..content.find(r#"<dialog id="modal""#).unwrap()];
    (menu, content)
}

#[tokio::test]
async fn every_page_has_the_same_menu_and_no_button_repeats_it() {
    for page in PAGES {
        let (status, html) = get(page, false).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let (menu, content) = split(&html);
        for (label, opens) in CREATE {
            assert!(menu.contains(opens), "{page}: the menu's {label}");
            assert!(!content.contains(opens), "{page}: {label} is repeated on the page");
            assert_eq!(html.matches(opens).count(), 1, "{page}: {label} appears once");
        }
        assert_eq!(menu.matches("<li>").count(), CREATE.len(), "{page}: nothing else in the menu");
        assert!(!menu.contains("data-soon") && !menu.contains("Later"), "{page}: a menu entry is still a placeholder");
        assert_eq!(html.matches(r#"id="file-upload-result""#).count(), 1, "{page}: where an upload error goes");
    }
}

#[tokio::test]
async fn the_list_pages_have_no_create_button() {
    for (page, button) in [
        ("/templates", "New template"),
        ("/customers", "New customer"),
        ("/parties", "New contact"),
        ("/files", "Add file"),
        ("/items", "New item"),
        ("/opportunities", "New opportunity"),
        ("/sales", "New invoice"),
        ("/sales", "New sales receipt"),
    ] {
        let (_, html) = get(page, false).await;
        let (_, content) = split(&html);
        assert!(!content.contains(button), "{page}: {button}");
    }
    let (_, html) = get("/activities", false).await;
    let (_, content) = split(&html);
    assert!(!content.contains("/activities/new") && !content.contains("/email-messages/new"), "plain timeline");
}

#[tokio::test]
async fn each_menu_item_opens_something() {
    for uri in [
        "/parties/new?role=customer",
        "/parties/new",
        "/templates/new",
        "/email-messages/new",
        "/invoices/new",
        "/sales-receipts/new",
        "/opportunities/new",
        "/items/new",
    ] {
        let (status, html) = get(uri, true).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains("<form"), "{uri}");
    }
    for kind in ["task", "note", "meeting", "call"] {
        let uri = format!("/activities/new?kind={kind}");
        let (status, html) = get(&uri, true).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains("<form"), "{uri}");
    }
}

/// The exception of AGENTS.md: these preset the party, which the header's menu cannot do.
#[tokio::test]
async fn a_party_keeps_the_actions_that_preset_it() {
    for page in ["/activities?party_id=p-hp", "/parties/p-hp"] {
        let (_, html) = get(page, false).await;
        let (_, content) = split(&html);
        for kind in ["task", "note", "meeting", "call"] {
            assert!(
                content.contains(&format!("/activities/new?kind={kind}&amp;party_id=p-hp"))
                    || content.contains(&format!("/activities/new?kind={kind}&party_id=p-hp")),
                "{page}: {kind}"
            );
        }
        assert!(content.contains(r#"hx-get="/email-messages/new?party_id=p-hp""#), "{page}: email");
        assert!(content.contains("/discussions/new?entity_type_id=10&amp;entity_id=p-hp"), "{page}: discussion");
    }
}

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

#[tokio::test]
async fn each_i5_create_works_from_another_page() {
    let a = app();
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
