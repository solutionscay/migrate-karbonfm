//! The header's New menu is the one place for a page's create action (AGENTS.md): it offers each create action, the
//! pages do not repeat them, and an action the header cannot do (it presets the record that is open) stays in place.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::{db, router, AppState};
use tower::ServiceExt;

/// Each page of the app with a header; the menu must be the same on all of them.
const PAGES: [&str; 13] = [
    "/",
    "/activities",
    "/activities?party_id=p-hp",
    "/chat",
    "/customers",
    "/parties",
    "/parties/p-hp",
    "/parties/new",
    "/sales",
    "/payments",
    "/templates",
    "/files",
    "/settings",
];

/// What the menu's create items open: (label, attribute that starts it).
const CREATE: [(&str, &str); 9] = [
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
        }
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
    for uri in ["/parties/new?role=customer", "/parties/new", "/templates/new", "/email-messages/new"] {
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
