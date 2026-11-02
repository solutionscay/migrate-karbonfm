//! Discussion screens (UI-09, design/activity.html): the page with its comments and the comment box, the new
//! discussion and subscribers dialogs (OP-ACTIVITY-06/07), and the notification bell of the top bar (OP-ACTIVITY-09).

use super::activities::{activity_href, command_error};
use super::current_user::CurrentUser;
use super::party_forms::{go, toast};
use super::{initials, Nav};
use crate::admin::{CommandError, FieldError};
use crate::discussions::{self, Discussion, NewDiscussion};
use crate::invoice::display_date;
use crate::web::{render, urlencode, AppError, FormPairs};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

pub fn discussion_href(id: &str) -> String {
    format!("/discussions/{}", urlencode(id))
}

fn when(date: &str, time: &str) -> String {
    let t = time.get(..5).unwrap_or(time);
    match (date.is_empty(), t.is_empty()) {
        (true, _) => t.to_owned(),
        (false, true) => display_date(date),
        (false, false) => format!("{}, {t}", display_date(date)),
    }
}

/// The comment list of a discussion (`#comments`); a new comment answers with it.
#[derive(Template)]
#[template(path = "discussions/comments.html")]
pub struct Comments<'a> {
    pub d: &'a Discussion,
}

impl Comments<'_> {
    fn initials(&self, name: &str) -> String {
        initials(name)
    }
    fn color(&self, id: &str) -> usize {
        super::parties::color(id)
    }
    fn when(&self, date: &str, time: &str) -> String {
        when(date, time)
    }
}

#[derive(Template)]
#[template(path = "discussions/detail.html")]
struct DetailPage<'a> {
    nav: Nav,
    demo: bool,
    d: &'a Discussion,
    comments: String,
}

impl DetailPage<'_> {
    fn href(&self) -> String {
        discussion_href(&self.d.activity.id)
    }
    fn delete_href(&self) -> String {
        activity_href(&self.d.activity.id)
    }
    fn initials(&self, name: &str) -> String {
        initials(name)
    }
    fn color(&self, id: &str) -> usize {
        super::parties::color(id)
    }
    fn started(&self) -> String {
        when(&self.d.date, &self.d.time)
    }
}

/// `GET /discussions/{id}` (UI-09).
pub async fn detail(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let d = match discussions::discussion_get(&conn, &id) {
        Ok(d) => d,
        Err(e) => return command_error("discussion_get", e),
    };
    let comments = Comments { d: &d }.render()?;
    Ok(render(&DetailPage { nav: Nav::Activity, demo: crate::db::has_demo_data(&conn)?, d: &d, comments })?
        .into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// New discussion (OP-ACTIVITY-06)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "discussions/new_dialog.html")]
struct NewDialog {
    entity_type_id: i64,
    entity_id: String,
    title: String,
    notes: String,
    choices: Vec<(String, String, bool)>,
    errors: Vec<FieldError>,
}

impl NewDialog {
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    entity_type_id: i64,
    entity_id: String,
}

/// `GET /discussions/new?entity_type_id=&entity_id=`: the Start discussion dialog. The title comes from the record
/// (BR-ACTIVITY-07); the people offered as subscribers start unchecked (421).
pub async fn new_dialog(State(state): State<AppState>, Query(q): Query<NewQuery>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let title = match discussions::title_for(&conn, q.entity_type_id, &q.entity_id) {
        Ok(t) => t,
        // No record or one that does not exist: a field error (422), as the create answers it.
        Err(CommandError::Validation(e)) => {
            let message = e.first().map(|e| e.message.clone()).unwrap_or_default();
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, message).into_response());
        }
        Err(e) => return command_error("discussion_create", e),
    };
    let choices = discussions::new_subscriber_choices(&conn, q.entity_type_id, &q.entity_id)?
        .into_iter()
        .map(|(id, name)| (id, name, false))
        .collect();
    let view = NewDialog {
        entity_type_id: q.entity_type_id,
        entity_id: q.entity_id,
        title,
        notes: String::new(),
        choices,
        errors: vec![],
    };
    Ok(Html(view.render()?).into_response())
}

/// `POST /discussions` (OP-ACTIVITY-06): creates it and opens it.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    form: FormPairs,
) -> Result<Response, AppError> {
    // The fields of the form; `subscriber` repeats, one per checked person.
    if let Err(refused) = form.only(&["entity_type_id", "entity_id", "notes", "subscriber"]) {
        return Ok(*refused);
    }
    let subscriber = form.all("subscriber");
    let notes = form.one("notes");
    let mut conn = lock(&state)?;
    let new = NewDiscussion {
        entity_type_id: form.one("entity_type_id").parse().unwrap_or(0),
        entity_id: form.one("entity_id"),
        notes: notes.clone(),
        subscribers: subscriber.clone(),
    };
    match discussions::discussion_create(&mut conn, &new, &actor) {
        Ok(id) => {
            let mut res = go(&headers, &discussion_href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Discussion started", true));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let title = discussions::title_for(&conn, new.entity_type_id, &new.entity_id).unwrap_or_default();
            let choices = discussions::new_subscriber_choices(&conn, new.entity_type_id, &new.entity_id)?
                .into_iter()
                .map(|(id, name)| {
                    let on = subscriber.contains(&id);
                    (id, name, on)
                })
                .collect();
            let view = NewDialog {
                entity_type_id: new.entity_type_id,
                entity_id: new.entity_id,
                title,
                notes,
                choices,
                errors,
            };
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(view.render()?)).into_response())
        }
        Err(e) => command_error("discussion_create", e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Subscribers and active (the edit of OP-ACTIVITY-06)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "discussions/subscribers_dialog.html")]
struct SubscribersDialog {
    id: String,
    active: bool,
    choices: Vec<(String, String, bool)>,
}

/// `GET /discussions/{id}/subscribers`: the Manage subscribers card (layout 187): everyone on the discussion, the
/// party it is about and its relations, the subscribers checked; and the Active switch.
pub async fn subscribers_dialog(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let d = match discussions::discussion_get(&conn, &id) {
        Ok(d) => d,
        Err(e) => return command_error("discussion_get", e),
    };
    let choices = discussions::subscriber_choices(&conn, &id).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Html(SubscribersDialog { id, active: d.active, choices }.render()?).into_response())
}

/// `PATCH /discussions/{id}`: saves the subscriber list and the Active switch (739), then reloads the page.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    form: FormPairs,
) -> Result<Response, AppError> {
    if let Err(refused) = form.only(&["active", "subscriber"]) {
        return Ok(*refused);
    }
    let mut conn = lock(&state)?;
    let active = !form.one("active").is_empty();
    match discussions::discussion_edit(&mut conn, &id, Some(active), Some(&form.all("subscriber")), &actor) {
        Ok(()) => {
            let mut res = go(&headers, &discussion_href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Saved", true));
            Ok(res)
        }
        Err(e) => command_error("discussion_edit", e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Comments (OP-ACTIVITY-07)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CommentForm {
    comment: String,
}

/// `POST /discussions/{id}/comments` (OP-ACTIVITY-07): adds the comment and answers with the comment list.
pub async fn comment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    axum::Form(form): axum::Form<CommentForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    match discussions::comment_add(&mut conn, &id, &form.comment, &actor) {
        Ok(_) => {
            let d = discussions::discussion_get(&conn, &id).map_err(|e| AppError::Internal(e.to_string()))?;
            let html = Comments { d: &d }.render()?;
            Ok(([("HX-Trigger", toast("Comment added", false))], Html(html)).into_response())
        }
        Err(CommandError::Validation(errors)) => {
            let message = errors.first().map(|e| e.message.clone()).unwrap_or_default();
            Ok((StatusCode::CONFLICT, message).into_response())
        }
        Err(e) => command_error("discussion_comment_add", e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The bell (OP-ACTIVITY-09)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "partials/bell.html")]
struct Bell {
    rows: Vec<discussions::Notification>,
    unread: i64,
    open: bool,
}

impl Bell {
    fn date(&self, iso: &str) -> String {
        display_date(iso)
    }
}

fn bell(conn: &rusqlite::Connection, actor: &crate::audit::Actor, open: bool) -> Result<Response, AppError> {
    let (rows, unread) =
        discussions::notification_list(conn, actor, 20).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Html(Bell { rows, unread, open }.render()?).into_response())
}

/// `GET /notifications`: the bell of the top bar with the unread count and the latest notifications. Each page's
/// top bar loads it once (hx-trigger load).
pub async fn notifications(
    State(state): State<AppState>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    bell(&conn, &actor, false)
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ReadForm {
    id: String,
}

/// `POST /notifications/read` (form `id`): marks one notification read (layout 101) and answers with the bell.
pub async fn mark_read(
    State(state): State<AppState>,
    CurrentUser(actor): CurrentUser,
    axum::Form(form): axum::Form<ReadForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    match discussions::notification_mark_read(&mut conn, &form.id, &actor) {
        Ok(()) => bell(&conn, &actor, true),
        Err(e) => command_error("notification_mark_read", e),
    }
}
