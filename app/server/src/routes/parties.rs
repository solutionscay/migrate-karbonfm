//! People and organizations (UI-03 list, UI-04 detail, UI-05 create). The handlers call the commands of
//! [`crate::parties`] and render askama templates: a full page, or a fragment for htmx.

use super::{initials, Nav};
use crate::parties::{self, Kind, PartyRow, RoleFilter, Search, SearchPage};
use crate::web::{is_htmx, render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Html;
use serde::Deserialize;

/// Avatar colour 1..=7 from the id, as the customers list does.
pub fn color(id: &str) -> usize {
    id.bytes().map(usize::from).sum::<usize>() % 7 + 1
}

/// `GET /parties` query: `q`, `kind` (all, person, org), `role` (any, customer, lead, employee, vendor), `page`,
/// `full` (full-word search) and `archived` (include archived parties).
#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct ListQuery {
    pub q: String,
    pub kind: String,
    pub role: String,
    pub page: i64,
    pub full: String,
    pub archived: String,
}

impl ListQuery {
    pub fn kind(&self) -> &'static str {
        Kind::parse(&self.kind).key()
    }
    pub fn role(&self) -> &'static str {
        RoleFilter::parse(&self.role).key()
    }
    pub fn full(&self) -> bool {
        !self.full.is_empty()
    }
    pub fn archived(&self) -> bool {
        !self.archived.is_empty()
    }
    fn search(&self) -> Search {
        Search {
            query: self.q.clone(),
            kind: Kind::parse(&self.kind),
            role: RoleFilter::parse(&self.role),
            full_word: self.full(),
            include_deleted: self.archived(),
            page: self.page,
            ..Default::default()
        }
    }
    /// The URL of another page or filter, keeping the rest.
    pub fn href(&self, page: i64, kind: &str, role: &str) -> String {
        let mut url = format!("/parties?q={}&kind={kind}&role={role}&page={page}", urlencode(&self.q));
        if self.full() {
            url.push_str("&full=1");
        }
        if self.archived() {
            url.push_str("&archived=1");
        }
        url
    }
}

/// One list row as the template shows it.
pub struct Row {
    pub party: PartyRow,
    pub initials: String,
    pub color: usize,
    /// `/parties/{id}`, the id percent-encoded (imported ids are any text).
    pub href: String,
}

pub fn party_href(id: &str) -> String {
    format!("/parties/{}", urlencode(id))
}

#[derive(Template)]
#[template(path = "parties/table.html")]
pub struct Table<'a> {
    query: &'a ListQuery,
    rows: Vec<Row>,
    page: i64,
    pages: i64,
    matched: i64,
    capped: bool,
    first: i64,
    last: i64,
}

impl<'a> Table<'a> {
    fn new(query: &'a ListQuery, found: SearchPage) -> Self {
        let first = if found.rows.is_empty() { 0 } else { (found.page - 1) * found.page_size + 1 };
        let last = first + found.rows.len() as i64 - i64::from(!found.rows.is_empty());
        let rows = found
            .rows
            .into_iter()
            .map(|party| Row {
                initials: initials(&party.name),
                color: color(&party.id),
                href: party_href(&party.id),
                party,
            })
            .collect();
        Table {
            query,
            rows,
            page: found.page,
            pages: found.pages,
            matched: found.matched,
            capped: found.capped,
            first,
            last,
        }
    }
}

#[derive(Template)]
#[template(path = "parties/list.html")]
struct ListPage<'a> {
    nav: Nav,
    demo: bool,
    table: Table<'a>,
}

/// `GET /parties` (UI-03, OP-PARTY-05).
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let found = parties::party_search(&conn, &query.search()).map_err(|e| AppError::Internal(e.to_string()))?;
    let table = Table::new(&query, found);
    if is_htmx(&headers) {
        return render(&table);
    }
    render(&ListPage { nav: Nav::Parties, demo: crate::db::has_demo_data(&conn)?, table })
}
