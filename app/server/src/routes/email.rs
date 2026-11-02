//! Email message screens (UI-08): compose (OP-ACTIVITY-01), the message page with its recipients, edit
//! (OP-ACTIVITY-02), apply a template (OP-TEMPLATE-01) and send (OP-INTEG-01). A sent message is read only (AP-08).
//! The send runs on `spawn_blocking`: it talks to the mail host and may wait on the keychain.

use super::activities::{command_error, PartiesPanel};
use super::current_user::CurrentUser;
use super::party_forms::{go, toast};
use super::Nav;
use crate::activities::{self, Activity};
use crate::admin::{CommandError, FieldError};
use crate::email::{self, NewEmail, SendOutcome};
use crate::secrets::Secret;
use crate::web::{render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

pub fn email_href(id: &str) -> String {
    format!("/email-messages/{}", urlencode(id))
}

// ---------------------------------------------------------------------------------------------------------------
// Compose (OP-ACTIVITY-01)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "email/compose_dialog.html")]
struct ComposeDialog {
    party_id: String,
    party_name: String,
    entity_type_id: String,
    entity_id: String,
    picker: String,
    subject: String,
    message: String,
    errors: Vec<FieldError>,
}

impl ComposeDialog {
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ComposeQuery {
    party_id: String,
    entity_type_id: String,
    entity_id: String,
}

fn email_picker(conn: &rusqlite::Connection, party_id: &str) -> Result<String, AppError> {
    if party_id.is_empty() {
        return Ok(String::new());
    }
    let d = crate::parties::party_get(conn, party_id).map_err(|e| super::parties::page_error("party_get", e))?;
    let rows = crate::contacts::methods_of(conn, &d.id)?
        .into_iter()
        .filter(|m| m.kind == crate::contacts::MethodKind::Email)
        .map(|m| (m.id.clone(), m.display(), m.label.clone(), m.primary))
        .collect();
    Ok(super::relation_forms::MethodPicker { kind: "email", name: d.name, rows }.render()?)
}

/// `GET /email-messages/new?party_id=&entity_type_id=&entity_id=`: the compose dialog. Opened from a party, the
/// party is the recipient (To) and its email addresses are offered.
pub async fn new_dialog(State(state): State<AppState>, Query(q): Query<ComposeQuery>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let party_name = if q.party_id.is_empty() {
        String::new()
    } else {
        conn.query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [&q.party_id], |r| r.get(0))
            .map_err(|_| AppError::NotFound)?
    };
    let picker = email_picker(&conn, &q.party_id)?;
    let view = ComposeDialog {
        party_id: q.party_id,
        party_name,
        entity_type_id: q.entity_type_id,
        entity_id: q.entity_id,
        picker,
        subject: String::new(),
        message: String::new(),
        errors: vec![],
    };
    Ok(Html(view.render()?).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ComposeForm {
    party_id: String,
    email_id: String,
    entity_type_id: String,
    entity_id: String,
    subject: String,
    message: String,
}

/// `POST /email-messages` (OP-ACTIVITY-01, 389): creates the unsent message and opens it.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<ComposeForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let entity = match (form.entity_type_id.parse::<i64>(), form.entity_id.is_empty()) {
        (Ok(t), false) => Some((t, form.entity_id.clone())),
        _ => None,
    };
    let to = if form.party_id.is_empty() { vec![] } else { vec![(form.party_id.clone(), form.email_id.clone())] };
    let new = NewEmail {
        subject: form.subject.clone(),
        message: form.message.clone(),
        entity,
        to,
        use_system_settings: false,
    };
    match email::email_create(&mut conn, &new, &actor) {
        Ok(id) => {
            let mut res = go(&headers, &email_href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Email saved, not sent", true));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let party_name = conn
                .query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [&form.party_id], |r| r.get(0))
                .unwrap_or_default();
            let picker = email_picker(&conn, &form.party_id)?;
            let view = ComposeDialog {
                party_id: form.party_id,
                party_name,
                entity_type_id: form.entity_type_id,
                entity_id: form.entity_id,
                picker,
                subject: form.subject,
                message: form.message,
                errors,
            };
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(view.render()?)).into_response())
        }
        Err(e) => command_error("email_create", e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The message page (UI-08)
// ---------------------------------------------------------------------------------------------------------------

/// The send panel: what is missing, the password question, or the failure of the last try.
#[derive(Template)]
#[template(path = "email/send_panel.html")]
pub struct SendPanel {
    pub id: String,
    pub errors: Vec<FieldError>,
    pub failure: Option<String>,
    /// The account needs a password the keychain does not have (kontacts 775 s4-9).
    pub ask_password: bool,
}

impl SendPanel {
    fn href(&self) -> String {
        email_href(&self.id)
    }
}

#[derive(Template)]
#[template(path = "email/page.html")]
struct EmailPage<'a> {
    nav: Nav,
    demo: bool,
    a: &'a Activity,
    parties: String,
    templates: Vec<(i64, String)>,
    send: String,
    errors: Vec<FieldError>,
}

impl EmailPage<'_> {
    fn href(&self) -> String {
        email_href(&self.a.id)
    }
    fn delete_href(&self) -> String {
        super::activities::activity_href(&self.a.id)
    }
    fn system(&self) -> bool {
        self.a.email.as_ref().is_some_and(|e| e.use_system_settings)
    }
    fn stamp(&self, ts: &str) -> String {
        crate::invoice::display_stamp(ts)
    }
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

fn page_view(conn: &rusqlite::Connection, id: &str, errors: Vec<FieldError>) -> Result<Response, AppError> {
    let a = match activities::activity_get(conn, id) {
        Ok(a) if a.email.is_some() => a,
        Ok(_) => return Err(AppError::NotFound),
        Err(e) => return command_error("activity_get", e),
    };
    let parties = PartiesPanel { a: &a, oob: false }.render()?;
    let templates = crate::templates::template_list(conn)?.into_iter().map(|t| (t.id, t.name)).collect();
    let send = SendPanel { id: a.id.clone(), errors: vec![], failure: None, ask_password: false }.render()?;
    let page = EmailPage {
        nav: Nav::Activity,
        demo: crate::db::has_demo_data(conn)?,
        a: &a,
        parties,
        templates,
        send,
        errors,
    };
    Ok(render(&page)?.into_response())
}

/// `GET /email-messages/{id}` (UI-08): the message, editable until it is sent, then read only (AP-08).
pub async fn page(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    page_view(&conn, &id, vec![])
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct EditForm {
    subject: String,
    message: String,
    account: String,
}

/// `PATCH /email-messages/{id}` (OP-ACTIVITY-02, 522): subject, message and the account; AlreadySent once sent.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<EditForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let system = form.account == "system";
    match email::email_update(&mut conn, &id, &form.subject, &form.message, system, &actor) {
        Ok(()) => {
            let mut res = go(&headers, &email_href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Saved", false));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let mut res = page_view(&conn, &id, errors)?;
            *res.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
            Ok(res)
        }
        // An email left with no subject and no content: the 668 discard offer, as for any activity.
        Err(CommandError::ConfirmationRequired(summary)) => {
            let view = super::contact_forms::ConfirmDialog {
                title: "Discard this email?".into(),
                summary,
                action: format!("{}?confirm=true", super::activities::activity_href(&id)),
                button: "Discard".into(),
            };
            let mut res = Html(view.render()?).into_response();
            res.headers_mut().insert("HX-Retarget", "#modal-body".parse().expect("static header"));
            Ok(res)
        }
        Err(e) => command_error("email_update", e),
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TemplateForm {
    template_id: String,
}

/// `POST /email-messages/{id}/template` (form `template_id`): applies the merged template to the message (kontacts
/// 343/346; subject unchanged, template_id set).
pub async fn apply_template(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<TemplateForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let template = form.template_id.parse::<i64>().unwrap_or(0);
    match email::email_apply_template(&mut conn, &id, template, &actor) {
        Ok(()) => {
            let mut res = go(&headers, &email_href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Template applied", false));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let message = errors.first().map(|e| e.message.clone()).unwrap_or_default();
            Ok((StatusCode::CONFLICT, message).into_response())
        }
        Err(e) => command_error("email_apply_template", e),
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SendForm {
    /// The SMTP password asked by the send dialog when the keychain has none (kontacts 775 s4-9). Write-only: it
    /// is never rendered back (AP-03).
    password: String,
}

/// `POST /email-messages/{id}/send` (OP-INTEG-01): sends the message. Success reloads the page, now read only. A
/// missing item or a failure answers 422 with the send panel; nothing is written and the message stays unsent.
pub async fn send(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<SendForm>,
) -> Result<Response, AppError> {
    let password = (!form.password.is_empty()).then(|| Secret::new(form.password.clone()));
    let (db, secrets, mailer) = (state.db.clone(), state.secret_store(), state.mailer.clone());
    let task_id = id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        email::email_send(&db, &secrets, mailer.as_ref(), None, &task_id, &actor, password.as_ref())
    })
    .await
    .map_err(|e| AppError::Internal(format!("send task: {e}")))?;
    let panel = |errors: Vec<FieldError>, failure: Option<String>, ask_password: bool| -> Result<Response, AppError> {
        let html = SendPanel { id: id.clone(), errors, failure, ask_password }.render()?;
        Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
    };
    match outcome {
        Ok(SendOutcome::Sent(_)) => {
            let mut res = go(&headers, &email_href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Email sent", false));
            Ok(res)
        }
        Ok(SendOutcome::Failed { error, forgot_password }) => {
            let text = if forgot_password {
                format!("{error}. Enter your SMTP password again.")
            } else {
                format!("{error}. It was not sent; try again later.")
            };
            panel(vec![], Some(text), forgot_password)
        }
        Err(CommandError::Validation(errors)) => {
            let ask = errors.iter().any(|e| e.field == "password");
            let rest = if ask && errors.len() == 1 { vec![] } else { errors };
            panel(rest, None, ask)
        }
        Err(CommandError::Refused(message)) => panel(vec![], Some(message), false),
        Err(e) => command_error("email_send", e),
    }
}
