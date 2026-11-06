//! UI-05 Create party (OP-PARTY-11 `party_create_wizard`, BR-PARTY-09, BR-PARTY-02 duplicate check).
//!
//! Two steps, like setup: names -> details -> save. Each step posts the whole form to `POST /parties/wizard`; the
//! other step's values ride along as hidden inputs and nothing is written until the save, which runs the one
//! command in one transaction.

use super::current_user::CurrentUser;
use super::parties::party_href;
use super::party_forms::go;
use super::{demo, Nav};
use crate::admin::{CommandError, FieldError};
use crate::parties::{
    self, Kind, NewParty, OrganizationFields, PartyFields, PartyRow, PersonFields, RoleKind, Search, WizardInput,
};
use crate::web::{is_htmx, render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

/// Every field of both steps. `step` is the step being posted (names or details); `go=back` returns to names.
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct WizardForm {
    pub step: String,
    pub go: String,
    pub org_id: String,
    pub person_id: String,
    pub org_name: String,
    pub person_first_name: String,
    pub person_last_name: String,
    pub org_email: String,
    pub org_phone: String,
    pub org_role: String,
    pub person_email: String,
    pub person_phone: String,
    pub person_job_title: String,
    pub person_role: String,
    pub relationship: String,
    pub billing_contact: String,
}

impl WizardForm {
    fn has_org(&self) -> bool {
        !self.org_name.trim().is_empty()
    }
    fn has_person(&self) -> bool {
        !(self.person_first_name.trim().is_empty() && self.person_last_name.trim().is_empty())
    }
    /// The hidden inputs that carry the fields of the step not shown.
    pub fn carried(&self, step: &str) -> Vec<(&'static str, &str)> {
        let mut out = vec![("org_id", self.org_id.as_str()), ("person_id", self.person_id.as_str())];
        if step == "details" {
            out.extend([
                ("org_name", self.org_name.as_str()),
                ("person_first_name", self.person_first_name.as_str()),
                ("person_last_name", self.person_last_name.as_str()),
            ]);
        } else {
            out.extend([
                ("org_email", self.org_email.as_str()),
                ("org_phone", self.org_phone.as_str()),
                ("org_role", self.org_role.as_str()),
                ("person_email", self.person_email.as_str()),
                ("person_phone", self.person_phone.as_str()),
                ("person_job_title", self.person_job_title.as_str()),
                ("person_role", self.person_role.as_str()),
                ("relationship", self.relationship.as_str()),
                ("billing_contact", self.billing_contact.as_str()),
            ]);
        }
        out
    }
    fn input(&self) -> WizardInput {
        let role = |s: &str| RoleKind::parse(s);
        let organization = self.has_org().then(|| NewParty {
            id: self.org_id.clone(),
            fields: PartyFields::Organization(OrganizationFields {
                organization_name: self.org_name.clone(),
                ..Default::default()
            }),
            comments: String::new(),
            email: self.org_email.clone(),
            phone: self.org_phone.clone(),
            role: role(&self.org_role),
        });
        let person = self.has_person().then(|| NewParty {
            id: self.person_id.clone(),
            fields: PartyFields::Person(PersonFields {
                first_name: self.person_first_name.clone(),
                last_name: self.person_last_name.clone(),
                job_title: self.person_job_title.clone(),
                ..Default::default()
            }),
            comments: String::new(),
            email: self.person_email.clone(),
            phone: self.person_phone.clone(),
            role: role(&self.person_role),
        });
        WizardInput {
            organization,
            person,
            relationship: self.relationship.clone(),
            billing_contact: !self.billing_contact.is_empty(),
        }
    }
}

#[derive(Template)]
#[template(path = "parties/wizard_step.html")]
pub struct StepView {
    pub step: &'static str,
    pub form: WizardForm,
    pub errors: Vec<FieldError>,
    /// BR-PARTY-02: parties whose names look like the ones typed (shown on the details step).
    pub org_matches: Vec<(PartyRow, String)>,
    pub person_matches: Vec<(PartyRow, String)>,
}

impl StepView {
    pub fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
    pub fn has_org(&self) -> bool {
        self.form.has_org()
    }
    pub fn has_person(&self) -> bool {
        self.form.has_person()
    }
}

#[derive(Template)]
#[template(path = "parties/wizard.html")]
struct WizardPage {
    nav: Nav,
    demo: bool,
    title: &'static str,
    body: String,
}

fn respond(state: &AppState, headers: &HeaderMap, view: StepView, status: StatusCode) -> Result<Response, AppError> {
    respond_titled(state, headers, view, status, "New contact")
}

fn respond_titled(
    state: &AppState,
    headers: &HeaderMap,
    view: StepView,
    status: StatusCode,
    title: &'static str,
) -> Result<Response, AppError> {
    let body = view.render()?;
    let html = if is_htmx(headers) {
        Html(body)
    } else {
        render(&WizardPage { nav: Nav::Parties, demo: demo(state)?, title, body })?
    };
    Ok((status, html).into_response())
}

fn fresh() -> WizardForm {
    WizardForm {
        org_id: crate::ids::new_id(),
        person_id: crate::ids::new_id(),
        org_role: "customer".into(),
        ..Default::default()
    }
}

/// `?role=` of `GET /parties/new`: the organization's role to preselect (`customer`, `lead` or `none`).
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    pub role: String,
}

/// `GET /parties/new?role=` (UI-05): the names step. Every create entry point opens it (the top bar's New ▾ Customer
/// with `role=customer`, its Contact without a role), so every create has the duplicate check
/// of BR-PARTY-02. An unknown role keeps the default (customer).
pub async fn page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<NewQuery>,
) -> Result<Response, AppError> {
    let mut form = fresh();
    match q.role.as_str() {
        "customer" | "lead" => form.org_role = q.role.clone(),
        "none" => form.org_role = String::new(),
        _ => {}
    }
    // Opened for a customer (the top bar, /customers): the page says so.
    let title = if q.role == "customer" { "New customer" } else { "New contact" };
    let view =
        StepView { step: "names", form, errors: Vec::new(), org_matches: Vec::new(), person_matches: Vec::new() };
    respond_titled(&state, &headers, view, StatusCode::OK, title)
}

/// The duplicate check of BR-PARTY-02: the parties of that kind whose names start with the typed words.
fn matches(conn: &rusqlite::Connection, name: &str, kind: Kind) -> Result<Vec<(PartyRow, String)>, AppError> {
    if name.trim().is_empty() {
        return Ok(Vec::new());
    }
    let found = parties::party_search(conn, &Search { query: name.into(), kind, page_size: 5, ..Default::default() })
        .map_err(|e| AppError::Internal(format!("party_search: {e}")))?;
    Ok(found
        .rows
        .into_iter()
        .map(|r| {
            let href = party_href(&r.id);
            (r, href)
        })
        .collect())
}

/// `POST /parties/wizard` (OP-PARTY-11).
pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(mut form): Form<WizardForm>,
) -> Result<Response, AppError> {
    if form.org_id.is_empty() || form.person_id.is_empty() {
        let ids = fresh();
        form.org_id = ids.org_id;
        form.person_id = ids.person_id;
    }
    let view = |step, form: WizardForm, errors| StepView {
        step,
        form,
        errors,
        org_matches: Vec::new(),
        person_matches: Vec::new(),
    };
    if form.go == "back" {
        return respond(&state, &headers, view("names", form, Vec::new()), StatusCode::OK);
    }
    if form.step != "details" {
        // The names step: at least one name, then the details step with the possible duplicates.
        if !form.has_org() && !form.has_person() {
            let errors =
                vec![FieldError { field: "org_name", message: "Enter an organization, a person, or both.".into() }];
            return respond(&state, &headers, view("names", form, errors), StatusCode::UNPROCESSABLE_ENTITY);
        }
        if form.person_role.is_empty() {
            form.person_role = if form.has_org() { "employee" } else { "lead" }.into();
        }
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        let org_matches = matches(&conn, &form.org_name, Kind::Organization)?;
        let person_name = format!("{} {}", form.person_first_name.trim(), form.person_last_name.trim());
        let person_matches = matches(&conn, &person_name, Kind::Person)?;
        let v = StepView { step: "details", form, errors: Vec::new(), org_matches, person_matches };
        drop(conn);
        return respond(&state, &headers, v, StatusCode::OK);
    }
    let result = {
        let mut conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        parties::party_create_wizard(&mut conn, &form.input(), &actor)
    };
    match result {
        Ok((org, person)) => {
            let id = org.or(person).unwrap_or_default();
            let mut res = go(&headers, &party_href(&id))?;
            res.headers_mut().insert("HX-Trigger", super::party_forms::toast("Created", false));
            Ok(res)
        }
        Err(CommandError::AlreadyExists(id)) => go(&headers, &party_href(&id)),
        Err(CommandError::Validation(errors)) => {
            let names = errors.iter().any(|e| matches!(e.field, "org_name" | "person_first_name"));
            let step = if names { "names" } else { "details" };
            respond(&state, &headers, view(step, form, errors), StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => Err(AppError::Internal(format!("party_create_wizard: {e}"))),
    }
}
