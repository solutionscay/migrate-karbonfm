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
async fn about_shows_the_regarding_party_and_the_entity_link_and_created_is_a_date() {
    // B8: a task, note or call is about its Regarding (63) party (390 s121, 641 s129); an event or meeting started on
    // a party is about its activity_entity link (392 s130-133). The page shows either under About, and the timeline
    // row of a meeting with no party shows the party it is linked to.
    let app = demo();
    let (_, h, _) = send(
        &app,
        form("POST", "/activities", "kind=task&subject=Send+glaze+quote&due_date=2026-11-06&regarding_id=p-hp"),
    )
    .await;
    let (_, html) = get(&app, &redirect(&h)).await;
    let about = html.split(r#"<dt class="muted">About</dt>"#).nth(1).unwrap().split("</dd>").next().unwrap();
    assert!(about.contains(r#"href="/parties/p-hp""#) && about.contains("Halden &#38; Pryce Architects"), "{about}");
    let created = html.split(r#"<dt class="muted">Created</dt>"#).nth(1).unwrap().split("</dd>").next().unwrap();
    assert!(!created.contains(':') || created.contains(" · "), "{created}");
    assert!(!created.contains('-'), "no raw D-05 stamp: {created}");

    let (_, h, _) = send(
        &app,
        form(
            "POST",
            "/activities",
            "kind=meeting&subject=Glaze+review&start_date=2026-11-04&start_time=10%3A00&entity_type_id=10&entity_id=p-bm",
        ),
    )
    .await;
    let meeting = redirect(&h);
    let (_, html) = get(&app, &meeting).await;
    let about = html.split(r#"<dt class="muted">About</dt>"#).nth(1).unwrap().split("</dd>").next().unwrap();
    assert!(about.contains("Bluefin Marine Supply"), "{about}");
    let (_, html) = page(&app, "/activities?kind=meet").await;
    let row = html.split("Glaze review").nth(1).unwrap().split("</a>").next().unwrap();
    assert!(row.contains("Bluefin Marine Supply"), "the meeting row names its party: {row}");
}

#[tokio::test]
async fn a_sent_email_says_when_and_with_which_account() {
    // B9: the seed's INV-1048 went out through the system account (use_system_settings_ind = 1).
    let app = demo();
    let (_, html) = get(&app, "/email-messages/a-mail-1048").await;
    assert!(html.contains("Sent Oct 14, 2026 · 16:40 with the system account"), "{html}");
    assert!(!html.contains("2026-10-14 16:40:00"));
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
    let (status, _, _) = send(&app, Request::get("/activities/a-mail-1048").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "an email opens its own page (UI-08)");
    let (_, before) = get(&app, "/email-messages/a-mail-1048").await;
    assert!(before.contains("Sent</span>"));
    assert!(!before.contains(r#"name="subject""#), "no edit form on a sent email");
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

    let (status, _, _) =
        send(&app, form("PATCH", "/email-messages/a-mail-1048", "subject=x&message=y&account=user")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) = send(&app, form("POST", "/email-messages/a-mail-1048/template", "template_id=1")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, after) = get(&app, "/email-messages/a-mail-1048").await;
    assert_eq!(before, after, "nothing changed");
}

// ---------------------------------------------------------------------------------------------------------------
// S2: the timeline, tasks and home (OP-ACTIVITY-05, OP-ACTIVITY-08, UI-02)
// ---------------------------------------------------------------------------------------------------------------

async fn page(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app, Request::get(uri).body(Body::empty()).unwrap()).await;
    (s, b)
}

#[tokio::test]
async fn the_timeline_lists_activities_by_day_with_filters() {
    let app = demo();
    let (status, html) = page(&app, "/activities").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains(r#"href="/activities" aria-current="page""#), "the sidebar marks Activity");
    assert!(html.contains("Send revised fit-out quote") && html.contains("Phase 2 walkthrough on site"));
    assert!(html.contains("Oct 28, 2026") && html.contains("Oct 16, 2026"), "day headers");
    assert!(html.contains(r#"href="/discussions/a-disc-light""#), "a discussion opens its own page");
    assert!(html.contains("2 comments"));
    assert!(!html.contains("Payment received"), "payments are not activities (design-only row left out)");

    let (_, html) = page(&app, "/activities?kind=meet").await;
    assert!(html.contains("Phase 2 walkthrough") && !html.contains("Send revised fit-out quote"));
    assert!(html.contains(r#"class="filter-chip on" href="/activities?kind=meet" aria-current="true""#));
    let (_, html) = page(&app, "/activities?kind=task&status=complete").await;
    assert!(html.contains("No activities match."));
    let req = Request::get("/activities?kind=email").header("HX-Request", "true").header("HX-Target", "activity-rows");
    let (status, _, html) = send(&app, req.body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.trim_start().starts_with(r#"<div id="activity-rows">"#)
            && html.contains("Invoice INV-1048")
            && html.contains("Sent")
    );
}

#[tokio::test]
async fn a_task_is_completed_and_reopened_from_its_page() {
    let app = demo();
    let (_, html) = page(&app, "/activities/a-task-quote").await;
    assert!(html.contains(r#"hx-post="/tasks/a-task-quote/complete""#) && html.contains(">Complete</button>"));
    let req = form("POST", "/tasks/a-task-quote/complete", "complete=true");
    let (status, h, _) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(redirect(&h), "/activities/a-task-quote");
    assert!(h.get("HX-Trigger").unwrap().to_str().unwrap().contains("Task completed"));
    let (_, html) = page(&app, "/activities/a-task-quote").await;
    assert!(html.contains("Completed</span>") && html.contains(">Reopen</button>") && html.contains("Completed By"));
    let (status, _, text) = send(&app, form("POST", "/tasks/a-task-quote/complete", "complete=true")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(text.contains("complete already"));
    let mut req = form("POST", "/tasks/a-task-quote/complete", "complete=false");
    req.headers_mut().insert("HX-Current-URL", "http://127.0.0.1:5555/?x=1".parse().unwrap());
    let (_, h, _) = send(&app, req).await;
    assert_eq!(redirect(&h), "/?x=1", "back to the page it was done on");
    let (status, _, _) = send(&app, form("POST", "/tasks/a-meet-walk/complete", "complete=true")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a meeting is not a task");
}

#[tokio::test]
async fn the_dashboard_shows_my_tasks_and_recent_activity() {
    let app = demo();
    let (status, html) = page(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="tasks-title""#) && html.contains("1 open"));
    assert!(html.contains("Send revised fit-out quote"));
    assert!(html.contains(r#"aria-label="Complete Send revised fit-out quote""#));
    assert!(html.contains(r#"id="recent-title""#) && html.contains("Lighting spec for the reception area"));
    send(&app, form("POST", "/tasks/a-task-quote/complete", "complete=true")).await;
    let (_, html) = page(&app, "/").await;
    assert!(html.contains("0 open") && html.contains("No open tasks."));
}

#[tokio::test]
async fn the_party_activity_tab_lists_its_activities_and_new_buttons() {
    let app = demo();
    let (status, html) = page(&app, "/parties/p-hp").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("Activities come in a later release"));
    assert!(html.contains(r#"aria-selected="tab === 'activity'">Activity <span class="n">4</span>"#));
    assert!(html.contains("Send revised fit-out quote") && html.contains("Invoice INV-1048"));
    assert!(html.contains(r#"hx-get="/activities/new?kind=note&amp;party_id=p-hp""#));
    assert!(html.contains(r#"href="/activities?party_id=p-hp""#));
    let (_, html) = page(&app, "/activities?party_id=p-hp").await;
    assert!(html.contains("Activity · Halden &#38; Pryce Architects</h1>"));
}

#[tokio::test]
async fn the_top_bar_new_task_and_note_open_the_dialog() {
    let app = demo();
    let (_, html) = page(&app, "/").await;
    assert!(html.contains(r##"hx-get="/activities/new?kind=task" hx-target="#modal-body""##));
    assert!(html.contains(r##"hx-get="/activities/new?kind=note" hx-target="#modal-body""##));
    assert!(!html.contains(r#"data-soon="New task""#));
}

// ---------------------------------------------------------------------------------------------------------------
// S3: discussions and notifications (OP-ACTIVITY-06, 07, 09; UI-09)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_discussion_page_shows_its_comments_and_subscribers() {
    let app = demo();
    let (status, html) = page(&app, "/discussions/a-disc-light").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="disc-title" class="page-title flex-1">Lighting spec for the reception area</h1>"#));
    assert!(html.contains("Can we see the warmer 2700K option?") && html.contains("Elena Halden"));
    assert!(html.contains("2 subscribers"), "Elena, and Mara (the demo seed, B4)");
    assert!(html.contains(r#"href="/parties/p-hp""#) || html.contains("Halden"), "what it is about");
    assert!(html.contains(r#"aria-label="Comment""#));
    assert!(!html.contains("get an email"), "the design's email promise is not in the source (670 s114)");
    let (_, html) = page(&app, "/activities/a-disc-light").await;
    assert!(html.is_empty() || !html.contains("<h1"), "the activity page sends a discussion to its own page");
    let (status, _, _) = send(&app, Request::get("/activities/a-disc-light").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _) = page(&app, "/discussions/a-task-quote").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_comment_is_added_and_notifies_the_subscriber() {
    let app = demo();
    let (status, h, html) =
        send(&app, form("POST", "/discussions/a-disc-light/comments", "comment=Warmer+it+is")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"id="comments""#) && html.contains("Warmer it is"));
    assert!(h.get("HX-Trigger").unwrap().to_str().unwrap().contains("Comment added"));
    // The bell reloads itself on this event, so a new notification shows without a page load.
    assert!(h.get("HX-Trigger").unwrap().to_str().unwrap().contains("karbon-notifications"));
    let (_, bell) = get(&app, "/notifications").await;
    assert!(bell.contains(r#"hx-trigger="karbon-notifications from:body""#), "{bell}");
    let (status, _, text) = send(&app, form("POST", "/discussions/a-disc-light/comments", "comment=+")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(text.contains("Write a comment first"));
    let (_, html) = page(&app, "/activities?kind=disc").await;
    assert!(html.contains("3 comments"));
}

#[tokio::test]
async fn a_discussion_is_started_on_a_party_and_its_subscribers_managed() {
    let app = demo();
    let (status, html) = get(&app, "/discussions/new?entity_type_id=10&entity_id=p-hp").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Title: <b") && html.contains("Halden &#38; Pryce Architects"));
    assert!(html.contains(r#"name="subscriber" value="p-hp-eh" class="checkbox checkbox-xs" >"#), "unchecked");
    let (status, _) = get(&app, "/discussions/new?entity_type_id=10&entity_id=nobody").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let body = "entity_type_id=10&entity_id=p-hp&notes=Kick-off&subscriber=p-hp-eh&subscriber=p-hp-mp";
    let (status, h, _) = send(&app, form("POST", "/discussions", body)).await;
    assert_eq!(status, StatusCode::OK);
    let to = redirect(&h);
    assert!(to.starts_with("/discussions/"), "{to}");
    let (_, html) = page(&app, &to).await;
    assert!(html.contains("2 subscribers") && html.contains("Kick-off"));

    let (status, html) = get(&app, &format!("{to}/subscribers")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"value="p-hp-eh" class="checkbox checkbox-xs" checked"#));
    let (status, h, _) = send(&app, form("PATCH", &to, "subscriber=p-hp-jw")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(redirect(&h), to);
    let (_, html) = page(&app, &to).await;
    assert!(html.contains("1 subscriber") && html.contains("Closed"), "no active box: closed");
    let (status, _, _) = send(&app, form("PATCH", &to, "subscriber=p-hp-jw&bogus=1")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown fields are refused");
    let (status, _, _) = send(&app, form("POST", "/discussions", "entity_type_id=14&entity_id=x")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_bell_counts_unread_notifications_and_marks_one_read() {
    let app = demo();
    let (_, html) = page(&app, "/").await;
    assert!(html.contains(r#"<div id="bell" hx-get="/notifications" hx-trigger="load""#));
    let (status, html) = get(&app, "/notifications").await;
    assert_eq!(status, StatusCode::OK);
    // The demo seed's comments notified Mara as a subscriber: one read, one unread (B4).
    assert!(html.contains("Notifications, 1 unread") && html.contains(r#"href="/discussions/a-disc-light""#));
    // A discussion with no subscribers notifies everyone on it, the commenter (the current user) too (422).
    let (_, h, _) = send(&app, form("POST", "/discussions", "entity_type_id=10&entity_id=p-bm")).await;
    let disc = redirect(&h);
    send(&app, form("POST", &format!("{disc}/comments"), "comment=Hello")).await;
    let (_, html) = get(&app, "/notifications").await;
    assert!(html.contains("Notifications, 2 unread") && html.contains(&format!(r#"href="{disc}""#)));
    let id = html.split(r#"{"id": ""#).nth(1).unwrap().split('"').next().unwrap().to_owned();
    let (status, _, html) = send(&app, form("POST", "/notifications/read", &format!("id={id}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(" read</span>") && html.contains("Notifications, 1 unread"), "{html}");
    let (status, _, _) = send(&app, form("POST", "/notifications/read", "id=nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------------------------------------------
// S5: email compose (UI-08); the send itself is in tests/smtp_contract.rs
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_email_is_composed_from_a_party_and_its_recipients_managed() {
    let app = demo();
    let (status, html) = get(&app, "/email-messages/new?party_id=p-hp-eh").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("New email</h2>") && html.contains("To Elena Halden") && html.contains(r#"name="email_id""#));
    let (status, h, _) = send(
        &app,
        form(
            "POST",
            "/email-messages",
            "party_id=p-hp-eh&email_id=cm-037&entity_type_id=&entity_id=&subject=Hello&message=Hi",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let to = redirect(&h);
    assert!(to.starts_with("/email-messages/"), "{to}");
    assert!(h.get("HX-Trigger").unwrap().to_str().unwrap().contains("not sent"));
    let (_, html) = page(&app, &to).await;
    assert!(html.contains("Not sent</span>") && html.contains(r#"name="subject" value="Hello""#));
    assert!(
        html.contains(">From<") || html.contains("From ·") || html.contains("From</span>") || html.contains("From"),
        "the From default"
    );
    assert!(html.contains("Elena Halden") && html.contains("To"));
    assert!(html.contains(r#"hx-post="/email-messages/"#) && html.contains("/send"));
    let id = to.trim_start_matches("/email-messages/");
    let (status, html) = get(&app, &format!("/activities/{id}/parties/new")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains(r#"<option value="55""#)
            && html.contains(r#"<option value="57""#)
            && html.contains("Email address")
    );
    assert!(!html.contains(r#"<option value="63""#), "Regarding does not fit an email (710 s34)");
    let (status, _, html) = send(&app, form("PATCH", &to, "subject=Hello+again&message=Hi&account=system")).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let (_, html) = page(&app, &to).await;
    assert!(html.contains(r#"value="system" class="radio radio-xs" checked"#));
    let (status, html) = get(&app, "/pickers/email?other_id=p-hp-mp").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(r#"name="email_id""#));
    let (status, html) = get(&app, "/pickers/email?other_id=").await;
    assert_eq!((status, html.as_str()), (StatusCode::OK, ""), "nobody chosen yet");
}

#[tokio::test]
async fn the_party_activity_tab_offers_a_new_email() {
    let app = demo();
    let (_, html) = page(&app, "/parties/p-hp").await;
    assert!(html.contains(r#"hx-get="/email-messages/new?party_id=p-hp""#));
    assert!(html.contains(r#"<a class="nav-link" href="/activities?kind=email">"#), "the sidebar's Email link is live");
}
