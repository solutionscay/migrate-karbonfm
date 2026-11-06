//! Relations on UI-04's Contacts tab (OP-PARTY-07..09) and the pickers of UI-06 (OP-PARTY-05 search, OP-PARTY-06
//! find by contact). Relation changes answer with the Contacts fragment (#party-relations).

use super::contact_forms::{ConfirmDialog, ConfirmQuery};
use super::current_user::CurrentUser;
use super::initials;
use super::parties::{color, page_error, party_href, Relations};
use super::party_forms::toast;
use crate::admin::{CommandError, FieldError};
use crate::contacts::{self, MethodKind};
use crate::parties::{self, Found, Kind, PartyRow, RelationChange, RelationInput, Search};
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

fn dialog(view: impl Template, status: StatusCode) -> Result<Response, AppError> {
    Ok((status, Html(view.render()?)).into_response())
}

fn relation_href(id: &str) -> String {
    format!("/relations/{}", urlencode(id))
}

/// The Contacts fragment of `party_id`, out of band (after a dialog, which closes) or as the answer.
fn relations_response(
    conn: &rusqlite::Connection,
    party_id: &str,
    oob: bool,
    text: &str,
) -> Result<Response, AppError> {
    let d = parties::party_get(conn, party_id).map_err(|e| page_error("party_get", e))?;
    let html = Relations { d: &d, oob }.render()?;
    Ok(([("HX-Trigger", toast(text, oob))], Html(html)).into_response())
}

/// The party a Contacts-tab row belongs to: the row is other -> party, so it is party_id_to.
fn party_of(conn: &rusqlite::Connection, relation_id: &str) -> Result<String, AppError> {
    conn.query_row("SELECT party_id_to FROM party_relation WHERE id = ?1", [relation_id], |r| r.get(0))
        .map_err(AppError::from)
}

// ---------------------------------------------------------------------------------------------------------------
// Create, edit, end, restore, delete
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct RelationForm {
    pub other_id: String,
    /// The picker's search box rides along with the form.
    pub q: String,
    pub relationship: String,
    pub reciprocal: String,
    pub billing: String,
    pub shipping: String,
}

#[derive(Template)]
#[template(path = "parties/relation_dialog.html")]
pub struct RelationDialog {
    /// `POST /parties/{id}/relations` or `PATCH /relations/{id}`.
    pub action: String,
    pub edit: bool,
    pub party_name: String,
    /// The other party (an edit, or a create with a choice made).
    pub other_name: String,
    pub form: RelationForm,
    pub picker: String,
    pub errors: Vec<FieldError>,
    pub failure: Option<String>,
}

impl RelationDialog {
    pub fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

/// `GET /parties/{id}/relations/new`: choose the other party with the contact picker and say how they relate.
pub async fn new_dialog(State(state): State<AppState>, Path(party_id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let d = parties::party_get(&conn, &party_id).map_err(|e| page_error("party_get", e))?;
    let picker = contact_picker(&conn, "", &party_id, "")?;
    dialog(
        RelationDialog {
            action: format!("{}/relations", party_href(&party_id)),
            edit: false,
            party_name: d.name,
            other_name: String::new(),
            form: RelationForm::default(),
            picker,
            errors: Vec::new(),
            failure: None,
        },
        StatusCode::OK,
    )
}

/// `POST /parties/{id}/relations` (OP-PARTY-07).
pub async fn create(
    State(state): State<AppState>,
    Path(party_id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<RelationForm>,
) -> Result<Response, AppError> {
    let input = RelationInput {
        party_id: party_id.clone(),
        other_id: form.other_id.clone(),
        relationship: form.relationship.clone(),
        reciprocal: form.reciprocal.clone(),
        billing: !form.billing.is_empty(),
        shipping: !form.shipping.is_empty(),
    };
    let mut conn = lock(&state)?;
    match parties::party_relation_create(&mut conn, &input, &actor) {
        Ok(_) => relations_response(&conn, &party_id, true, "Related"),
        Err(e @ (CommandError::Validation(_) | CommandError::Refused(_))) => {
            let d = parties::party_get(&conn, &party_id).map_err(|e| page_error("party_get", e))?;
            let picker = contact_picker(&conn, &form.q, &party_id, &form.other_id)?;
            let mut view = RelationDialog {
                action: format!("{}/relations", party_href(&party_id)),
                edit: false,
                party_name: d.name,
                other_name: String::new(),
                form,
                picker,
                errors: Vec::new(),
                failure: None,
            };
            match e {
                CommandError::Validation(errors) => view.errors = errors,
                CommandError::Refused(message) => view.failure = Some(message),
                _ => unreachable!(),
            }
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => Err(page_error("party_relation_create", e)),
    }
}

/// `GET /relations/{id}/edit`: the texts and flags of the pair, from the party's side.
pub async fn edit_dialog(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let party_id = party_of(&conn, &id)?;
    let d = parties::party_get(&conn, &party_id).map_err(|e| page_error("party_get", e))?;
    let r = d.relations.iter().find(|r| r.id == id).ok_or(AppError::NotFound)?;
    let on = |b: bool| if b { "on".to_owned() } else { String::new() };
    dialog(
        RelationDialog {
            action: relation_href(&id),
            edit: true,
            party_name: d.name.clone(),
            other_name: r.other_name.clone(),
            form: RelationForm {
                other_id: r.other_id.clone(),
                q: String::new(),
                relationship: r.relationship.clone(),
                reciprocal: r.reciprocal.clone(),
                billing: on(r.billing),
                shipping: on(r.shipping),
            },
            picker: String::new(),
            errors: Vec::new(),
            failure: None,
        },
        StatusCode::OK,
    )
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PatchForm {
    /// edit (the default), end or restore.
    change: String,
    relationship: String,
    reciprocal: String,
    billing: String,
    shipping: String,
}

/// `PATCH /relations/{id}` (OP-PARTY-08): edit (from the dialog), end or restore (from the card).
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<PatchForm>,
) -> Result<Response, AppError> {
    let (change, from_dialog, text) = match form.change.as_str() {
        "end" => (RelationChange::End, false, "Ended"),
        "restore" => (RelationChange::Restore, false, "Restored"),
        _ => (
            RelationChange::Edit {
                relationship: form.relationship.clone(),
                reciprocal: form.reciprocal.clone(),
                billing: !form.billing.is_empty(),
                shipping: !form.shipping.is_empty(),
            },
            true,
            "Saved",
        ),
    };
    let mut conn = lock(&state)?;
    let party_id = party_of(&conn, &id)?;
    match parties::party_relation_update(&mut conn, &id, &change, &actor) {
        Ok(()) => relations_response(&conn, &party_id, from_dialog, text),
        Err(CommandError::Refused(message)) => Ok((StatusCode::CONFLICT, message).into_response()),
        Err(e) => Err(page_error("party_relation_update", e)),
    }
}

/// `DELETE /relations/{id}` (OP-PARTY-09, AP-07): the confirmation dialog, then with confirm=true both rows go.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ConfirmQuery>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let party_id = party_of(&conn, &id)?;
    match parties::party_relation_delete(&mut conn, &id, q.yes(), &actor) {
        Ok(()) => relations_response(&conn, &party_id, true, "Relation deleted"),
        Err(CommandError::ConfirmationRequired(summary)) => dialog(
            ConfirmDialog {
                title: "Delete this relation?".into(),
                summary,
                action: format!("{}?confirm=true", relation_href(&id)),
                button: "Delete".into(),
            },
            StatusCode::OK,
        ),
        Err(CommandError::Refused(message)) => Ok((StatusCode::CONFLICT, message).into_response()),
        Err(e) => Err(page_error("party_relation_delete", e)),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// UI-06 pickers
// ---------------------------------------------------------------------------------------------------------------

/// The contact picker (UI-06): a search box (OP-PARTY-05) and radio rows named `other_id`. htmx swaps the rows as
/// the user types.
#[derive(Template)]
#[template(path = "parties/contact_picker.html")]
pub struct ContactPicker {
    pub q: String,
    pub exclude: String,
    pub selected: String,
    pub rows: Vec<(PartyRow, String, usize)>,
    pub matched: i64,
}

pub fn contact_picker(conn: &rusqlite::Connection, q: &str, exclude: &str, selected: &str) -> Result<String, AppError> {
    let found =
        parties::party_search(conn, &Search { query: q.into(), kind: Kind::All, page_size: 8, ..Default::default() })
            .map_err(|e| page_error("party_search", e))?;
    let rows = found
        .rows
        .into_iter()
        .filter(|r| r.id != exclude)
        .map(|r| {
            let i = initials(&r.name);
            let c = color(&r.id);
            (r, i, c)
        })
        .collect();
    Ok(ContactPicker { q: q.into(), exclude: exclude.into(), selected: selected.into(), rows, matched: found.matched }
        .render()?)
}

impl ContactPicker {
    /// The search URL; the typed `q` is added by htmx (the input's own value).
    pub fn search_url(&self) -> String {
        format!("/pickers/contact?exclude={}", urlencode(&self.exclude))
    }
}

/// An email or phone picker (UI-06): the party's methods of that kind, primary first, as radio rows.
#[derive(Template)]
#[template(path = "parties/method_picker.html")]
pub struct MethodPicker {
    pub kind: &'static str,
    pub name: String,
    pub rows: Vec<(String, String, String, bool)>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct PickerQuery {
    q: String,
    exclude: String,
    selected: String,
    party_id: String,
    /// The contact picker's radio name: the add-person dialog of an email (UI-08) sends its form, so the email
    /// picker also takes the chosen party as `other_id`.
    other_id: String,
}

/// `GET /pickers/{kind}`: `contact` (`?q=&exclude=&selected=`), `email` or `phone` (`?party_id=`), `item` (`?q=&selected=`,
/// active items only, BR-SALES-16).
pub async fn picker(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Query(q): Query<PickerQuery>,
) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    match kind.as_str() {
        "contact" => Ok(Html(contact_picker(&conn, &q.q, &q.exclude, &q.selected)?).into_response()),
        "item" => Ok(Html(super::items::item_picker(&conn, &q.q, &q.selected)?).into_response()),
        "customer" => Ok(Html(super::sales_forms::customer_picker(&conn, &q.q, &q.selected)?).into_response()),
        "email" | "phone" => {
            let party = if q.party_id.is_empty() { &q.other_id } else { &q.party_id };
            if party.is_empty() {
                // Nobody chosen yet (the add-person dialog asks again on change).
                return Ok(Html(String::new()).into_response());
            }
            let d = parties::party_get(&conn, party).map_err(|e| page_error("party_get", e))?;
            let want = if kind == "email" { MethodKind::Email } else { MethodKind::Phone };
            let rows = contacts::methods_of(&conn, &d.id)?
                .into_iter()
                .filter(|m| m.kind == want)
                .map(|m| (m.id.clone(), m.display(), m.label.clone(), m.primary))
                .collect();
            let view = MethodPicker { kind: if kind == "email" { "email" } else { "phone" }, name: d.name, rows };
            Ok(Html(view.render()?).into_response())
        }
        _ => Err(AppError::NotFound),
    }
}

/// Picker rows of parties found by email or phone (OP-PARTY-06).
#[derive(Template)]
#[template(path = "parties/found_rows.html")]
pub struct FoundRows {
    pub rows: Vec<(Found, String)>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ByContactQuery {
    email: String,
    phone: String,
    /// "0" turns off the organization preference (BR-PARTY-11), which is on by default.
    prefer_org: String,
}

/// `GET /parties/by-contact?email=&phone=` (OP-PARTY-06, BR-PARTY-11): the parties that own the email or phone, the
/// employer organization first.
pub async fn by_contact(State(state): State<AppState>, Query(q): Query<ByContactQuery>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let split = |s: &str| s.split(',').map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
    let found = parties::party_find_by_contact(&conn, &split(&q.email), &split(&q.phone), q.prefer_org != "0")
        .map_err(|e| page_error("party_find_by_contact", e))?;
    let rows = found.into_iter().map(|f| {
        let href = party_href(&f.id);
        (f, href)
    });
    Ok(Html(FoundRows { rows: rows.collect() }.render()?).into_response())
}
