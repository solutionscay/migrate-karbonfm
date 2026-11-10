//! The party write forms of UI-03/UI-04: create a person or an organization (OP-PARTY-01/02), edit it (OP-PARTY-03),
//! archive it (OP-PARTY-04) and add a role (OP-PARTY-10). Each form is a dialog body loaded into the shared
//! `<dialog>`; a field error re-renders the dialog with 422 (D-25), a save closes it with a toast.

use super::current_user::CurrentUser;
use super::parties::{page_error, party_href, Header};
use crate::admin::{CommandError, FieldError};
use crate::parties::{self, NewParty, OrganizationFields, PartyDetail, PartyFields, PersonFields, RoleKind};
use crate::web::{is_htmx, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

/// Sends the page to `to`: `HX-Redirect` for htmx, 303 otherwise.
pub fn go(headers: &HeaderMap, to: &str) -> Result<Response, AppError> {
    let value = HeaderValue::from_str(to).map_err(|e| AppError::Internal(format!("redirect {to}: {e}")))?;
    Ok(if is_htmx(headers) {
        ([("HX-Redirect", value)], "").into_response()
    } else {
        (StatusCode::SEE_OTHER, [(header::LOCATION, value)]).into_response()
    })
}

/// An `HX-Trigger` header value with a toast and, when `close`, the dialog-closing event.
pub fn toast(text: &str, close: bool) -> HeaderValue {
    let v = if close {
        serde_json::json!({ "karbon-close-dialog": null, "karbon-toast": text })
    } else {
        serde_json::json!({ "karbon-toast": text })
    };
    HeaderValue::from_str(&ascii_json(&v.to_string())).expect("ASCII JSON is a valid header value")
}

/// JSON with every non-ASCII character written as a `\uXXXX` escape. A header is read as Latin-1 by the browser,
/// so a name like "Tomás" must not travel as raw UTF-8 bytes.
pub fn ascii_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn ascii_json_escapes_non_ascii() {
        assert_eq!(super::ascii_json(r#"{"t":"Tomás 🎉"}"#), r#"{"t":"Tom\u00e1s \ud83c\udf89"}"#);
        let v: serde_json::Value = serde_json::from_str(&super::ascii_json(r#"{"t":"Tomás 🎉"}"#)).unwrap();
        assert_eq!(v["t"], "Tomás 🎉");
    }
}

fn error_of<'a>(errors: &'a [FieldError], field: &str) -> Option<&'a str> {
    errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
}

// ---------------------------------------------------------------------------------------------------------------
// Create (OP-PARTY-01, OP-PARTY-02)
// ---------------------------------------------------------------------------------------------------------------

/// The fields of the create and edit forms. A person form posts the person fields, an organization form the
/// organization fields; anything else is refused (deny_unknown_fields, 05).
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct PersonForm {
    pub id: String,
    pub role: String,
    pub prefix: String,
    pub first_name: String,
    pub middle_name: String,
    pub last_name: String,
    pub suffix: String,
    pub nickname: String,
    pub job_title: String,
    pub department: String,
    pub comments: String,
    pub email: String,
    pub phone: String,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct OrganizationForm {
    pub id: String,
    pub role: String,
    pub organization_name: String,
    pub company_type: String,
    pub federal_tax_id_number: String,
    pub comments: String,
    pub email: String,
    pub phone: String,
}

impl PersonForm {
    fn fields(&self) -> PartyFields {
        PartyFields::Person(PersonFields {
            prefix: self.prefix.clone(),
            first_name: self.first_name.clone(),
            middle_name: self.middle_name.clone(),
            last_name: self.last_name.clone(),
            suffix: self.suffix.clone(),
            nickname: self.nickname.clone(),
            job_title: self.job_title.clone(),
            department: self.department.clone(),
            employer_name: None, // not on the form: an edit keeps it (284 s131-132)
        })
    }
}

impl OrganizationForm {
    fn fields(&self) -> PartyFields {
        PartyFields::Organization(OrganizationFields {
            organization_name: self.organization_name.clone(),
            company_type: self.company_type.clone(),
            federal_tax_id_number: self.federal_tax_id_number.clone(),
        })
    }
}

/// The create or edit dialog of a person or an organization.
#[derive(Template)]
#[template(path = "parties/form_dialog.html")]
pub struct FormDialog {
    /// `None` for a create; the party id for an edit.
    pub edit: Option<String>,
    pub organization: bool,
    pub person: PersonForm,
    pub org: OrganizationForm,
    pub errors: Vec<FieldError>,
    /// A message for the whole form (a refused save).
    pub failure: Option<String>,
}

impl FormDialog {
    pub fn error(&self, field: &str) -> Option<&str> {
        error_of(&self.errors, field)
    }
    pub fn role(&self) -> &str {
        if self.organization {
            &self.org.role
        } else {
            &self.person.role
        }
    }
    pub fn id(&self) -> &str {
        if self.organization {
            &self.org.id
        } else {
            &self.person.id
        }
    }
    pub fn title(&self) -> String {
        let what = if self.organization { "organization" } else { "person" };
        match (&self.edit, RoleKind::parse(self.role())) {
            (Some(_), _) => format!("Edit {what}"),
            (None, Some(RoleKind::Customer)) => "New customer".into(),
            (None, Some(RoleKind::Lead)) => "New lead".into(),
            (None, _) => format!("New {what}"),
        }
    }
    pub fn action(&self) -> String {
        match &self.edit {
            Some(id) => party_href(id),
            None if self.organization => "/parties/organizations".into(),
            None => "/parties/people".into(),
        }
    }
}

fn dialog(view: impl Template, status: StatusCode) -> Result<Response, AppError> {
    Ok((status, Html(view.render()?)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    role: String,
}

fn role_of(text: &str) -> Option<RoleKind> {
    // The create dialogs offer a customer or lead role; employee and vendor come from the detail page.
    RoleKind::parse(text).filter(|r| matches!(r, RoleKind::Customer | RoleKind::Lead))
}

fn new_dialog(organization: bool, role: &str) -> FormDialog {
    let id = crate::ids::new_id();
    let role = role_of(role).map(|r| r.key().to_owned()).unwrap_or_default();
    FormDialog {
        edit: None,
        organization,
        person: PersonForm { id: id.clone(), role: role.clone(), ..Default::default() },
        org: OrganizationForm { id, role, ..Default::default() },
        errors: Vec::new(),
        failure: None,
    }
}

/// `GET /parties/people/new?role=`: the New person dialog with a server-issued id (BR-PARTY-10).
pub async fn new_person(Query(q): Query<NewQuery>) -> Result<Response, AppError> {
    dialog(new_dialog(false, &q.role), StatusCode::OK)
}

/// `GET /parties/organizations/new?role=`: the New organization dialog (the top bar's New customer: role=customer).
pub async fn new_organization(Query(q): Query<NewQuery>) -> Result<Response, AppError> {
    dialog(new_dialog(true, &q.role), StatusCode::OK)
}

async fn create(
    state: AppState,
    headers: HeaderMap,
    actor: crate::audit::Actor,
    mut view: FormDialog,
    new: NewParty,
) -> Result<Response, AppError> {
    let result = {
        let mut conn = lock(&state)?;
        parties::party_create(&mut conn, &new, &actor)
    };
    match result {
        Ok(id) | Err(CommandError::AlreadyExists(id)) => go(&headers, &party_href(&id)),
        Err(CommandError::Validation(errors)) => {
            view.errors = errors;
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => Err(AppError::Internal(format!("party_create: {e}"))),
    }
}

/// `POST /parties/people` (OP-PARTY-01): creates the person and opens it.
pub async fn create_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<PersonForm>,
) -> Result<Response, AppError> {
    let new = NewParty {
        id: form.id.clone(),
        fields: form.fields(),
        comments: form.comments.clone(),
        email: form.email.clone(),
        phone: form.phone.clone(),
        role: role_of(&form.role),
    };
    let view = FormDialog {
        edit: None,
        organization: false,
        person: form,
        org: OrganizationForm::default(),
        errors: Vec::new(),
        failure: None,
    };
    create(state, headers, actor, view, new).await
}

/// `POST /parties/organizations` (OP-PARTY-02): creates the organization and opens it.
pub async fn create_organization(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<OrganizationForm>,
) -> Result<Response, AppError> {
    let new = NewParty {
        id: form.id.clone(),
        fields: form.fields(),
        comments: form.comments.clone(),
        email: form.email.clone(),
        phone: form.phone.clone(),
        role: role_of(&form.role),
    };
    let view = FormDialog {
        edit: None,
        organization: true,
        person: PersonForm::default(),
        org: form,
        errors: Vec::new(),
        failure: None,
    };
    create(state, headers, actor, view, new).await
}

// ---------------------------------------------------------------------------------------------------------------
// Edit (OP-PARTY-03)
// ---------------------------------------------------------------------------------------------------------------

fn edit_dialog(d: &PartyDetail) -> FormDialog {
    let p = d.person.clone().unwrap_or_default();
    let o = d.org.clone().unwrap_or_default();
    FormDialog {
        edit: Some(d.id.clone()),
        organization: d.organization,
        person: PersonForm {
            prefix: p.prefix,
            first_name: p.first_name,
            middle_name: p.middle_name,
            last_name: p.last_name,
            suffix: p.suffix,
            nickname: p.nickname,
            job_title: p.job_title,
            department: p.department,
            comments: d.comments.clone(),
            ..Default::default()
        },
        org: OrganizationForm {
            organization_name: o.organization_name,
            company_type: o.company_type,
            federal_tax_id_number: o.federal_tax_id_number,
            comments: d.comments.clone(),
            ..Default::default()
        },
        errors: Vec::new(),
        failure: None,
    }
}

/// `GET /parties/{id}/edit`: the edit dialog, filled.
pub async fn edit(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let d = parties::party_get(&*lock(&state)?, &id).map_err(|e| page_error("party_get", e))?;
    if d.archived {
        return Ok((StatusCode::CONFLICT, "An archived party cannot be edited.").into_response());
    }
    dialog(edit_dialog(&d), StatusCode::OK)
}

/// Both edit forms in one: the kind of the party decides which fields are read (a person's form has no
/// organization_name, an organization's no first_name).
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct EditForm {
    pub prefix: Option<String>,
    pub first_name: Option<String>,
    pub middle_name: Option<String>,
    pub last_name: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub organization_name: Option<String>,
    pub company_type: Option<String>,
    pub federal_tax_id_number: Option<String>,
    pub comments: String,
}

/// `PATCH /parties/{id}` (OP-PARTY-03): saves the edit, closes the dialog and swaps the header in place.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<EditForm>,
) -> Result<Response, AppError> {
    let s = |v: &Option<String>| v.clone().unwrap_or_default();
    let organization = form.organization_name.is_some();
    let person = PersonForm {
        prefix: s(&form.prefix),
        first_name: s(&form.first_name),
        middle_name: s(&form.middle_name),
        last_name: s(&form.last_name),
        suffix: s(&form.suffix),
        nickname: s(&form.nickname),
        job_title: s(&form.job_title),
        department: s(&form.department),
        comments: form.comments.clone(),
        ..Default::default()
    };
    let org = OrganizationForm {
        organization_name: s(&form.organization_name),
        company_type: s(&form.company_type),
        federal_tax_id_number: s(&form.federal_tax_id_number),
        comments: form.comments.clone(),
        ..Default::default()
    };
    let fields = if organization { org.fields() } else { person.fields() };
    let mut view = FormDialog { edit: Some(id.clone()), organization, person, org, errors: Vec::new(), failure: None };
    let mut conn = lock(&state)?;
    match parties::party_update(&mut conn, &id, &fields, &form.comments, &actor) {
        Ok(()) => {
            let d = parties::party_get(&conn, &id).map_err(|e| page_error("party_get", e))?;
            let header = Header { oob: true, ..Header::for_party(&conn, &d)? }.render()?;
            Ok(([("HX-Trigger", toast("Saved", true))], Html(header)).into_response())
        }
        Err(CommandError::Validation(errors)) => {
            view.errors = errors;
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(CommandError::Refused(message)) => {
            view.failure = Some(message);
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => Err(page_error("party_update", e)),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Archive (OP-PARTY-04)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "parties/archive_dialog.html")]
pub struct ArchiveDialog {
    pub href: String,
    pub name: String,
    pub sales: i64,
}

/// `GET /parties/{id}/archive`: asks for a reason and says what stays.
pub async fn archive_dialog(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let d = parties::party_get(&conn, &id).map_err(|e| page_error("party_get", e))?;
    let sales: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sales_transaction st JOIN party_role r ON r.id = st.id_customer
         WHERE r.party_id = ?1 AND COALESCE(st.deleted, 0) = 0",
        [&id],
        |r| r.get(0),
    )?;
    dialog(ArchiveDialog { href: party_href(&d.id), name: d.name, sales }, StatusCode::OK)
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ArchiveForm {
    reason: String,
}

/// `POST /parties/{id}/archive` (OP-PARTY-04, BR-PARTY-01): archives and goes back to the list, where the party is
/// no longer shown.
pub async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<ArchiveForm>,
) -> Result<Response, AppError> {
    {
        let mut conn = lock(&state)?;
        parties::party_archive(&mut conn, &id, &form.reason, &actor).map_err(|e| page_error("party_archive", e))?;
    }
    let mut res = go(&headers, "/parties")?;
    res.headers_mut().insert("HX-Trigger", toast("Archived", true));
    Ok(res)
}

// ---------------------------------------------------------------------------------------------------------------
// Roles (OP-PARTY-10)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RoleForm {
    role: String,
}

/// `POST /parties/{id}/roles` (OP-PARTY-10): adds the role (idempotent) and answers with the header.
pub async fn add_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<RoleForm>,
) -> Result<Response, AppError> {
    let Some(role) = RoleKind::parse(&form.role) else {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, "Choose customer, lead, employee or vendor.").into_response());
    };
    let mut conn = lock(&state)?;
    match parties::party_role_add(&mut conn, &id, role, &actor) {
        Ok(_) => {
            let d = parties::party_get(&conn, &id).map_err(|e| page_error("party_get", e))?;
            let header = Header::for_party(&conn, &d)?.render()?;
            let text = format!("{} is now a {}", d.name, role.label().to_lowercase());
            Ok(([("HX-Trigger", toast(&text, false))], Html(header)).into_response())
        }
        Err(CommandError::Refused(message)) => Ok((StatusCode::CONFLICT, message).into_response()),
        Err(e) => Err(page_error("party_role_add", e)),
    }
}
