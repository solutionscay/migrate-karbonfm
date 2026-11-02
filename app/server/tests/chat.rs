//! I4 S7: UI-21 Messages, a static mock (OD-05-02). Sample conversations, no chat data, nothing can be sent.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::{db, router, AppState};
use tower::ServiceExt;

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn main_of(html: &str) -> &str {
    let start = html.find("<main").expect("a main element");
    let end = html.find("</main>").expect("the end of main");
    &html[start..end]
}

#[tokio::test]
async fn the_chat_page_is_a_sample_with_the_banner() {
    let app = router(AppState::new(db::open_demo(None).unwrap()));
    let (status, html) = send(&app, Request::get("/chat").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("Sample data.</b> SMS is not available in this version."));
    assert!(html.contains(r#"href="/chat" aria-current="page""#), "the sidebar's Messages link is current");
    let main = main_of(&html);
    for name in ["Elena Halden", "Tomás Ferreira", "Dana Whitlock", "Dr. Ngozi Okafor"] {
        assert!(main.contains(name), "{name}");
    }
    assert!(main.contains("Boards look great. Marcus loves the oak detail."));
    assert!(main.contains(r#"class="bubble out""#) && main.contains(r#"class="bubble in""#));
}

#[tokio::test]
async fn nothing_on_the_chat_page_can_send() {
    let app = router(AppState::new(db::open_demo(None).unwrap()));
    let (_, html) = send(&app, Request::get("/chat").body(Body::empty()).unwrap()).await;
    let main = main_of(&html);
    assert!(!main.contains("<form"), "no form");
    for attr in ["hx-post", "hx-put", "hx-patch", "hx-delete", "hx-get", "x-model", "@click"] {
        assert!(!main.contains(attr), "no {attr}");
    }
    let textarea = &main[main.find("<textarea").unwrap()..];
    assert!(textarea[..textarea.find('>').unwrap()].contains(" disabled"), "the message box is disabled");
    let send_button = &main[..main.find(r#"aria-label="Send""#).unwrap()];
    let tag = &main[send_button.rfind("<button").unwrap()..];
    assert!(tag[..tag.find('>').unwrap()].contains(" disabled"), "the send button is disabled");
    for (i, _) in main.match_indices("<button") {
        let tag = &main[i..i + main[i..].find('>').unwrap()];
        assert!(tag.contains(" disabled"), "every button is disabled: {tag}");
    }
    // GET only: there is no route that takes a message.
    let (status, _) = send(&app, Request::post("/chat").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn the_chat_page_reads_no_chat_data() {
    // A database with no SMS activities and no parties to speak of renders the same conversations.
    let (conn, _) = db::open_in_memory(db::Seed::Demo).unwrap();
    conn.execute_batch("DELETE FROM activity WHERE activity_type_id = 9;").unwrap();
    let app = router(AppState::new(conn));
    let (status, html) = send(&app, Request::get("/chat").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(main_of(&html).contains("Elena Halden"));
}
