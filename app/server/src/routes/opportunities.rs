//! UI-10 Opportunities (layout 153, the chart), UI-11 the detail page and card (layouts 156, 162), and the row of a
//! party's Opportunities tab. OP-SALES-01, 02, 03.

use super::current_user::CurrentUser;
use super::parties::{page_error, party_href, Relations};
use super::party_forms::{go, toast};
use super::relation_forms::contact_picker;
use super::Nav;
use crate::admin::{CommandError, FieldError};
use crate::invoice::display_date;
use crate::money::usd;
use crate::opportunities::{
    self, ActiveFilter, CreatedFrom, ListRow, NewOpportunity, NextTask, Opportunity, OpportunityEdit, Referring,
    StatusTotal, ENTITY_TYPE,
};
use crate::web::{is_htmx, render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use karbon_money::Cents;
use serde::Deserialize;

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, AppError> {
    state.db.lock().map_err(|e| AppError::Internal(e.to_string()))
}

/// The date a create and a status change use (602 s57, 608 s45).
fn today(_state: &AppState) -> String {
    crate::audit::local_now()[..10].to_owned()
}

fn href(id: &str) -> String {
    format!("/opportunities/{}", urlencode(id))
}

fn value_text(value: Option<i64>) -> String {
    value.map_or_else(|| "—".to_owned(), usd)
}

/// The name a row links with. The source does not require one (602 s71), so a row may have none yet.
fn name_text(name: String) -> String {
    if name.is_empty() {
        "(no name)".to_owned()
    } else {
        name
    }
}

pub struct TaskView {
    pub href: String,
    pub subject: String,
    pub date: String,
}

impl From<NextTask> for TaskView {
    fn from(t: NextTask) -> Self {
        TaskView {
            href: super::activities::activity_href(&t.activity_id),
            subject: if t.subject.is_empty() { "Task".to_owned() } else { t.subject },
            date: display_date(&t.date),
        }
    }
}

/// One opportunity as a list or the party tab shows it.
pub struct OpportunityView {
    pub href: String,
    pub name: String,
    pub party: String,
    pub party_href: String,
    pub status: String,
    pub value: String,
    pub probability: String,
    pub active: bool,
    pub next_task: Option<TaskView>,
}

impl From<ListRow> for OpportunityView {
    fn from(o: ListRow) -> Self {
        OpportunityView {
            href: href(&o.id),
            name: name_text(o.name),
            party_href: if o.party_id.is_empty() { String::new() } else { party_href(&o.party_id) },
            party: o.party,
            status: o.status,
            value: value_text(o.value),
            probability: o.probability,
            active: o.active,
            next_task: o.next_task.map(Into::into),
        }
    }
}

/// A list row, or the subtotal that ends a status group (layout 153: "<<StatusCached>> subtotal", ValueTotal).
enum Line {
    Row(Box<OpportunityView>),
    Subtotal { status: String, value: String },
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct ListQuery {
    pub q: String,
    pub active: String,
    pub page: i64,
}

impl ListQuery {
    /// Show Active is the default; Show All is `all` (789, layout 153).
    fn href(&self, page: i64, active: &str) -> String {
        format!("/opportunities?q={}&active={active}&page={page}", urlencode(&self.q))
    }
}

#[derive(Template)]
#[template(path = "opportunities/table.html")]
struct OpportunitiesTable<'a> {
    query: &'a ListQuery,
    active: &'static str,
    lines: Vec<Line>,
    page: i64,
    pages: i64,
    matched: i64,
    first: i64,
    last: i64,
    total: String,
}

#[derive(Template)]
#[template(path = "opportunities/list.html")]
struct OpportunitiesPage<'a> {
    nav: Nav,
    demo: bool,
    table: OpportunitiesTable<'a>,
}

fn bad_filter() -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, "active must be 1, 0 or all").into_response()
}

/// Rows of the page with a subtotal after each group that ends on it. The groups cover every matched row, so a subtotal
/// counts the group's rows on other pages too.
fn lines(found: opportunities::ListPage) -> Vec<Line> {
    let offset = (found.page - 1) * found.page_size;
    let mut ends = Vec::with_capacity(found.groups.len());
    let mut seen = 0;
    for g in &found.groups {
        seen += g.count;
        ends.push((seen, g));
    }
    let mut out = Vec::with_capacity(found.rows.len() + ends.len());
    for (i, row) in found.rows.into_iter().enumerate() {
        out.push(Line::Row(Box::new(row.into())));
        let position = offset + i as i64 + 1;
        if let Some((_, g)) = ends.iter().find(|(end, _)| *end == position) {
            out.push(Line::Subtotal { status: g.status.clone(), value: usd(g.value) });
        }
    }
    out
}

/// `GET /opportunities` (`?q=&active=&page=`).
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Response, AppError> {
    let Some(active) = ActiveFilter::parse(&query.active) else {
        return Ok(bad_filter());
    };
    let conn = lock(&state)?;
    let found = opportunities::opportunity_list(&conn, &query.q, active, query.page, 0)?;
    let first = if found.rows.is_empty() { 0 } else { (found.page - 1) * found.page_size + 1 };
    let last = if found.rows.is_empty() { 0 } else { first + found.rows.len() as i64 - 1 };
    let (page, pages, matched, total) = (found.page, found.pages, found.matched, usd(found.total));
    let table = OpportunitiesTable {
        query: &query,
        active: active.key(),
        lines: lines(found),
        page,
        pages,
        matched,
        first,
        last,
        total,
    };
    if is_htmx(&headers) {
        return Ok(render(&table)?.into_response());
    }
    let demo = crate::db::has_demo_data(&conn)?;
    Ok(render(&OpportunitiesPage { nav: Nav::Opportunities, demo, table })?.into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TotalsQuery {
    active: String,
}

#[derive(Template)]
#[template(path = "opportunities/chart.html")]
struct ChartFragment {
    chart_json: String,
    rows: Vec<(String, String)>,
    total: String,
}

/// `GET /opportunities/status-totals?active=` (OP-SALES-03, BR-SALES-14): the chart and the same numbers as text.
pub async fn status_totals(State(state): State<AppState>, Query(q): Query<TotalsQuery>) -> Result<Response, AppError> {
    let Some(active) = ActiveFilter::parse(&q.active) else {
        return Ok(bad_filter());
    };
    let conn = lock(&state)?;
    let totals = opportunities::opportunity_status_totals(&conn, active)?;
    let labels: Vec<&str> = totals.iter().map(|t| t.status.as_str()).collect();
    let values: Vec<f64> = totals.iter().map(|t| t.value as f64 / 100.0).collect();
    let view = ChartFragment {
        chart_json: serde_json::json!({ "labels": labels, "values": values }).to_string(),
        total: usd(totals.iter().map(|t: &StatusTotal| t.value).sum()),
        rows: totals.iter().map(|t| (t.status.clone(), usd(t.value))).collect(),
    };
    Ok(render(&view)?.into_response())
}

// ---------------------------------------------------------------------------------------------------------------
// New (O3)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewQuery {
    party_id: String,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct NewForm {
    /// Given by a party page (kontacts 610).
    party_id: String,
    /// The contact picker's choice on the list's dialog (kontacts 615), and its search box.
    other_id: String,
    q: String,
    name: String,
}

#[derive(Template)]
#[template(path = "opportunities/new_dialog.html")]
struct NewDialog {
    party_id: String,
    party_name: String,
    picker: String,
    name: String,
    errors: Vec<FieldError>,
}

impl NewDialog {
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

/// `GET /opportunities/new?party_id=`.
pub async fn new_dialog(State(state): State<AppState>, Query(q): Query<NewQuery>) -> Result<Html<String>, AppError> {
    let conn = lock(&state)?;
    let (party_name, picker) = if q.party_id.is_empty() {
        (String::new(), contact_picker(&conn, "", "", "")?)
    } else {
        let d = crate::parties::party_get(&conn, &q.party_id).map_err(|e| page_error("party_get", e))?;
        (d.name, String::new())
    };
    render(&NewDialog { party_id: q.party_id, party_name, picker, name: String::new(), errors: Vec::new() })
}

/// `POST /opportunities` (OP-SALES-01): opens the new opportunity.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<NewForm>,
) -> Result<Response, AppError> {
    let from_party = !form.party_id.is_empty();
    let new = NewOpportunity {
        party_id: if from_party { form.party_id.clone() } else { form.other_id.clone() },
        name: form.name.clone(),
        from: if from_party { CreatedFrom::PartyPage } else { CreatedFrom::Picker },
    };
    let today = today(&state);
    let mut conn = lock(&state)?;
    match opportunities::opportunity_create(&mut conn, &new, &today, &actor) {
        Ok(id) => {
            let mut res = go(&headers, &href(&id))?;
            res.headers_mut().insert("HX-Trigger", toast("Opportunity created", true));
            Ok(res)
        }
        Err(CommandError::Validation(errors)) => {
            // A party page's party that is gone falls back to the picker, so the dialog can still finish.
            let picker = contact_picker(&conn, &form.q, "", &form.other_id)?;
            let view =
                NewDialog { party_id: String::new(), party_name: String::new(), picker, name: form.name, errors };
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(view.render()?)).into_response())
        }
        Err(e) => Err(page_error("opportunity_create", e)),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The summary, the detail page
// ---------------------------------------------------------------------------------------------------------------

/// Name, status menu and fields (layout 156). A save or a status change answers with it.
#[derive(Template)]
#[template(path = "opportunities/summary.html")]
struct Summary {
    o: Opportunity,
    statuses: Vec<(i64, String)>,
    balance: Option<String>,
    retainer: Option<String>,
    oob: bool,
}

impl Summary {
    fn load(conn: &rusqlite::Connection, id: &str, oob: bool) -> Result<Self, AppError> {
        let o = opportunities::opportunity_get(conn, id).map_err(|e| page_error("opportunity_get", e))?;
        let party = crate::parties::party_get(conn, &o.party_id).ok();
        let customer = party.as_ref().and_then(|p| p.customer.as_ref());
        Ok(Summary {
            balance: customer.map(|c| usd(c.balance)),
            retainer: customer.map(|c| usd(c.retainer)),
            statuses: opportunities::statuses(conn)?,
            oob,
            o,
        })
    }
    fn href(&self) -> String {
        href(&self.o.id)
    }
    fn title(&self) -> &str {
        if self.o.name.is_empty() {
            "(no name)"
        } else {
            &self.o.name
        }
    }
    fn value(&self) -> String {
        value_text(self.o.value)
    }
    fn party_href(&self) -> String {
        party_href(&self.o.party_id)
    }
    fn other_href(&self, id: &str) -> String {
        party_href(id)
    }
    fn date(&self, iso: &str) -> String {
        if iso.is_empty() {
            "—".to_owned()
        } else {
            display_date(iso)
        }
    }
    fn created(&self) -> String {
        display_date(self.o.created_at.get(..10).unwrap_or(""))
    }
}

#[derive(Template)]
#[template(path = "opportunities/detail.html")]
struct DetailPage {
    nav: Nav,
    demo: bool,
    id: String,
    party_id: String,
    summary: String,
    relations: String,
    activity: String,
    activity_count: i64,
    files: String,
    entity_type: i64,
}

/// `GET /opportunities/{id}` (UI-11). Activities, Discuss and Attach reuse I4 with entity 13.
pub async fn detail(State(state): State<AppState>, Path(id): Path<String>) -> Result<Html<String>, AppError> {
    let conn = lock(&state)?;
    let summary = Summary::load(&conn, &id, false)?;
    let party_id = summary.o.party_id.clone();
    // An imported row may have lost its party; the page still opens, with no contacts.
    let relations = match crate::parties::party_get(&conn, &party_id) {
        Ok(party) => Relations { d: &party, oob: false }.render()?,
        Err(_) => String::new(),
    };
    let q = crate::activities::ListQuery { entity: Some((ENTITY_TYPE, id.clone())), ..Default::default() };
    let found = crate::activities::activity_list(&conn, &q).map_err(|e| page_error("activity_list", e))?;
    let activity_count = found.matched;
    let activity = super::timeline::Rows {
        rows: super::timeline::rows(&found, true),
        matched: found.matched,
        page: found.page,
        page_size: found.page_size,
        base: format!("/activities?kind=all&entity_type_id={ENTITY_TYPE}&entity_id={}", urlencode(&id)),
        empty: "No activities yet.",
    }
    .render()?;
    let files = super::files::FilesPanel::load_for(&state, &conn, ENTITY_TYPE, &id)?.render()?;
    render(&DetailPage {
        nav: Nav::Opportunities,
        demo: crate::db::has_demo_data(&conn)?,
        id,
        party_id,
        summary: summary.render()?,
        relations,
        activity,
        activity_count,
        files,
        entity_type: ENTITY_TYPE,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Edit (O6, O7)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct EditForm {
    name: String,
    value: String,
    probability: String,
    start_date: String,
    end_date: String,
    salesperson_id: String,
    notes: String,
    primary_contact_party_id: String,
    /// The contact picker's choice (the referring party, kontacts 722) and its search box.
    other_id: String,
    q: String,
    clear_referring: String,
}

impl EditForm {
    fn of(o: &Opportunity) -> Self {
        EditForm {
            name: o.name.clone(),
            value: o.value.map(|v| Cents(v).to_input_value()).unwrap_or_default(),
            probability: o.probability.clone(),
            start_date: o.start_date.clone(),
            end_date: o.end_date.clone(),
            salesperson_id: o.salesperson_id.clone(),
            notes: o.notes.clone(),
            primary_contact_party_id: o.primary_contact_party_id.clone(),
            ..Default::default()
        }
    }
    fn edit(&self) -> OpportunityEdit {
        OpportunityEdit {
            name: self.name.clone(),
            value: self.value.clone(),
            probability: self.probability.clone(),
            start_date: self.start_date.clone(),
            end_date: self.end_date.clone(),
            salesperson_id: self.salesperson_id.clone(),
            notes: self.notes.clone(),
            primary_contact_party_id: self.primary_contact_party_id.clone(),
            referring: if !self.clear_referring.is_empty() {
                Referring::Clear
            } else if self.other_id.is_empty() {
                Referring::Keep
            } else {
                Referring::Set(self.other_id.clone())
            },
        }
    }
}

#[derive(Template)]
#[template(path = "opportunities/card.html")]
struct Card {
    action: String,
    form: EditForm,
    referring: String,
    /// (value, label) of each list; a stored value outside the list stays on offer (O7).
    probabilities: Vec<String>,
    salespeople: Vec<(String, String)>,
    contacts: Vec<(String, String)>,
    picker: String,
    errors: Vec<FieldError>,
}

impl Card {
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }

    fn load(
        conn: &rusqlite::Connection,
        o: &Opportunity,
        form: EditForm,
        errors: Vec<FieldError>,
    ) -> Result<Self, AppError> {
        let mut probabilities: Vec<String> =
            std::iter::once(opportunities::UNKNOWN).chain(opportunities::PROBABILITIES).map(str::to_owned).collect();
        if !o.probability.is_empty() && !probabilities.contains(&o.probability) {
            probabilities.push(o.probability.clone());
        }
        let mut salespeople = opportunities::current_employees(conn)?;
        if !o.salesperson_id.is_empty() && !salespeople.iter().any(|(id, _)| *id == o.salesperson_id) {
            salespeople.push((o.salesperson_id.clone(), o.salesperson.clone()));
        }
        let mut contacts: Vec<(String, String)> = Vec::new();
        for r in crate::parties::relations_of(conn, &o.party_id)? {
            if !contacts.iter().any(|(id, _)| *id == r.other_id) {
                contacts.push((r.other_id, r.other_name));
            }
        }
        if !o.primary_contact_party_id.is_empty() && !contacts.iter().any(|(id, _)| *id == o.primary_contact_party_id) {
            contacts.push((o.primary_contact_party_id.clone(), o.primary_contact.clone()));
        }
        Ok(Card {
            action: href(&o.id),
            picker: contact_picker(conn, &form.q, &o.party_id, &form.other_id)?,
            referring: o.referring.clone(),
            probabilities,
            salespeople,
            contacts,
            form,
            errors,
        })
    }
}

/// `GET /opportunities/{id}/edit`: the card.
pub async fn edit_dialog(State(state): State<AppState>, Path(id): Path<String>) -> Result<Html<String>, AppError> {
    let conn = lock(&state)?;
    let o = opportunities::opportunity_get(&conn, &id).map_err(|e| page_error("opportunity_get", e))?;
    render(&Card::load(&conn, &o, EditForm::of(&o), Vec::new())?)
}

/// `PUT /opportunities/{id}` (OP-SALES-01): the summary, out of band; the dialog closes.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<EditForm>,
) -> Result<Response, AppError> {
    let mut conn = lock(&state)?;
    match opportunities::opportunity_update(&mut conn, &id, &form.edit(), &actor) {
        Ok(()) => {
            let html = Summary::load(&conn, &id, true)?.render()?;
            Ok(([("HX-Trigger", toast("Opportunity saved", true))], Html(html)).into_response())
        }
        Err(CommandError::Validation(errors)) => {
            let o = opportunities::opportunity_get(&conn, &id).map_err(|e| page_error("opportunity_get", e))?;
            let html = Card::load(&conn, &o, form, errors)?.render()?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
        }
        Err(e) => Err(page_error("opportunity_update", e)),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Status (OP-SALES-02)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct StatusForm {
    status_id: String,
    end_date: String,
}

/// `POST /opportunities/{id}/status` (BR-SALES-12): the summary, which the menu swaps.
pub async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(actor): CurrentUser,
    Form(form): Form<StatusForm>,
) -> Result<Response, AppError> {
    let today = today(&state);
    let mut conn = lock(&state)?;
    match opportunities::opportunity_set_status(&mut conn, &id, &form.status_id, &form.end_date, &today, &actor) {
        Ok(()) => {
            let html = Summary::load(&conn, &id, false)?.render()?;
            Ok(([("HX-Trigger", toast("Status changed", false))], Html(html)).into_response())
        }
        Err(CommandError::Validation(errors)) => {
            let message = errors.into_iter().map(|e| e.message).collect::<Vec<_>>().join(" ");
            Ok((StatusCode::UNPROCESSABLE_ENTITY, message).into_response())
        }
        Err(e) => Err(page_error("opportunity_set_status", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str) -> ListRow {
        ListRow {
            id: "o".into(),
            name: "n".into(),
            party_id: String::new(),
            party: String::new(),
            status: status.into(),
            value: None,
            probability: String::new(),
            active: true,
            next_task: None,
        }
    }

    /// Page 2 of 2 rows per page over A x3, B x1, C x1: A ends here with all of A's value, B ends here, C does not.
    #[test]
    fn a_subtotal_ends_its_group_and_counts_the_other_pages() {
        let group = |status: &str, value, count| StatusTotal { status: status.into(), value, count };
        let found = opportunities::ListPage {
            rows: vec![row("A"), row("B")],
            groups: vec![group("A", 300, 3), group("B", 100, 1), group("C", 500, 1)],
            matched: 5,
            page: 2,
            pages: 3,
            page_size: 2,
            total: 900,
        };
        let subtotals: Vec<(String, String)> = lines(found)
            .into_iter()
            .filter_map(|l| match l {
                Line::Subtotal { status, value } => Some((status, value)),
                Line::Row(_) => None,
            })
            .collect();
        assert_eq!(subtotals, vec![("A".to_owned(), "$3.00".to_owned()), ("B".to_owned(), "$1.00".to_owned())]);
    }
}
