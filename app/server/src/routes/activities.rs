//! Activity screens (UI-07): one activity's page, the create and edit dialogs (OP-ACTIVITY-01/02), the two-step
//! delete (OP-ACTIVITY-03, AP-07) and the people on an activity (OP-ACTIVITY-04). Field errors answer 422 with the
//! dialog again (D-25); a refusal by a rule answers 409 with its message (AlreadySent, AP-08).

use super::current_user::CurrentUser;
use super::party_forms::{go, toast};
use super::{initials, Nav};
use crate::activities::{self, Activity, Fields, Kind, NewActivity, PartyInput};
use crate::admin::{CommandError, FieldError};
use crate::invoice::display_date;
use crate::reference::{ActivityPartyRole, ActivityType, EntityType};
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

pub fn activity_href(id: &str) -> String {
    format!("/activities/{}", urlencode(id))
}

/// The page that shows an activity of this type: a discussion has its own (UI-09), an email its own (UI-08) once
/// S3 and S5 add them; every other type opens here.
pub fn page_href(id: &str, type_id: i64) -> String {
    match ActivityType::from_id(type_id) {
        Some(ActivityType::Discussion) => format!("/discussions/{}", urlencode(id)),
        _ => activity_href(id),
    }
}

/// Command errors that are neither field errors nor a confirmation: 404, 409 with the rule's message, else 500.
pub fn command_error(op: &str, e: CommandError) -> Result<Response, AppError> {
    match e {
        CommandError::NotFound => Err(AppError::NotFound),
        CommandError::AlreadySent => Ok((StatusCode::CONFLICT, crate::admin::ALREADY_SENT).into_response()),
        CommandError::Refused(message) => Ok((StatusCode::CONFLICT, message).into_response()),
        e => Err(AppError::Internal(format!("{op}: {e}"))),
    }
}

fn dialog(view: impl Template, status: StatusCode) -> Result<Response, AppError> {
    Ok((status, Html(view.render()?)).into_response())
}

/// The icon of an activity type in the timeline and on the page (design/activity.html).
pub fn icon(type_id: i64) -> &'static str {
    match ActivityType::from_id(type_id) {
        Some(ActivityType::Email) => "mail",
        Some(ActivityType::Task) => "task",
        Some(ActivityType::Event | ActivityType::Meeting) => "video",
        Some(ActivityType::Call) => "phone",
        Some(ActivityType::Discussion | ActivityType::Comment) => "comment",
        Some(ActivityType::Sms) => "message",
        _ => "note",
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The page (GET /activities/{id})
// ---------------------------------------------------------------------------------------------------------------

/// The people panel of an activity: its parties with their roles, and the add, edit and remove actions.
#[derive(Template)]
#[template(path = "activities/parties.html")]
pub struct PartiesPanel<'a> {
    pub a: &'a Activity,
    pub oob: bool,
}

impl PartiesPanel<'_> {
    fn initials(&self, name: &str) -> String {
        initials(name)
    }
    fn color(&self, id: &str) -> usize {
        super::parties::color(id)
    }
    fn party_href(&self, id: &str) -> String {
        super::parties::party_href(id)
    }
    fn editable(&self) -> bool {
        !self.a.sent()
    }
    fn row_href(&self, id: &str) -> String {
        format!("/activity-parties/{}", urlencode(id))
    }
    fn add_href(&self) -> String {
        format!("{}/parties/new", activity_href(&self.a.id))
    }
}

#[derive(Template)]
#[template(path = "activities/detail.html")]
struct DetailPage<'a> {
    nav: Nav,
    demo: bool,
    a: &'a Activity,
    parties: String,
}

impl DetailPage<'_> {
    fn icon(&self) -> &'static str {
        icon(self.a.type_id)
    }
    fn href(&self) -> String {
        activity_href(&self.a.id)
    }
    fn date(&self, iso: &str) -> String {
        display_date(iso)
    }
    fn time(&self, t: &str) -> String {
        t.get(..5).unwrap_or(t).to_owned()
    }
    fn editable(&self) -> bool {
        self.a.kind().is_some() && !self.a.sent()
    }
}

/// `GET /activities/{id}` (UI-07): the activity, its details and the people on it. A discussion opens its own page.
pub async fn detail(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let a = match activities::activity_get(&conn, &id) {
        Ok(a) => a,
        Err(e) => return command_error("activity_get", e),
    };
    if a.discussion {
        let to = page_href(&a.id, a.type_id);
        if to != activity_href(&a.id) {
            return Ok(axum::response::Redirect::to(&to).into_response());
        }
    }
    let parties = PartiesPanel { a: &a, oob: false }.render()?;
    Ok(render(&DetailPage { nav: Nav::Activity, demo: crate::db::has_demo_data(&conn)?, a: &a, parties })?
        .into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// Create and edit (OP-ACTIVITY-01, OP-ACTIVITY-02)
// ---------------------------------------------------------------------------------------------------------------

/// The fields of the create and edit dialogs. Unknown fields are refused (05).
#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct ActivityForm {
    pub kind: String,
    pub subject: String,
    pub message: String,
    pub date: String,
    pub time: String,
    pub due_date: String,
    pub due_time: String,
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub location: String,
    pub all_day: String,
    pub caller_name: String,
    pub callback_number: String,
    pub result: String,
    /// The party the activity is about: Regarding (63) on a note, task or call.
    pub regarding_id: String,
    /// Task: Assigned To (53): "me" or empty.
    pub assigned_to: String,
    /// The record it is about (activity_entity): an event or meeting started from a party page.
    pub entity_type_id: String,
    pub entity_id: String,
}

impl ActivityForm {
    fn fields(&self) -> Fields {
        Fields {
            subject: self.subject.clone(),
            message: self.message.clone(),
            date: self.date.clone(),
            time: self.time.clone(),
            due_date: self.due_date.clone(),
            due_time: self.due_time.clone(),
            start_date: self.start_date.clone(),
            start_time: self.start_time.clone(),
            end_date: self.end_date.clone(),
            end_time: self.end_time.clone(),
            location: self.location.clone(),
            all_day: matches!(self.all_day.as_str(), "on" | "true" | "1"),
            caller_name: self.caller_name.clone(),
            callback_number: self.callback_number.clone(),
            result: self.result.clone(),
        }
    }

    fn of(kind: Kind, a: &Activity) -> Self {
        let t = a.task.clone().unwrap_or_default();
        let e = a.event.clone().unwrap_or_default();
        let c = a.call.clone().unwrap_or_default();
        let hm = |s: &str| s.get(..5).unwrap_or(s).to_owned();
        ActivityForm {
            kind: kind.key().into(),
            subject: a.subject.clone(),
            message: a.message.clone(),
            date: a.date_display.clone(),
            time: hm(&a.time_display),
            due_date: t.due_date,
            due_time: hm(&t.due_time),
            start_date: e.start_date,
            start_time: hm(&e.start_time),
            end_date: e.end_date,
            end_time: hm(&e.end_time),
            location: e.location,
            all_day: if e.all_day { "on".into() } else { String::new() },
            caller_name: c.caller_name,
            callback_number: c.callback_number,
            result: c.result,
            ..Default::default()
        }
    }
}

/// The create or edit dialog of a note, task, event, meeting or call.
#[derive(Template)]
#[template(path = "activities/form_dialog.html")]
pub struct FormDialog {
    /// `None` for a create; the activity id for an edit.
    pub edit: Option<String>,
    pub kind: Kind,
    pub form: ActivityForm,
    /// "Regarding Halden & Pryce Architects" when the dialog was opened from a party.
    pub regarding_name: String,
    pub me: String,
    pub errors: Vec<FieldError>,
}

impl FormDialog {
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
    fn title(&self) -> String {
        match &self.edit {
            Some(_) => format!("Edit {}", self.kind.label().to_lowercase()),
            None => format!("New {}", self.kind.label().to_lowercase()),
        }
    }
    fn action(&self) -> String {
        match &self.edit {
            Some(id) => activity_href(id),
            None => "/activities".into(),
        }
    }
    fn is(&self, key: &str) -> bool {
        self.kind.key() == key
    }
    fn dated(&self) -> bool {
        matches!(self.kind, Kind::Note | Kind::Call)
    }
    fn timed_event(&self) -> bool {
        matches!(self.kind, Kind::Event | Kind::Meeting)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    kind: String,
    party_id: String,
}

/// `GET /activities/new?kind=note|task|event|meeting|call&party_id=`: the New dialog; opened from a party, the
/// activity is about that party (Regarding, or the entity link for an event or meeting).
pub async fn new_dialog(
    State(state): State<AppState>,
    Query(q): Query<NewQuery>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    // No kind is a note (the top bar's plain "New activity"); an unknown one or an email (UI-08) is 404.
    let kind = if q.kind.is_empty() { Some(Kind::Note) } else { Kind::parse(&q.kind) };
    let kind = kind.filter(|k| *k != Kind::Email).ok_or(AppError::NotFound)?;
    let conn = lock(&state)?;
    let me = me_name(&conn, &actor)?;
    let mut form = ActivityForm { kind: kind.key().into(), ..Default::default() };
    let mut regarding_name = String::new();
    if !q.party_id.is_empty() {
        regarding_name = conn
            .query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [&q.party_id], |r| r.get(0))
            .map_err(|_| AppError::NotFound)?;
        if matches!(kind, Kind::Event | Kind::Meeting) {
            form.entity_type_id = EntityType::Party.id().to_string();
            form.entity_id = q.party_id.clone();
        } else {
            form.regarding_id = q.party_id.clone();
        }
    }
    if kind == Kind::Task {
        form.assigned_to = "me".into();
    }
    dialog(FormDialog { edit: None, kind, form, regarding_name, me, errors: Vec::new() }, StatusCode::OK)
}

fn me_name(conn: &rusqlite::Connection, actor: &crate::audit::Actor) -> Result<String, AppError> {
    let party = activities::current_party(conn, actor)?;
    Ok(match party {
        Some(p) => conn.query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [p], |r| r.get(0))?,
        None => String::new(),
    })
}

/// `POST /activities` (OP-ACTIVITY-01): creates the activity and opens it.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<ActivityForm>,
) -> Result<Response, AppError> {
    let kind = Kind::parse(&form.kind).filter(|k| *k != Kind::Email).ok_or(AppError::NotFound)?;
    let mut conn = lock(&state)?;
    let mut parties = Vec::new();
    if !form.regarding_id.is_empty() {
        parties.push(PartyInput::new(ActivityPartyRole::Regarding, &form.regarding_id));
    }
    if kind == Kind::Task && form.assigned_to == "me" {
        if let Some(me) = activities::current_party(&conn, &actor)? {
            parties.push(PartyInput::new(ActivityPartyRole::AssignedTo, &me));
        }
    }
    let entity = match (form.entity_type_id.parse::<i64>(), form.entity_id.is_empty()) {
        (Ok(t), false) => Some((t, form.entity_id.clone())),
        _ => None,
    };
    let new = NewActivity { kind, fields: form.fields(), entity, parties };
    match activities::activity_create(&mut conn, &new, &actor) {
        Ok(id) => {
            let to = activity_href(&id);
            let mut res = go(&headers, &to)?;
            res.headers_mut().insert("HX-Trigger", toast(&format!("{} created", kind.label()), true));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let me = me_name(&conn, &actor)?;
            let regarding_name = name_of(&conn, &form.regarding_id, &form.entity_id)?;
            dialog(FormDialog { edit: None, kind, form, regarding_name, me, errors }, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => command_error("activity_create", e),
    }
}

fn name_of(conn: &rusqlite::Connection, a: &str, b: &str) -> Result<String, AppError> {
    let id = if a.is_empty() { b } else { a };
    if id.is_empty() {
        return Ok(String::new());
    }
    Ok(conn
        .query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [id], |r| r.get(0))
        .unwrap_or_default())
}

/// `GET /activities/{id}/edit`: the Edit dialog with the activity's fields.
pub async fn edit_dialog(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let a = match activities::activity_get(&conn, &id) {
        Ok(a) => a,
        Err(e) => return command_error("activity_get", e),
    };
    if a.sent() {
        return command_error("activity_update", CommandError::AlreadySent);
    }
    let kind = a.kind().ok_or(AppError::NotFound)?;
    let me = me_name(&conn, &actor)?;
    let form = ActivityForm::of(kind, &a);
    dialog(
        FormDialog { edit: Some(id), kind, form, regarding_name: String::new(), me, errors: Vec::new() },
        StatusCode::OK,
    )
}

/// `PATCH /activities/{id}` (OP-ACTIVITY-02). An activity left with no subject and no content offers the discard
/// (kontacts 668): the confirmation dialog of the delete.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<ActivityForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let kind = match activities::activity_get(&conn, &id) {
        Ok(a) => a.kind().ok_or(AppError::NotFound)?,
        Err(e) => return command_error("activity_get", e),
    };
    match activities::activity_update(&mut conn, &id, &form.fields(), &actor) {
        Ok(()) => {
            let mut res = go(&headers, &activity_href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Saved", true));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            let me = me_name(&conn, &actor)?;
            let view = FormDialog { edit: Some(id), kind, form, regarding_name: String::new(), me, errors };
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(CommandError::ConfirmationRequired(summary)) => dialog(
            super::contact_forms::ConfirmDialog {
                title: "Discard activity?".into(),
                summary,
                action: format!("{}?confirm=true", activity_href(&id)),
                button: "Discard".into(),
            },
            StatusCode::OK,
        ),
        Err(e) => command_error("activity_update", e),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ConfirmQuery {
    confirm: String,
}

/// Where the page goes after a delete: the first party the activity was about, else the timeline.
fn after_delete(a: &Activity) -> String {
    a.parties
        .iter()
        .find(|p| p.role_id != ActivityPartyRole::Creator.id() && !p.party_id.is_empty())
        .map(|p| super::parties::party_href(&p.party_id))
        .or_else(|| a.entities.iter().find(|e| e.type_id == EntityType::Party.id()).map(|e| e.href.clone()))
        .unwrap_or_else(|| "/activities".into())
}

/// `DELETE /activities/{id}` (OP-ACTIVITY-03, AP-07): first the confirmation dialog with what goes, then with
/// confirm=true the delete, and the page moves to the party it was about.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ConfirmQuery>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let a = match activities::activity_get(&conn, &id) {
        Ok(a) => a,
        Err(e) => return command_error("activity_get", e),
    };
    match activities::activity_delete(&mut conn, &id, q.confirm == "true", &actor) {
        Ok(()) => {
            let mut res = go(&headers, &after_delete(&a))?;
            res.headers_mut().insert("HX-Trigger", toast("Deleted", true));
            Ok(res)
        }
        Err(CommandError::ConfirmationRequired(summary)) => dialog(
            super::contact_forms::ConfirmDialog {
                title: format!("Delete this {}?", a.type_name.to_lowercase()),
                summary,
                action: format!("{}?confirm=true", activity_href(&id)),
                button: "Delete".into(),
            },
            StatusCode::OK,
        ),
        Err(e) => command_error("activity_delete", e),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The people on an activity (OP-ACTIVITY-04)
// ---------------------------------------------------------------------------------------------------------------

/// The add or edit dialog of one activity party: the role (only the roles that fit the type) and the party.
#[derive(Template)]
#[template(path = "activities/party_dialog.html")]
pub struct PartyDialog {
    pub activity_id: String,
    /// The activity_party id for an edit.
    pub row: Option<String>,
    pub roles: Vec<(i64, &'static str)>,
    pub role: i64,
    pub picker: String,
    pub contact_method_id: String,
    pub errors: Vec<FieldError>,
}

impl PartyDialog {
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
    fn action(&self) -> String {
        format!("{}/parties", activity_href(&self.activity_id))
    }
}

/// The roles a party may take on an activity of this type (BR-ACTIVITY-03; 710 s34). The Creator is set by the
/// create and Completed By by the task's Complete (690), so the dialog offers neither.
fn roles_for(type_id: i64) -> Vec<(i64, &'static str)> {
    ActivityPartyRole::ALL
        .iter()
        .filter(|r| {
            r.eligible_for(type_id) && !matches!(r, ActivityPartyRole::Creator | ActivityPartyRole::CompletedBy)
        })
        .map(|r| (r.id(), r.name()))
        .collect()
}

#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct PartyForm {
    pub activity_party_id: String,
    pub role: String,
    pub other_id: String,
    pub q: String,
    pub contact_method_id: String,
    pub email_id: String,
}

/// `GET /activities/{id}/parties/new`: the Add person dialog.
pub async fn party_new(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let target = match activities::Target::of(&conn, &id) {
        Ok(t) => t,
        Err(e) => return command_error("activity_get", e),
    };
    if target.sent {
        return command_error("activity_party_set", CommandError::AlreadySent);
    }
    let roles = roles_for(target.type_id);
    let role = roles.first().map(|r| r.0).unwrap_or(0);
    let picker = super::relation_forms::contact_picker(&conn, "", "", "")?;
    dialog(
        PartyDialog {
            activity_id: id,
            row: None,
            roles,
            role,
            picker,
            contact_method_id: String::new(),
            errors: vec![],
        },
        StatusCode::OK,
    )
}

/// `GET /activity-parties/{id}/edit`: the Edit dialog of one activity party.
pub async fn party_edit_dialog(State(state): State<AppState>, Path(row): Path<String>) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let activity_id = match activities::activity_of_party(&conn, &row) {
        Ok(a) => a,
        Err(e) => return command_error("activity_party_set", e),
    };
    let a = activities::activity_get(&conn, &activity_id).map_err(|e| AppError::Internal(e.to_string()))?;
    if a.sent() {
        return command_error("activity_party_set", CommandError::AlreadySent);
    }
    let p = a.parties.iter().find(|p| p.id == row).ok_or(AppError::NotFound)?;
    let picker = super::relation_forms::contact_picker(&conn, &p.name, "", &p.party_id)?;
    dialog(
        PartyDialog {
            activity_id,
            row: Some(row.clone()),
            roles: roles_for(a.type_id),
            role: p.role_id,
            picker,
            contact_method_id: p.contact_method_id.clone(),
            errors: vec![],
        },
        StatusCode::OK,
    )
}

fn parties_response(conn: &rusqlite::Connection, activity_id: &str, text: &str) -> Result<Response, AppError> {
    let a = activities::activity_get(conn, activity_id).map_err(|e| AppError::Internal(e.to_string()))?;
    let html = PartiesPanel { a: &a, oob: true }.render()?;
    Ok(([("HX-Trigger", toast(text, true))], Html(html)).into_response())
}

/// `PUT /activities/{id}/parties` (OP-ACTIVITY-04): adds a party, or edits the row `activity_party_id`. Answers
/// with the people panel out of band and closes the dialog.
pub async fn party_set(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<PartyForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    let role = form.role.parse::<i64>().ok().and_then(ActivityPartyRole::from_id);
    let result = match role {
        None => Err(CommandError::Validation(vec![FieldError { field: "role", message: "Choose a role.".into() }])),
        Some(role) => {
            let cm = if form.email_id.is_empty() { form.contact_method_id.clone() } else { form.email_id.clone() };
            let p = PartyInput { role, party_id: form.other_id.clone(), contact_method_id: cm };
            if form.activity_party_id.is_empty() {
                activities::activity_party_add(&mut conn, &id, &p, &actor).map(|_| ())
            } else {
                match activities::activity_of_party(&conn, &form.activity_party_id) {
                    Ok(a) if a == id => activities::activity_party_edit(&mut conn, &form.activity_party_id, &p, &actor),
                    Ok(_) => Err(CommandError::NotFound),
                    Err(e) => Err(e),
                }
            }
        }
    };
    match result {
        Ok(()) => parties_response(&conn, &id, if form.activity_party_id.is_empty() { "Added" } else { "Saved" }),
        Err(CommandError::Validation(errors)) => {
            let target = activities::Target::of(&conn, &id).map_err(|e| AppError::Internal(e.to_string()))?;
            let picker = super::relation_forms::contact_picker(&conn, &form.q, "", &form.other_id)?;
            let view = PartyDialog {
                activity_id: id,
                row: (!form.activity_party_id.is_empty()).then(|| form.activity_party_id.clone()),
                roles: roles_for(target.type_id),
                role: role.map(|r| r.id()).unwrap_or(0),
                picker,
                contact_method_id: form.contact_method_id,
                errors,
            };
            dialog(view, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => command_error("activity_party_set", e),
    }
}

/// `DELETE /activity-parties/{id}` (OP-ACTIVITY-04, 638): removes one party from the activity; no confirmation
/// (05 lists none for this command). Answers with the people panel.
pub async fn party_remove(
    State(state): State<AppState>,
    Path(row): Path<String>,
    CurrentUser(actor): CurrentUser,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    match activities::activity_party_remove(&mut conn, &row, &actor) {
        Ok(activity_id) => {
            let a = activities::activity_get(&conn, &activity_id).map_err(|e| AppError::Internal(e.to_string()))?;
            let html = PartiesPanel { a: &a, oob: false }.render()?;
            Ok(([("HX-Trigger", toast("Removed", false))], Html(html)).into_response())
        }
        Err(e) => command_error("activity_party_remove", e),
    }
}
