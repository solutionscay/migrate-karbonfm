//! UI-01 Welcome setup (OP-ADMIN-01 `setup_welcome`, BR-ADMIN-01) and the setup gate.
//!
//! The wizard is organization -> person -> settings -> done. Each step posts the whole form to `POST /setup`; values
//! of the other steps ride along as hidden inputs, so nothing is written until the last step, which runs the one
//! command in one transaction. Until setup is done, every page answers with a redirect to /setup; a demo database
//! (seeded with initial_setup_done_ind = 1) counts as set up.

use crate::admin::{self, CommandError, FieldError, SetupInput};
use crate::web::{is_htmx, render, AppError, ErrorDetail};
use crate::AppState;
use askama::Template;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::atomic::Ordering;

/// Sends the page to `to`: a 303 for a normal request, `HX-Redirect` for htmx (which would otherwise swap the target).
pub fn redirect(headers: &HeaderMap, to: &'static str) -> Response {
    if is_htmx(headers) {
        ([("HX-Redirect", HeaderValue::from_static(to))], "").into_response()
    } else {
        (StatusCode::SEE_OTHER, [(header::LOCATION, HeaderValue::from_static(to))]).into_response()
    }
}

/// True when setup is done; cached in the state once true.
fn is_done(state: &AppState) -> Result<bool, AppError> {
    if state.setup_done.load(Ordering::Relaxed) {
        return Ok(true);
    }
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let done = admin::setup_done(&conn)?;
    if done {
        state.setup_done.store(true, Ordering::Relaxed);
    }
    Ok(done)
}

/// The setup gate (BR-ADMIN-01), inside the AP-13 guard: before setup, every route but /setup, the static files and
/// the launch route redirects to /setup; after setup, /setup redirects to /.
pub async fn setup_gate(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if path.starts_with("/assets/") || path.starts_with("/session/") {
        return next.run(req).await;
    }
    let done = match is_done(&state) {
        Ok(done) => done,
        Err(e) => return e.into_response(),
    };
    match (done, path == "/setup") {
        (false, false) => redirect(req.headers(), "/setup"),
        (true, true) => redirect(req.headers(), "/"),
        _ => next.run(req).await,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Organization,
    Person,
    Settings,
}

impl Step {
    fn parse(s: &str) -> Self {
        match s {
            "person" => Step::Person,
            "settings" => Step::Settings,
            _ => Step::Organization,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Step::Organization => "organization",
            Step::Person => "person",
            Step::Settings => "settings",
        }
    }
    pub fn number(self) -> usize {
        self as usize + 1
    }
    fn next(self) -> Option<Self> {
        match self {
            Step::Organization => Some(Step::Person),
            Step::Person => Some(Step::Settings),
            Step::Settings => None,
        }
    }
    fn back(self) -> Self {
        match self {
            Step::Settings => Step::Person,
            _ => Step::Organization,
        }
    }
    /// The form fields shown on this step; the others are carried as hidden inputs.
    fn fields(self) -> &'static [&'static str] {
        match self {
            Step::Organization => &["organization_name"],
            Step::Person => &["first_name", "last_name", "job_title", "email"],
            Step::Settings => &["tax_rate", "country_code", "area_code", "format_phone_numbers"],
        }
    }
}

/// The wizard form: every field of every step (05: typed form, unknown fields refused).
#[derive(Deserialize, Clone, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct SetupForm {
    pub step: String,
    /// "back" goes one step back without checking the fields.
    pub go: String,
    pub organization_name: String,
    pub first_name: String,
    pub last_name: String,
    pub job_title: String,
    pub email: String,
    pub tax_rate: String,
    pub country_code: String,
    pub area_code: String,
    /// The checkbox: "on" when checked, absent (empty) when not.
    pub format_phone_numbers: String,
}

impl Default for SetupForm {
    fn default() -> Self {
        Self {
            step: String::new(),
            go: String::new(),
            organization_name: String::new(),
            first_name: String::new(),
            last_name: String::new(),
            job_title: String::new(),
            email: String::new(),
            tax_rate: String::new(),
            country_code: "1".into(),
            area_code: String::new(),
            format_phone_numbers: "on".into(),
        }
    }
}

impl SetupForm {
    fn value(&self, field: &str) -> &str {
        match field {
            "organization_name" => &self.organization_name,
            "first_name" => &self.first_name,
            "last_name" => &self.last_name,
            "job_title" => &self.job_title,
            "email" => &self.email,
            "tax_rate" => &self.tax_rate,
            "country_code" => &self.country_code,
            "area_code" => &self.area_code,
            "format_phone_numbers" => &self.format_phone_numbers,
            _ => "",
        }
    }

    /// Checks the fields of one step.
    fn check(&self, step: Step) -> Vec<FieldError> {
        let mut errors = Vec::new();
        let mut err = |field, message: &str| errors.push(FieldError { field, message: message.into() });
        match step {
            Step::Organization => {
                if self.organization_name.trim().is_empty() {
                    err("organization_name", "Enter the name of your business.");
                }
            }
            Step::Person => {
                if self.first_name.trim().is_empty() {
                    err("first_name", "Enter your first name.");
                }
                if self.last_name.trim().is_empty() {
                    err("last_name", "Enter your last name.");
                }
                if let Err(m) = admin::parse_email(&self.email) {
                    err("email", &m);
                }
            }
            Step::Settings => {
                if let Err(m) = admin::parse_percent_bp(&self.tax_rate) {
                    err("tax_rate", &m);
                }
                if let Err(m) = admin::parse_country_code(&self.country_code) {
                    err("country_code", &m);
                }
                if let Err(m) = admin::parse_area_code(&self.area_code) {
                    err("area_code", &m);
                }
            }
        }
        errors
    }

    /// The command input; `None` if a field does not check (the last step checks all of them again).
    fn input(&self) -> Result<SetupInput, (Step, Vec<FieldError>)> {
        for step in [Step::Organization, Step::Person, Step::Settings] {
            let errors = self.check(step);
            if !errors.is_empty() {
                return Err((step, errors));
            }
        }
        Ok(SetupInput {
            organization_name: self.organization_name.trim().to_owned(),
            first_name: self.first_name.trim().to_owned(),
            last_name: self.last_name.trim().to_owned(),
            job_title: admin::optional(&self.job_title),
            email: admin::parse_email(&self.email).unwrap_or_default(),
            tax_rate_bp: admin::parse_percent_bp(&self.tax_rate).unwrap_or_default(),
            country_code: admin::parse_country_code(&self.country_code).unwrap_or_else(|_| "1".into()),
            area_code: admin::parse_area_code(&self.area_code).unwrap_or_default(),
            format_phone_numbers: self.format_phone_numbers == "on",
            account_name: admin::os_account_name(),
        })
    }
}

#[derive(Template)]
#[template(path = "setup/step.html")]
pub struct StepView {
    pub step: Step,
    pub form: SetupForm,
    pub errors: Vec<FieldError>,
    /// A failure that is not one field's (the command could not save).
    pub failure: Option<&'static str>,
}

impl StepView {
    pub fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
    pub fn value(&self, field: &str) -> &str {
        self.form.value(field)
    }
    /// (name, value) of the fields of the other steps, for hidden inputs.
    pub fn carried(&self) -> Vec<(&'static str, &str)> {
        [Step::Organization, Step::Person, Step::Settings]
            .into_iter()
            .filter(|s| *s != self.step)
            .flat_map(|s| s.fields().iter().copied())
            .map(|f| (f, self.form.value(f)))
            .filter(|(_, v)| !v.is_empty())
            .collect()
    }
    pub fn steps(&self) -> [(usize, &'static str, bool, bool); 3] {
        let n = self.step.number();
        [(1, "Organization", n == 1, n > 1), (2, "You", n == 2, n > 2), (3, "Settings", n == 3, false)]
    }
}

#[derive(Template)]
#[template(path = "setup/page.html")]
struct SetupPage {
    /// The rendered step: the page and the htmx fragment are the same HTML.
    body: String,
}

fn respond(headers: &HeaderMap, view: StepView, status: StatusCode) -> Result<Response, AppError> {
    let body = view.render()?;
    let html = if is_htmx(headers) { axum::response::Html(body) } else { render(&SetupPage { body })? };
    Ok((status, html).into_response())
}

/// `GET /setup`: the first step.
pub async fn page(headers: HeaderMap) -> Result<Response, AppError> {
    let view = StepView { step: Step::Organization, form: SetupForm::default(), errors: Vec::new(), failure: None };
    respond(&headers, view, StatusCode::OK)
}

/// `POST /setup`: checks the posted step and shows the next one; the last step runs `setup_welcome`.
pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SetupForm>,
) -> Result<Response, AppError> {
    let step = Step::parse(&form.step);
    if form.go == "back" {
        return respond(
            &headers,
            StepView { step: step.back(), form, errors: Vec::new(), failure: None },
            StatusCode::OK,
        );
    }
    let errors = form.check(step);
    if !errors.is_empty() {
        return respond(&headers, StepView { step, form, errors, failure: None }, StatusCode::UNPROCESSABLE_ENTITY);
    }
    if let Some(next) = step.next() {
        return respond(&headers, StepView { step: next, form, errors: Vec::new(), failure: None }, StatusCode::OK);
    }
    let input = match form.input() {
        Ok(input) => input,
        Err((step, errors)) => {
            return respond(&headers, StepView { step, form, errors, failure: None }, StatusCode::UNPROCESSABLE_ENTITY)
        }
    };
    let result = {
        let mut conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        admin::setup_welcome(&mut conn, &input)
    };
    match result {
        Ok(_) => {
            state.setup_done.store(true, Ordering::Relaxed);
            Ok(redirect(&headers, "/"))
        }
        Err(CommandError::AlreadySetUp) => Ok(redirect(&headers, "/")),
        Err(CommandError::Validation(errors)) => {
            respond(&headers, StepView { step, form, errors, failure: None }, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(CommandError::Storage(e)) => {
            // Nothing was saved (one transaction). The page says so; the cause goes to the log (BR-ADMIN-03).
            let view = StepView {
                step,
                form,
                errors: Vec::new(),
                failure: Some("Setup could not be saved, so nothing was changed. Try again."),
            };
            let mut res = respond(&headers, view, StatusCode::UNPROCESSABLE_ENTITY)?;
            res.extensions_mut().insert(ErrorDetail { code: "Storage", message: format!("setup_welcome: {e}") });
            Ok(res)
        }
    }
}
