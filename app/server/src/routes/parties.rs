//! People and organizations (UI-03 list, UI-04 detail, UI-05 create). The handlers call the commands of
//! [`crate::parties`] and render askama templates: a full page, or a fragment for htmx.

use super::{initials, Nav};
use crate::admin::CommandError;
use crate::contacts::{Method, MethodKind};
use crate::invoice::display_date;
use crate::money::usd;
use crate::parties::{self, Kind, PartyDetail, PartyRow, RoleFilter, Search, SearchPage};
use crate::sales_list::{self, SalesQuery, SalesRow};
use crate::web::{is_htmx, render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
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

// ---------------------------------------------------------------------------------------------------------------
// UI-04 party detail
// ---------------------------------------------------------------------------------------------------------------

/// A command error as a page error: NotFound is 404, the rest 500 with a log line (D-25).
pub fn page_error(op: &str, e: CommandError) -> AppError {
    match e {
        CommandError::NotFound => AppError::NotFound,
        e => AppError::Internal(format!("{op}: {e}")),
    }
}

/// One sales transaction row of the Sales tab and the sales list (UI-13).
pub struct SalesView {
    pub id: String,
    pub locked: bool,
    pub doc_number: String,
    pub href: String,
    pub date: String,
    pub due: String,
    pub kind: &'static str,
    pub status: &'static str,
    pub chip: &'static str,
    pub total: String,
    pub balance: String,
    pub has_balance: bool,
    pub customer: String,
    pub customer_href: String,
}

impl From<SalesRow> for SalesView {
    fn from(r: SalesRow) -> Self {
        SalesView {
            id: r.id.clone(),
            locked: r.locked,
            href: format!("/sales/{}", urlencode(&r.doc_number)),
            date: display_date(&r.date),
            due: display_date(&r.due_date),
            kind: if r.invoice { "Invoice" } else { "Sales receipt" },
            status: r.status.label(),
            chip: r.status.chip(),
            total: usd(r.total),
            balance: usd(r.balance),
            has_balance: r.balance != 0,
            customer_href: if r.party_id.is_empty() { String::new() } else { party_href(&r.party_id) },
            customer: r.customer,
            doc_number: r.doc_number,
        }
    }
}

/// "March 2021" from an ISO date or timestamp; empty when it does not parse.
pub fn month_year(iso: &str) -> String {
    let d = display_date(iso.get(..10).unwrap_or(""));
    match d.split_once(' ') {
        Some((month, rest)) if rest.contains(", ") => {
            let year = rest.rsplit(", ").next().unwrap_or("");
            const NAMES: [(&str, &str); 12] = [
                ("Jan", "January"),
                ("Feb", "February"),
                ("Mar", "March"),
                ("Apr", "April"),
                ("May", "May"),
                ("Jun", "June"),
                ("Jul", "July"),
                ("Aug", "August"),
                ("Sep", "September"),
                ("Oct", "October"),
                ("Nov", "November"),
                ("Dec", "December"),
            ];
            let full = NAMES.iter().find(|(s, _)| *s == month).map(|(_, f)| *f).unwrap_or(month);
            format!("{full} {year}")
        }
        _ => String::new(),
    }
}

/// The header of UI-04 (swapped by the edit form in place, OP-PARTY-03).
#[derive(Template)]
#[template(path = "parties/header.html")]
pub struct Header<'a> {
    pub d: &'a PartyDetail,
    /// Sent as an out-of-band swap (after the edit dialog saves).
    pub oob: bool,
    /// `/parties/{id}`.
    pub href: String,
    pub initials: String,
    pub color: usize,
    pub subtitle: String,
    /// Shows "Look up on FullContact…" (OP-INTEG-03): a person, not archived, with enrichment turned on (AP-12).
    pub enrich: bool,
}

impl<'a> Header<'a> {
    /// The header with its integration actions as the settings say (reads no key: the call checks it).
    pub fn for_party(conn: &rusqlite::Connection, d: &'a PartyDetail) -> Result<Self, AppError> {
        let on = crate::integrations::enrich_on(&crate::admin::settings_get(conn)?);
        Ok(Header { enrich: on && d.person.is_some() && !d.archived, ..Header::new(d) })
    }

    pub fn new(d: &'a PartyDetail) -> Self {
        let mut parts: Vec<String> = vec![if d.organization { "Organization".into() } else { "Person".into() }];
        if let Some(p) = &d.person {
            if !p.job_title.is_empty() {
                parts.push(p.job_title.clone());
            }
        }
        if let Some(o) = &d.org {
            if !o.company_type.is_empty() {
                parts.push(o.company_type.clone());
            }
        }
        if let Some(role) = d.roles.iter().find(|r| r.kind == parties::RoleKind::Customer && r.current) {
            let since = month_year(&role.from_date);
            if !since.is_empty() {
                parts.push(format!("Customer since {since}"));
            }
        }
        Header {
            d,
            oob: false,
            href: party_href(&d.id),
            initials: initials(&d.name),
            color: color(&d.id),
            subtitle: parts.join(" · "),
            enrich: false,
        }
    }
    pub fn icon(&self, m: &Method) -> &'static str {
        match m.kind {
            MethodKind::Phone => "phone",
            MethodKind::Email => "mail",
            MethodKind::Web => "globe",
            MethodKind::Address => "pin",
        }
    }
    /// (purpose type id, label, set) for the purpose toggles of a method (BR-CONTACT-02 ids).
    pub fn purpose_toggles(&self, m: &Method) -> Vec<(i64, &'static str, bool)> {
        use crate::reference::ContactMethodPurposeType::*;
        [(Billing, m.billing), (Shipping, m.shipping), (Mailing, m.mailing)]
            .into_iter()
            .map(|(p, on)| (p.id(), p.name(), on))
            .collect()
    }
    pub fn method_href(&self, m: &Method) -> String {
        super::contact_forms::method_href(&m.id)
    }
    /// The roles the party does not hold now, for the Add role menu.
    pub fn missing_roles(&self) -> Vec<parties::RoleKind> {
        use parties::RoleKind::*;
        [Customer, Lead, Employee, Vendor].into_iter().filter(|k| !self.d.has_role(*k)).collect()
    }
    pub fn balance(&self) -> String {
        self.d.customer.as_ref().map(|c| usd(c.balance)).unwrap_or_default()
    }
    pub fn retainer(&self) -> String {
        self.d.customer.as_ref().map(|c| usd(c.retainer)).unwrap_or_default()
    }
}

/// The Contacts tab: the related parties (BR-PARTY-03 pairs, seen from this party).
#[derive(Template)]
#[template(path = "parties/relations.html")]
pub struct Relations<'a> {
    pub d: &'a PartyDetail,
    /// Sent as an out-of-band swap (after a dialog).
    pub oob: bool,
}

impl Relations<'_> {
    pub fn party(&self) -> String {
        party_href(&self.d.id)
    }
    pub fn relation_href(&self, id: &str) -> String {
        format!("/relations/{}", urlencode(id))
    }
    pub fn href(&self, id: &str) -> String {
        party_href(id)
    }
    pub fn initials(&self, name: &str) -> String {
        initials(name)
    }
    pub fn color(&self, id: &str) -> usize {
        color(id)
    }
}

#[derive(Template)]
#[template(path = "parties/detail.html")]
struct DetailPage<'a> {
    nav: Nav,
    demo: bool,
    d: &'a PartyDetail,
    header: String,
    relations: String,
    sales: Vec<SalesView>,
    sales_matched: i64,
    archived_on: String,
    /// The Activity tab (OP-ACTIVITY-08): the party's activities, newest first.
    activity: String,
    activity_count: i64,
    /// The Files panel (UI-19, `routes::files::FilesPanel`).
    files: String,
    /// The Opportunities tab (O12).
    opportunities: Vec<super::opportunities::OpportunityView>,
}

/// `GET /parties/{id}` (UI-04). An archived party opens too, marked archived and without edit actions (D3).
pub async fn detail(State(state): State<AppState>, Path(id): Path<String>) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let d = parties::party_get(&conn, &id).map_err(|e| page_error("party_get", e))?;
    let today = state.today();
    let (sales, sales_matched) = match &d.customer {
        Some(c) => {
            let q = SalesQuery { customer_role_id: Some(c.role_id.clone()), page_size: 100, ..Default::default() };
            let page = sales_list::sales_list(&conn, &q, &today).map_err(|e| page_error("sales_list", e))?;
            (page.rows.into_iter().map(SalesView::from).collect(), page.matched)
        }
        None => (Vec::new(), 0),
    };
    let header = Header::for_party(&conn, &d)?.render()?;
    let relations = Relations { d: &d, oob: false }.render()?;
    let q =
        crate::activities::ListQuery { party_id: Some(d.id.clone()), today: Some(today.clone()), ..Default::default() };
    let found = crate::activities::activity_list(&conn, &q).map_err(|e| page_error("activity_list", e))?;
    let activity_count = found.matched;
    let activity = super::timeline::Rows {
        rows: super::timeline::rows(&found, true, &today),
        matched: found.matched,
        page: found.page,
        page_size: found.page_size,
        base: format!("/activities?kind=all&party_id={}", urlencode(&d.id)),
        empty: "No activities yet.",
    }
    .render()?;
    let files = super::files::FilesPanel::load(&state, &conn, &d.id)?.render()?;
    let opportunities =
        crate::opportunities::opportunities_of_party(&conn, &d.id)?.into_iter().map(Into::into).collect();
    render(&DetailPage {
        nav: Nav::Parties,
        demo: crate::db::has_demo_data(&conn)?,
        archived_on: display_date(d.deleted_timestamp.get(..10).unwrap_or("")),
        d: &d,
        header,
        relations,
        sales,
        sales_matched,
        activity,
        activity_count,
        files,
        opportunities,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn month_year() {
        assert_eq!(super::month_year("2021-03-04"), "March 2021");
        assert_eq!(super::month_year("2026-10-28 08:00:00"), "October 2026");
        assert_eq!(super::month_year(""), "");
    }
}
