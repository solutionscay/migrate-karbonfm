//! The activity timeline (OP-ACTIVITY-08, design/activity.html), the activity rows the party page and the dashboard
//! share, and the task Complete / Reopen action (OP-ACTIVITY-05).

use super::activities::{command_error, icon, page_href};
use super::current_user::CurrentUser;
use super::party_forms::{go, toast};
use super::Nav;
use crate::activities::{self, KindFilter, ListPage, ListQuery, ListRow, Status};
use crate::invoice::display_date;
use crate::reference::ActivityType;
use crate::web::{is_htmx, render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

/// One row of a timeline: what the template prints.
pub struct RowView {
    pub href: String,
    pub icon: &'static str,
    /// The tl-icon colour class of design/activity.html.
    pub tone: &'static str,
    pub title: String,
    pub meta: String,
    pub excerpt: String,
    /// (text, chip class).
    pub chip: Option<(String, &'static str)>,
    /// The day header shown above the first row of each day ("Today", "Yesterday", "Oct 16, 2026").
    pub day: Option<String>,
}

fn day_label(iso: &str, today: &str, yesterday: &str) -> String {
    if iso.is_empty() {
        "No date".into()
    } else if iso == today {
        "Today".into()
    } else if iso == yesterday {
        "Yesterday".into()
    } else {
        display_date(iso)
    }
}

fn short_time(t: &str) -> String {
    t.get(..5).unwrap_or(t).to_owned()
}

impl RowView {
    pub fn of(r: &ListRow, today: &str) -> Self {
        let type_id = r.type_id;
        let tone = match ActivityType::from_id(type_id) {
            Some(ActivityType::Task) => "task",
            Some(ActivityType::Email) => "email",
            Some(ActivityType::Discussion) => "disc",
            Some(ActivityType::Event | ActivityType::Meeting | ActivityType::Call) => "meet",
            _ => "",
        };
        let title = if r.subject.is_empty() { r.type_name.clone() } else { r.subject.clone() };
        let mut meta: Vec<String> = Vec::new();
        if !r.subject.is_empty() {
            meta.push(r.type_name.clone());
        }
        if !r.parties.is_empty() {
            meta.push(r.parties.clone());
        }
        if !r.creator.is_empty() {
            meta.push(format!("by {}", r.creator));
        }
        if !r.time_display.is_empty() {
            meta.push(short_time(&r.time_display));
        }
        let chip = if r.is_task {
            if !r.completed_at.is_empty() {
                Some((
                    if r.completed_by.is_empty() {
                        "Completed".into()
                    } else {
                        format!("Completed by {}", r.completed_by)
                    },
                    "ok",
                ))
            } else if r.due_date.is_empty() {
                Some(("Open".into(), "info"))
            } else if r.due_date.as_str() < today {
                Some((format!("Overdue · {}", display_date(&r.due_date)), "bad"))
            } else if r.due_date == today {
                Some(("Due today".into(), "bad"))
            } else {
                Some((format!("Due {}", display_date(&r.due_date)), "info"))
            }
        } else if type_id == ActivityType::Discussion.id() {
            Some((format!("{} comment{}", r.comment_count, if r.comment_count == 1 { "" } else { "s" }), "brand"))
        } else {
            r.sent.map(|sent| if sent { ("Sent".into(), "ok") } else { ("Not sent".into(), "") })
        };
        let excerpt =
            if type_id == ActivityType::Note.id() { r.message.chars().take(220).collect() } else { String::new() };
        RowView {
            href: page_href(&r.id, type_id),
            icon: icon(type_id),
            tone,
            title,
            meta: meta.join(" · "),
            excerpt,
            chip,
            day: None,
        }
    }
}

/// The rows of a page, with a day header on the first row of each day.
pub fn rows(page: &ListPage, with_days: bool) -> Vec<RowView> {
    let today = crate::audit::local_now()[..10].to_owned();
    let yesterday = chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.pred_opt())
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let mut last: Option<String> = None;
    page.rows
        .iter()
        .map(|r| {
            let mut v = RowView::of(r, &today);
            if with_days && last.as_deref() != Some(r.date_display.as_str()) {
                v.day = Some(day_label(&r.date_display, &today, &yesterday));
                last = Some(r.date_display.clone());
            }
            v
        })
        .collect()
}

/// The timeline rows fragment (`#activity-rows`), with the pager.
#[derive(Template)]
#[template(path = "activities/rows.html")]
pub struct Rows {
    pub rows: Vec<RowView>,
    pub matched: i64,
    pub page: i64,
    pub page_size: i64,
    /// The list URL without `page`, for the pager.
    pub base: String,
    pub empty: &'static str,
}

impl Rows {
    fn first(&self) -> i64 {
        if self.matched == 0 {
            0
        } else {
            (self.page - 1) * self.page_size + 1
        }
    }
    fn last(&self) -> i64 {
        (self.page * self.page_size).min(self.matched)
    }
    fn more(&self) -> bool {
        self.page * self.page_size < self.matched
    }
    fn page_href(&self, page: i64) -> String {
        format!("{}&page={page}", self.base)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TimelineQuery {
    party_id: String,
    entity_type_id: String,
    entity_id: String,
    role_id: String,
    status: String,
    kind: String,
    page: i64,
}

impl TimelineQuery {
    fn list(&self) -> ListQuery {
        ListQuery {
            party_id: (!self.party_id.is_empty()).then(|| self.party_id.clone()),
            entity: self
                .entity_type_id
                .parse()
                .ok()
                .filter(|_| !self.entity_id.is_empty())
                .map(|t| (t, self.entity_id.clone())),
            role_id: self.role_id.parse().ok(),
            status: Status::parse(&self.status),
            kind: KindFilter::parse(&self.kind),
            page: self.page.max(1),
            ..Default::default()
        }
    }
    fn base(&self, kind: KindFilter, status: Status) -> String {
        let mut q = format!("/activities?kind={}", kind.key());
        if status != Status::Any {
            q.push_str(&format!("&status={}", status.key()));
        }
        if !self.party_id.is_empty() {
            q.push_str(&format!("&party_id={}", urlencode(&self.party_id)));
        }
        if !self.role_id.is_empty() {
            q.push_str(&format!("&role_id={}", urlencode(&self.role_id)));
        }
        if !self.entity_id.is_empty() {
            q.push_str(&format!(
                "&entity_type_id={}&entity_id={}",
                urlencode(&self.entity_type_id),
                urlencode(&self.entity_id)
            ));
        }
        q
    }
}

#[derive(Template)]
#[template(path = "activities/timeline.html")]
struct TimelinePage {
    nav: Nav,
    demo: bool,
    rows: String,
    /// (href, label, on) of each kind chip.
    kinds: Vec<(String, &'static str, bool)>,
    /// (href, label, on) of each status chip.
    statuses: Vec<(String, &'static str, bool)>,
    party_name: String,
    party_id: String,
}

/// `GET /activities?party_id=&role_id=&status=&kind=&page=` (OP-ACTIVITY-08): the timeline page, or its rows for
/// htmx.
pub async fn page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TimelineQuery>,
) -> Result<Response, AppError> {
    let conn = lock(&state)?;
    let list = q.list();
    let found = match activities::activity_list(&conn, &list) {
        Ok(p) => p,
        Err(e) => return command_error("activity_list", e),
    };
    let rows = Rows {
        rows: rows(&found, true),
        matched: found.matched,
        page: found.page,
        page_size: found.page_size,
        base: q.base(list.kind, list.status),
        empty: "No activities match.",
    }
    .render()?;
    if is_htmx(&headers) && headers.get("hx-target").is_some_and(|v| v == "activity-rows") {
        return Ok(Html(rows).into_response());
    }
    let kinds = KindFilter::ALL.iter().map(|k| (q.base(*k, list.status), k.label(), *k == list.kind)).collect();
    let statuses = [
        (Status::Any, "Any status"),
        (Status::Incomplete, "Incomplete"),
        (Status::Complete, "Complete"),
        (Status::Overdue, "Overdue"),
    ]
    .into_iter()
    .map(|(s, label)| (q.base(list.kind, s), label, s == list.status))
    .collect();
    let party_name = match &list.party_id {
        Some(p) => conn
            .query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [p], |r| r.get(0))
            .unwrap_or_default(),
        None => String::new(),
    };
    Ok(render(&TimelinePage {
        nav: Nav::Activity,
        demo: crate::db::has_demo_data(&conn)?,
        rows,
        kinds,
        statuses,
        party_name,
        party_id: q.party_id.clone(),
    })?
    .into_response())
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CompleteForm {
    complete: String,
}

/// `POST /tasks/{id}/complete` (OP-ACTIVITY-05, form `complete=true|false`): completes or reopens the task, then
/// reloads the page it was done on (the dashboard, the timeline, the party or the task) with a toast.
pub async fn complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<CompleteForm>,
) -> Result<Response, AppError> {
    let complete = form.complete != "false";
    let mut conn = lock(&state)?;
    match activities::task_set_complete(&mut conn, &id, complete, &actor) {
        Ok(()) => {
            let back = headers
                .get("hx-current-url")
                .and_then(|v| v.to_str().ok())
                .and_then(local_path)
                .unwrap_or_else(|| super::activities::activity_href(&id));
            let mut res = go(&headers, &back)?;
            res.headers_mut()
                .insert("HX-Trigger", toast(if complete { "Task completed" } else { "Task reopened" }, false));
            Ok(res)
        }
        Err(e) => command_error("task_set_complete", e),
    }
}

/// The path and query of the page's own URL (`http://127.0.0.1:port/x?y` → `/x?y`); anything else is ignored.
fn local_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let at = rest.find('/')?;
    let path = &rest[at..];
    (path.starts_with('/') && !path.starts_with("//")).then(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_path_keeps_only_the_path() {
        assert_eq!(super::local_path("http://127.0.0.1:5000/parties/p-hp?x=1").as_deref(), Some("/parties/p-hp?x=1"));
        assert_eq!(super::local_path("http://127.0.0.1:5000//evil.example/"), None);
        assert_eq!(super::local_path("javascript:alert(1)"), None);
    }
}
