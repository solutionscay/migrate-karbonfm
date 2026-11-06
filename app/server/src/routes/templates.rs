//! UI-16 Templates (layouts 111, 112): the list, the New dialog, the editor that saves in place, the merge fields
//! (OP-TEMPLATE-02) and a merge preview for an email (OP-TEMPLATE-01). Commands are in `crate::templates`.

use super::current_user::CurrentUser;
use super::party_forms::toast;
use super::Nav;
use crate::admin::{CommandError, FieldError};
use crate::templates::{self, MergeFieldRow, MergePreview, TemplateDetail, TemplateInput, TemplateRow, FIELDS};
use crate::web::{render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use rusqlite::Connection;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

fn command_error(op: &str, e: CommandError) -> AppError {
    match e {
        CommandError::NotFound => AppError::NotFound,
        e => AppError::Internal(format!("{op}: {e}")),
    }
}

fn error_of<'a>(errors: &'a [FieldError], field: &str) -> Option<&'a str> {
    errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
}

/// "Email, Note" for the list.
fn type_names(ids: &[i64]) -> String {
    ids.iter().filter_map(|id| templates::activity_type_name(*id)).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------------------------------------------------
// GET /templates
// ---------------------------------------------------------------------------------------------------------------

pub struct ListRow {
    pub href: String,
    pub name: String,
    pub subject: String,
    pub types: String,
    pub merge_fields: i64,
}

impl From<TemplateRow> for ListRow {
    fn from(t: TemplateRow) -> Self {
        Self {
            href: format!("/templates/{}", t.id),
            name: if t.name.is_empty() { "Untitled template".into() } else { t.name },
            subject: t.subject,
            types: type_names(&t.activity_types),
            merge_fields: t.merge_fields,
        }
    }
}

#[derive(Template)]
#[template(path = "templates/list.html")]
struct ListPage {
    nav: Nav,
    demo: bool,
    rows: Vec<ListRow>,
}

/// `GET /templates` (layout 111: Name, Subject).
pub async fn list(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let demo = super::demo(&state)?;
    let conn = lock(&state)?;
    let rows = templates::template_list(&conn)?.into_iter().map(ListRow::from).collect();
    render(&ListPage { nav: Nav::Templates, demo, rows })
}

// ---------------------------------------------------------------------------------------------------------------
// GET /templates/new, POST /templates
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NewForm {
    pub name: String,
}

#[derive(Template)]
#[template(path = "templates/new_dialog.html")]
struct NewDialog {
    name: String,
    errors: Vec<FieldError>,
}

impl NewDialog {
    fn error(&self, field: &str) -> Option<&str> {
        error_of(&self.errors, field)
    }
}

/// `GET /templates/new`: the list's New button.
pub async fn new_dialog() -> Result<Html<String>, AppError> {
    render(&NewDialog { name: String::new(), errors: Vec::new() })
}

/// `POST /templates` (`template_create`): the new template opens in the editor.
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<NewForm>,
) -> Result<Response, AppError> {
    let name = form.name.trim().to_string();
    if name.is_empty() {
        let errors = vec![FieldError { field: "name", message: "Name the template.".into() }];
        let html = NewDialog { name, errors }.render()?;
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response());
    }
    let mut conn = lock(&state)?;
    let input = TemplateInput { name, activity_types: vec![1], ..Default::default() };
    let id = templates::template_create(&mut conn, &input, &actor).map_err(|e| command_error("template_create", e))?;
    Ok((StatusCode::CREATED, [("HX-Redirect", format!("/templates/{id}"))]).into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// GET /templates/{id}, PUT /templates/{id}
// ---------------------------------------------------------------------------------------------------------------

/// One checkbox of "Activity type(s)".
pub struct TypeChoice {
    pub id: i64,
    pub name: &'static str,
    pub checked: bool,
}

#[derive(Template)]
#[template(path = "templates/editor.html")]
pub struct Editor {
    pub id: i64,
    pub name: String,
    pub subject: String,
    pub message: String,
    pub types: Vec<TypeChoice>,
    pub errors: Vec<FieldError>,
    pub fields: MergeFields,
}

impl Editor {
    pub fn error(&self, field: &str) -> Option<&str> {
        error_of(&self.errors, field)
    }

    fn new(d: TemplateDetail, input: Option<TemplateInput>, errors: Vec<FieldError>) -> Self {
        let input = input.unwrap_or_else(|| TemplateInput {
            name: d.name.clone(),
            subject: d.subject.clone(),
            message: d.message.clone(),
            activity_types: d.activity_types.clone(),
        });
        let types = crate::reference::ActivityType::ALL
            .iter()
            .map(|t| TypeChoice { id: t.id(), name: t.name(), checked: input.activity_types.contains(&t.id()) })
            .collect();
        Self {
            id: d.id,
            name: input.name,
            subject: input.subject,
            message: input.message,
            types,
            errors,
            fields: MergeFields { id: d.id, rows: d.merge_fields, error: None, oob: false },
        }
    }
}

/// The merge fields portal of layout 112: name, Required, and "not in use" for a row whose placeholder is gone.
#[derive(Template)]
#[template(path = "templates/merge_fields.html")]
pub struct MergeFields {
    pub id: i64,
    pub rows: Vec<MergeFieldRow>,
    pub error: Option<String>,
    /// Sent out of band with the editor after a save (a new placeholder adds a row).
    pub oob: bool,
}

/// An email the merge preview can use.
pub struct EmailChoice {
    pub id: String,
    pub label: String,
}

#[derive(Template)]
#[template(path = "templates/detail.html")]
struct DetailPage {
    nav: Nav,
    demo: bool,
    title: String,
    editor: Editor,
    fields: &'static [templates::Field],
    emails: Vec<EmailChoice>,
}

/// The latest emails, for the merge preview's choice (an email has the To and From parties the fields read).
fn email_choices(conn: &Connection) -> rusqlite::Result<Vec<EmailChoice>> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(NULLIF(subject, ''), '(no subject)'), COALESCE(date_display, ''), \
         COALESCE(parties_cached, '') FROM activity WHERE email_message_ind = 1 \
         ORDER BY date_display DESC, time_display DESC, id LIMIT 50",
    )?;
    let rows = stmt.query_map([], |r| {
        let (subject, date, parties): (String, String, String) = (r.get(1)?, r.get(2)?, r.get(3)?);
        let mut label = subject;
        if !parties.is_empty() {
            label.push_str(&format!(" · {parties}"));
        }
        if !date.is_empty() {
            label.push_str(&format!(" · {}", crate::invoice::display_date(&date)));
        }
        Ok(EmailChoice { id: r.get(0)?, label })
    })?;
    rows.collect()
}

/// `GET /templates/{id}` (layout 112).
pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Html<String>, AppError> {
    let demo = super::demo(&state)?;
    let conn = lock(&state)?;
    let d = templates::template_get(&conn, id).map_err(|e| command_error("template_get", e))?;
    let title = if d.name.is_empty() { "Untitled template".to_string() } else { d.name.clone() };
    let emails = email_choices(&conn)?;
    render(&DetailPage {
        nav: Nav::Templates,
        demo,
        title,
        editor: Editor::new(d, None, Vec::new()),
        fields: FIELDS,
        emails,
    })
}

/// The editor's form. `activity_type` repeats (a checkbox set), so the form is read as pairs; any other key is a 422
/// (the same answer as `deny_unknown_fields`).
fn template_input(pairs: Vec<(String, String)>) -> Result<TemplateInput, String> {
    let mut input = TemplateInput::default();
    for (k, v) in pairs {
        match k.as_str() {
            "name" => input.name = v,
            "subject" => input.subject = v,
            "message" => input.message = v,
            "activity_type" => input.activity_types.push(v.trim().parse().map_err(|_| "activity_type")?),
            _ => return Err(format!("unknown field `{k}`")),
        }
    }
    Ok(input)
}

/// `PUT /templates/{id}` (`template_save`): saves in place; the answer is the editor, with the merge fields out of
/// band, or the editor with field errors (422) and nothing saved.
pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    CurrentUser(actor): CurrentUser,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Result<Response, AppError> {
    let input = match template_input(pairs) {
        Ok(i) => i,
        Err(e) => return Ok((StatusCode::UNPROCESSABLE_ENTITY, e).into_response()),
    };
    let mut conn = lock(&state)?;
    match templates::template_save(&mut conn, id, &input, &actor) {
        Ok(()) => {
            let d = templates::template_get(&conn, id).map_err(|e| command_error("template_get", e))?;
            let mut editor = Editor::new(d, None, Vec::new());
            editor.fields.oob = true;
            let html = format!("{}{}", editor.render()?, editor.fields.render()?);
            Ok(([("HX-Trigger", toast("Template saved", false))], Html(html)).into_response())
        }
        Err(CommandError::Validation(errors)) => {
            let d = templates::template_get(&conn, id).map_err(|e| command_error("template_get", e))?;
            let html = Editor::new(d, Some(input), errors).render()?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
        }
        Err(e) => Err(command_error("template_save", e)),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// PUT /templates/{id}/merge-fields (OP-TEMPLATE-02)
// ---------------------------------------------------------------------------------------------------------------

/// `PUT /templates/{id}/merge-fields` (`merge_fields_save`): the Required checkboxes, all at once. The form repeats
/// `required` (one per checked row).
pub async fn save_merge_fields(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    CurrentUser(actor): CurrentUser,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Result<Response, AppError> {
    let mut required = Vec::new();
    for (k, v) in pairs {
        match (k.as_str(), v.trim().parse::<i64>()) {
            ("required", Ok(n)) => required.push(n),
            _ => return Ok((StatusCode::UNPROCESSABLE_ENTITY, format!("unknown field `{k}`")).into_response()),
        }
    }
    let mut conn = lock(&state)?;
    let result = templates::merge_fields_save(&mut conn, id, &required, &actor);
    let rows = templates::template_get(&conn, id).map_err(|e| command_error("template_get", e))?.merge_fields;
    match result {
        Ok(()) => {
            let html = MergeFields { id, rows, error: None, oob: false }.render()?;
            Ok(([("HX-Trigger", toast("Merge fields saved", false))], Html(html)).into_response())
        }
        Err(CommandError::Validation(errors)) => {
            let error = errors.into_iter().next().map(|e| e.message);
            let html = MergeFields { id, rows, error, oob: false }.render()?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
        }
        Err(e) => Err(command_error("merge_fields_save", e)),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// POST /templates/{id}/merge (OP-TEMPLATE-01)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MergeForm {
    pub activity_id: String,
    /// The editor's text, saved or not. Absent: the saved template.
    pub subject: Option<String>,
    pub message: Option<String>,
}

#[derive(Template)]
#[template(path = "templates/merged.html")]
struct MergedView {
    preview: Option<MergePreview>,
    error: Option<String>,
}

fn merge_refused(error: String) -> Result<Response, AppError> {
    let html = MergedView { preview: None, error: Some(error) }.render()?;
    Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
}

/// `POST /templates/{id}/merge`: the merged subject and message for an activity, as a fragment. Nothing is written.
pub async fn merge(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<MergeForm>,
) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let saved = templates::template_get(&conn, id).map_err(|e| command_error("template_get", e))?;
    let subject = form.subject.unwrap_or(saved.subject);
    let message = form.message.unwrap_or(saved.message);
    match templates::merge_preview(&conn, form.activity_id.trim(), &subject, &message) {
        Ok(preview) => render(&MergedView { preview: Some(preview), error: None }).map(IntoResponse::into_response),
        // The template exists, so NotFound is the activity: a field error, not a 404 page.
        Err(CommandError::NotFound) => merge_refused("Choose an email to merge with.".into()),
        Err(CommandError::Validation(errors)) => {
            let label = |field: &str| if field == "subject" { "Subject" } else { "Body" };
            let text: Vec<String> = errors.iter().map(|e| format!("{}: {}", label(e.field), e.message)).collect();
            merge_refused(text.join(" "))
        }
        Err(e) => Err(command_error("merge_preview", e)),
    }
}
