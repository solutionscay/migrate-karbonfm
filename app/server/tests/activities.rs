//! I4 route tests: activities (UI-07, OP-ACTIVITY-01..04), T-08-08. The router runs in process on the demo database
//! and the tests check status, HTML and htmx headers.

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

fn redirect(h: &HeaderMap) -> String {
    h.get("HX-Redirect").map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default()
}

#[tokio::test]
async fn an_activity_page_shows_its_details_and_people() {
    let app = demo();
    let (status, html) = get(&app, "/activities/a-meet-walk").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Phase 2 walkthrough on site</h1>"));
    assert!(html.contains("48 Wharf Street, Portland"));
    assert!(html.contains(r#"aria-label="People on this meeting""#));
    assert!(html.contains("Elena Halden") && html.contains("Invited"));
    assert!(html.contains(r#"hx-delete="/activities/a-meet-walk""#), "the delete button");
    let (status, html) = get(&app, "/activities/a-task-quote").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Open</span>"));
    let (status, _) = get(&app, "/activities/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_new_dialog_opened_from_a_party_is_about_it() {
    let app = demo();
    let (status, html) = get(&app, "/activities/new?kind=note&party_id=p-hp").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("New note</h2>"));
    assert!(html.contains(r#"name="regarding_id" value="p-hp""#));
    assert!(html.contains("Halden &#38; Pryce Architects"));
    let (_, html) = get(&app, "/activities/new?kind=meeting&party_id=p-hp").await;
    assert!(html.contains(r#"name="entity_id" value="p-hp""#), "an event is linked to the party");
    let (_, html) = get(&app, "/activities/new?kind=task").await;
    assert!(html.contains(r#"<option value="me" selected>Me (Mara Ellison)</option>"#));
    let (status, _) = get(&app, "/activities/new?kind=email").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "an email has its own composer (UI-08)");
}

#[tokio::test]
async fn create_opens_the_new_activity_and_field_errors_come_back_422() {
    let app = demo();
    let (status, h, _) = send(
        &app,
        form("POST", "/activities", "kind=note&subject=Called+Elena&message=About+lights&regarding_id=p-hp"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let to = redirect(&h);
    assert!(to.starts_with("/activities/"), "{to}");
    assert!(h.get("HX-Trigger").unwrap().to_str().unwrap().contains("Note created"));
    let (_, html) = get(&app, &to).await;
    assert!(html.contains("Called Elena</h1>") && html.contains("Regarding") && html.contains("Creator"));

    let (status, _, html) = send(&app, form("POST", "/activities", "kind=note&subject=&message=text")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("The subject is required."));
    let (status, _, html) =
        send(&app, form("POST", "/activities", "kind=meeting&subject=Walk&start_date=11%2F02")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter a date as YYYY-MM-DD."));
    let (status, _, _) = send(&app, form("POST", "/activities", "kind=note&subject=x&bogus=1")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown fields are refused (05)");
}

#[tokio::test]
async fn a_task_assigned_to_me_has_me_as_assignee() {
    let app = demo();
    let (_, h, _) =
        send(&app, form("POST", "/activities", "kind=task&subject=Order+boards&due_date=2026-11-04&assigned_to=me"))
            .await;
    let (_, html) = get(&app, &redirect(&h)).await;
    assert!(html.contains("Assigned To"));
    assert!(html.contains("Nov 4, 2026"), "{html}");
}

#[tokio::test]
async fn edit_saves_and_an_empty_activity_offers_the_discard() {
    let app = demo();
    let (status, html) = get(&app, "/activities/a-meet-walk/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"value="10:00""#) && html.contains("hx-patch"));
    let body = "kind=meeting&subject=Walkthrough&start_date=2026-10-16&start_time=10%3A30&location=On+site";
    let (status, h, _) = send(&app, form("PATCH", "/activities/a-meet-walk", body)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(redirect(&h), "/activities/a-meet-walk");
    let (_, html) = get(&app, "/activities/a-meet-walk").await;
    assert!(html.contains("Walkthrough</h1>") && html.contains("10:30"));

    let (_, h, _) = send(&app, form("POST", "/activities", "kind=call&subject=Ring")).await;
    let id = redirect(&h);
    let (status, _, html) = send(&app, form("PATCH", &id, "kind=call&subject=")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Discard activity?") && html.contains(&format!(r#"hx-delete="{id}?confirm=true""#)));
}

#[tokio::test]
async fn delete_asks_first_then_goes_to_the_party() {
    let app = demo();
    let (status, _, html) = send(&app, Request::delete("/activities/a-task-quote").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Delete this task?") && html.contains("3 people on it"));
    let (_, html) = get(&app, "/activities/a-task-quote").await;
    assert!(html.contains("Send revised fit-out quote"), "nothing deleted yet");
    let req = Request::delete("/activities/a-task-quote?confirm=true").header("HX-Request", "true").body(Body::empty());
    let (status, h, _) = send(&app, req.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(redirect(&h), "/parties/p-hp", "the party it was about");
    let (status, _) = get(&app, "/activities/a-task-quote").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn people_are_added_changed_and_removed() {
    let app = demo();
    let (status, html) = get(&app, "/activities/a-meet-walk/parties/new").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"<option value="60" selected>Invited</option>"#));
    assert!(html.contains(r#"<option value="62" >Organizer</option>"#));
    assert!(!html.contains("Regarding"), "only the roles that fit a meeting");

    let (status, _, html) =
        send(&app, form("PUT", "/activities/a-meet-walk/parties", "role=62&other_id=p-hp-jw&q=")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"hx-swap-oob="true""#) && html.contains("Organizer"));
    let (status, _, html) =
        send(&app, form("PUT", "/activities/a-meet-walk/parties", "role=63&other_id=p-hp-jw&q=")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("does not apply to this meeting"));

    let (_, html) = get(&app, "/activities/a-meet-walk").await;
    let row = html.split("hx-delete=\"/activity-parties/").nth(1).unwrap().split('"').next().unwrap().to_owned();
    let (status, html) = get(&app, &format!("/activity-parties/{row}/edit")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Change person") && html.contains(r#"name="activity_party_id""#));
    let req = Request::delete(format!("/activity-parties/{row}")).header("HX-Request", "true").body(Body::empty());
    let (status, _, html) = send(&app, req.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="activity-parties""#));
}

/// T-08-08 (AP-08): a sent email cannot be edited, nor its people changed; the answer is AlreadySent (409) and
/// nothing changes.
#[tokio::test]
async fn t_08_08_a_sent_email_is_read_only() {
    let app = demo();
    let (_, before) = get(&app, "/activities/a-mail-1048").await;
    assert!(before.contains("Sent</span>"));
    assert!(!before.contains("/activities/a-mail-1048/edit"), "no Edit button on a sent email");
    assert!(!before.contains("Add person"), "no people actions on a sent email");

    let (status, _, text) = send(&app, form("PATCH", "/activities/a-mail-1048", "kind=email&subject=Changed")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(text.contains("already been sent"));
    let (status, _) = get(&app, "/activities/a-mail-1048/edit").await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) =
        send(&app, form("PUT", "/activities/a-mail-1048/parties", "role=56&other_id=p-hp-eh&q=")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = get(&app, "/activities/a-mail-1048/parties/new").await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (_, after) = get(&app, "/activities/a-mail-1048").await;
    assert_eq!(before, after, "nothing changed");
}
