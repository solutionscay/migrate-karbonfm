//! Contact methods on UI-04 (OP-CONTACT-01..04): add and edit in a dialog, make primary, toggle a purpose, delete
//! with a confirmation dialog (AP-07). Every change answers with the party header, which lists the methods: in place
//! (`hx-target="#party-header"`) or out of band after a dialog.

use super::current_user::CurrentUser;
use super::parties::{page_error, party_href, Header};
use super::party_forms::toast;
use crate::admin::{CommandError, FieldError};
use crate::contacts::{self, Method, MethodInput, MethodKind};
use crate::reference::ContactMethodPurposeType;
use crate::web::{urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

pub fn method_href(id: &str) -> String {
    format!("/contact-methods/{}", urlencode(id))
}

/// The contact-method form. Only the fields of its kind are shown; every field is optional so that one struct reads
/// every kind's form, and nothing else is accepted (deny_unknown_fields).
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct MethodForm {
    pub kind: String,
    pub label: String,
    pub comment: String,
    pub primary: String,
    pub billing: String,
    pub shipping: String,
    pub mailing: String,
    pub number: String,
    pub extension: String,
    pub country_code: String,
    pub fax: String,
    pub sms: String,
    pub address: String,
    pub line1: String,
    pub line2: String,
    pub line3: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

impl MethodForm {
    fn from_method(m: &Method) -> Self {
        let on = |b: bool| if b { "on".to_owned() } else { String::new() };
        MethodForm {
            kind: m.kind.key().into(),
            label: m.label.clone(),
            comment: m.comment.clone(),
            primary: on(m.primary),
            billing: on(m.billing),
            shipping: on(m.shipping),
            mailing: on(m.mailing),
            number: m.number.clone(),
            extension: m.extension.clone(),
            country_code: m.country_code.map(|c| c.to_string()).unwrap_or_default(),
            fax: on(m.fax),
            sms: on(m.sms),
            address: m.address.clone(),
            line1: m.line1.clone(),
            line2: m.line2.clone(),
            line3: m.line3.clone(),
            city: m.city.clone(),
            state: m.state.clone(),
            postal_code: m.postal_code.clone(),
            country: m.country.clone(),
        }
    }

    /// The command input, or the field errors of the form itself (a country code that is not a number).
    fn input(&self) -> Result<MethodInput, Vec<FieldError>> {
        let country_code = match self.country_code.trim().trim_start_matches('+') {
            "" => None,
            t => match t.parse::<i64>() {
                Ok(n) if (1..=999).contains(&n) => Some(n),
                _ => {
                    return Err(vec![FieldError {
                        field: "country_code",
                        message: "Enter a country code like 1 or 44.".into(),
                    }])
                }
            },
        };
        let mut purposes = Vec::new();
        for (on, p) in [
            (&self.billing, ContactMethodPurposeType::Billing),
            (&self.shipping, ContactMethodPurposeType::Shipping),
            (&self.mailing, ContactMethodPurposeType::Mailing),
        ] {
            if !on.is_empty() {
                purposes.push(p);
            }
        }
        Ok(MethodInput {
            kind: MethodKind::parse(&self.kind),
            label: self.label.clone(),
            comment: self.comment.clone(),
            primary: !self.primary.is_empty(),
            purposes,
            number: self.number.clone(),
            extension: self.extension.clone(),
            country_code,
            fax: !self.fax.is_empty(),
            sms: !self.sms.is_empty(),
            address: self.address.clone(),
            line1: self.line1.clone(),
            line2: self.line2.clone(),
            line3: self.line3.clone(),
            city: self.city.clone(),
            state: self.state.clone(),
            postal_code: self.postal_code.clone(),
            country: self.country.clone(),
        })
    }

    pub fn is(&self, kind: &str) -> bool {
        self.kind == kind
    }
    pub fn on(&self, field: &str) -> bool {
        let v = match field {
            "primary" => &self.primary,
            "billing" => &self.billing,
            "shipping" => &self.shipping,
            "mailing" => &self.mailing,
            "fax" => &self.fax,
            "sms" => &self.sms,
            _ => return false,
        };
        !v.is_empty()
    }
}

#[derive(Template)]
#[template(path = "parties/method_dialog.html")]
pub struct MethodDialog {
    /// `POST /parties/{id}/contact-methods` or `PUT /contact-methods/{id}`.
    pub action: String,
    pub edit: bool,
    pub form: MethodForm,
    pub errors: Vec<FieldError>,
    pub failure: Option<String>,
}

impl MethodDialog {
    pub fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
    pub fn title(&self) -> String {
        let kind =
            MethodKind::parse(&self.form.kind).map(|k| k.label().to_lowercase()).unwrap_or("contact method".into());
        if self.edit {
            format!("Edit {kind}")
        } else {
            format!("Add {kind}")
        }
    }
}

fn dialog(view: impl Template, status: StatusCode) -> Result<Response, AppError> {
    Ok((status, Html(view.render()?)).into_response())
}

/// The party header after a change: out of band (after a dialog, which closes) or as the answer itself.
fn header_response(conn: &rusqlite::Connection, party_id: &str, oob: bool, text: &str) -> Result<Response, AppError> {
    let d = crate::parties::party_get(conn, party_id).map_err(|e| page_error("party_get", e))?;
    let html = Header { oob, ..Header::new(&d) }.render()?;
    Ok(([("HX-Trigger", toast(text, oob))], Html(html)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    kind: String,
}

/// `GET /parties/{id}/contact-methods/new?kind=phone|email|web|address`: the Add dialog.
pub async fn new_dialog(
    State(state): State<AppState>,
    Path(party_id): Path<String>,
    Query(q): Query<NewQuery>,
) -> Result<Response, AppError> {
    crate::parties::party_get(&*lock(&state)?, &party_id).map_err(|e| page_error("party_get", e))?;
    let kind = MethodKind::parse(&q.kind).unwrap_or(MethodKind::Phone);
    dialog(
        MethodDialog {
            action: format!("{}/contact-methods", party_href(&party_id)),
            edit: false,
            form: MethodForm { kind: kind.key().into(), ..Default::default() },
            errors: Vec::new(),
            failure: None,
        },
        StatusCode::OK,
    )
}

fn save_error(mut view: MethodDialog, e: CommandError, op: &str) -> Result<Response, AppError> {
    match e {
        CommandError::Validation(errors) => {
            view.errors = errors;
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        CommandError::Refused(message) => {
            view.failure = Some(message);
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        e => Err(page_error(op, e)),
    }
}

/// `POST /parties/{id}/contact-methods` (OP-CONTACT-01).
pub async fn create(
    State(state): State<AppState>,
    Path(party_id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<MethodForm>,
) -> Result<Response, AppError> {
    let view = MethodDialog {
        action: format!("{}/contact-methods", party_href(&party_id)),
        edit: false,
        form: form.clone(),
        errors: Vec::new(),
        failure: None,
    };
    let input = match form.input() {
        Ok(i) => i,
        Err(errors) => return save_error(view, CommandError::Validation(errors), "contact_method_save"),
    };
    let mut conn = lock(&state)?;
    match contacts::contact_method_create(&mut conn, &party_id, &input, &actor) {
        Ok(_) => header_response(&conn, &party_id, true, "Added"),
        Err(e) => save_error(view, e, "contact_method_save"),
    }
}

/// `GET /contact-methods/{id}/edit`: the Edit dialog, filled.
pub async fn edit_dialog(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let m = contacts::method(&*lock(&state)?, &id)?.ok_or(AppError::NotFound)?;
    dialog(
        MethodDialog {
            action: method_href(&m.id),
            edit: true,
            form: MethodForm::from_method(&m),
            errors: Vec::new(),
            failure: None,
        },
        StatusCode::OK,
    )
}

/// `PUT /contact-methods/{id}` (OP-CONTACT-01).
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<MethodForm>,
) -> Result<Response, AppError> {
    let view =
        MethodDialog { action: method_href(&id), edit: true, form: form.clone(), errors: Vec::new(), failure: None };
    let input = match form.input() {
        Ok(i) => i,
        Err(errors) => return save_error(view, CommandError::Validation(errors), "contact_method_save"),
    };
    let mut conn = lock(&state)?;
    match contacts::contact_method_update(&mut conn, &id, &input, &actor) {
        Ok(()) => {
            let party = contacts::method(&conn, &id)?.ok_or(AppError::NotFound)?.party_id;
            header_response(&conn, &party, true, "Saved")
        }
        Err(e) => save_error(view, e, "contact_method_save"),
    }
}

fn refused(e: CommandError, op: &str) -> Result<Response, AppError> {
    match e {
        CommandError::Refused(message) => Ok((StatusCode::CONFLICT, message).into_response()),
        e => Err(page_error(op, e)),
    }
}

/// `POST /contact-methods/{id}/primary` (OP-CONTACT-02).
pub async fn set_primary(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    match contacts::contact_method_set_primary(&mut conn, &id, &actor) {
        Ok(()) => {
            let m = contacts::method(&conn, &id)?.ok_or(AppError::NotFound)?;
            header_response(
                &conn,
                &m.party_id,
                false,
                &format!("{} is now the primary {}", m.display(), m.kind.label().to_lowercase()),
            )
        }
        Err(e) => refused(e, "contact_method_set_primary"),
    }
}

/// `POST /contact-methods/{id}/purposes/{type_id}` (OP-CONTACT-03): type 1 billing, 2 shipping, 3 mailing.
pub async fn toggle_purpose(
    State(state): State<AppState>,
    Path((id, type_id)): Path<(String, i64)>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let Some(purpose) = ContactMethodPurposeType::ALL.iter().copied().find(|p| p.id() == type_id) else {
        return Err(AppError::NotFound);
    };
    let mut conn = lock(&state)?;
    match contacts::contact_method_toggle_purpose(&mut conn, &id, purpose, &actor) {
        Ok(on) => {
            let party = contacts::method(&conn, &id)?.ok_or(AppError::NotFound)?.party_id;
            let text = format!("{} {}", purpose.name(), if on { "set" } else { "cleared" });
            header_response(&conn, &party, false, &text)
        }
        Err(e) => refused(e, "contact_method_toggle_purpose"),
    }
}

/// The delete confirmation (AP-07): the summary, and a button that sends the same request with confirm=true.
#[derive(Template)]
#[template(path = "parties/confirm_dialog.html")]
pub struct ConfirmDialog {
    pub title: String,
    pub summary: String,
    /// The DELETE URL, with `?confirm=true` (htmx sends a DELETE's parameters in the query string).
    pub action: String,
    pub button: String,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ConfirmQuery {
    confirm: String,
}

impl ConfirmQuery {
    pub fn yes(&self) -> bool {
        self.confirm == "true"
    }
}

/// `DELETE /contact-methods/{id}` (OP-CONTACT-04): first the confirmation dialog, then with confirm=true the delete.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ConfirmQuery>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let party = contacts::method(&conn, &id)?.ok_or(AppError::NotFound)?.party_id;
    match contacts::contact_method_delete(&mut conn, &id, q.yes(), &actor) {
        Ok(()) => header_response(&conn, &party, true, "Deleted"),
        Err(CommandError::ConfirmationRequired(summary)) => dialog(
            ConfirmDialog {
                title: "Delete this contact method?".into(),
                summary,
                action: format!("{}?confirm=true", method_href(&id)),
                button: "Delete".into(),
            },
            StatusCode::OK,
        ),
        Err(e) => refused(e, "contact_method_delete"),
    }
}
