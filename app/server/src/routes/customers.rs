//! Customers list (UI-03): a server-paged, server-sorted table. htmx swaps the table body and pager.

use super::{initials, Nav};
use crate::money::usd;
use crate::web::{is_htmx, render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

pub const PAGE_SIZE: i64 = 8;

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct ListQuery {
    pub q: String,
    pub status: String,
    pub sort: String,
    pub page: i64,
}

impl ListQuery {
    fn status(&self) -> &str {
        match self.status.as_str() {
            s @ ("overdue" | "retainer" | "inactive") => s,
            _ => "all",
        }
    }
    fn sort(&self) -> &str {
        match self.sort.as_str() {
            "name" => "name",
            _ => "balance",
        }
    }
    /// The query string for another page or filter, keeping the rest.
    pub fn href(&self, page: i64, status: &str) -> String {
        format!("/customers?q={}&status={status}&sort={}&page={page}", urlencode(&self.q), self.sort())
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub struct Row {
    pub name: String,
    pub initials: String,
    pub org: bool,
    pub contact: String,
    pub email: String,
    pub phone: String,
    pub current: bool,
    pub overdue: bool,
    pub balance: String,
    pub has_balance: bool,
    pub retainer: String,
    pub has_retainer: bool,
    pub color: usize,
}

pub struct Totals {
    pub customers: i64,
    pub current: i64,
    pub balance_due: String,
    pub with_balance: i64,
    pub overdue: String,
    pub overdue_customers: i64,
    pub oldest_overdue_days: i64,
    pub retainer: String,
    pub with_retainer: i64,
}

#[derive(Template)]
#[template(path = "customers/list.html")]
struct ListPage<'a> {
    nav: Nav,
    totals: Totals,
    table: Table<'a>,
}

#[derive(Template)]
#[template(path = "customers/table.html")]
pub struct Table<'a> {
    query: &'a ListQuery,
    rows: Vec<Row>,
    page: i64,
    pages: i64,
    matched: i64,
    first: i64,
    last: i64,
}

/// One row per customer role with its open balance, overdue amount and retainer balance (posted transactions only,
/// BR-SALES-06). Overdue is an open invoice past its due date, by the local date.
const CUSTOMERS_SQL: &str = "
WITH c AS (
  SELECT p.id AS party_id, p.name_cached AS name, p.organization_ind AS org,
    COALESCE((SELECT k.name_cached FROM party_relation r JOIN party k ON k.id = r.party_id_from
              WHERE r.party_id_to = p.id AND r.current_ind = 1 ORDER BY r.id LIMIT 1), '') AS contact,
    COALESCE(p.email_address_cached, '') AS email, COALESCE(p.phone_number_cached, '') AS phone,
    pr.current_ind AS current,
    COALESCE((SELECT SUM(st.balance) FROM sales_transaction st WHERE st.id_customer = pr.id AND st.locked_ind = 1
              AND COALESCE(st.deleted, 0) = 0 AND st.invoice_ind = 1), 0) AS balance,
    COALESCE((SELECT SUM(st.balance) FROM sales_transaction st WHERE st.id_customer = pr.id AND st.locked_ind = 1
              AND COALESCE(st.deleted, 0) = 0 AND st.invoice_ind = 1 AND st.balance > 0
              AND st.due_date < date('now', 'localtime')), 0) AS overdue,
    COALESCE((SELECT CAST(julianday(date('now', 'localtime')) - julianday(MIN(st.due_date)) AS INTEGER)
              FROM sales_transaction st WHERE st.id_customer = pr.id AND st.locked_ind = 1
              AND COALESCE(st.deleted, 0) = 0 AND st.invoice_ind = 1 AND st.balance > 0
              AND st.due_date < date('now', 'localtime')), 0) AS overdue_days,
    COALESCE(cu.retainer_balance_cached, 0) AS retainer
  FROM party_role pr JOIN party p ON p.id = pr.party_id LEFT JOIN customer cu ON cu.id = pr.id
  WHERE pr.customer_ind = 1 AND COALESCE(p.deleted_ind, 0) = 0
)
SELECT * FROM c";

fn filter_sql(query: &ListQuery) -> String {
    let status = match query.status() {
        "overdue" => "AND overdue > 0",
        "retainer" => "AND retainer > 0",
        "inactive" => "AND current = 0",
        _ => "",
    };
    format!(
        "WHERE (?1 = '' OR lower(name) LIKE '%' || lower(?1) || '%' OR lower(contact) LIKE '%' || lower(?1) || '%' \
         OR lower(email) LIKE '%' || lower(?1) || '%') {status}"
    )
}

pub fn load_table<'a>(conn: &rusqlite::Connection, query: &'a ListQuery) -> Result<Table<'a>, AppError> {
    let filter = filter_sql(query);
    let order = match query.sort() {
        "name" => "ORDER BY name COLLATE NOCASE",
        _ => "ORDER BY balance DESC, retainer DESC, name COLLATE NOCASE",
    };
    let q = query.q.trim();
    let matched: i64 = conn.query_row(
        &format!("{CUSTOMERS_SQL} {filter}").replacen("SELECT * FROM c", "SELECT COUNT(*) FROM c", 1),
        [q],
        |r| r.get(0),
    )?;
    let pages = ((matched + PAGE_SIZE - 1) / PAGE_SIZE).max(1);
    let page = query.page.clamp(1, pages);
    let mut stmt = conn.prepare(&format!("{CUSTOMERS_SQL} {filter} {order} LIMIT ?2 OFFSET ?3"))?;
    let rows = stmt
        .query_map(rusqlite::params![q, PAGE_SIZE, (page - 1) * PAGE_SIZE], |r| {
            let name: String = r.get("name")?;
            let balance: i64 = r.get("balance")?;
            let retainer: i64 = r.get("retainer")?;
            let party_id: String = r.get("party_id")?;
            Ok(Row {
                initials: initials(&name),
                color: party_id.bytes().map(usize::from).sum::<usize>() % 7 + 1,
                name,
                org: r.get::<_, i64>("org")? == 1,
                contact: r.get("contact")?,
                email: r.get("email")?,
                phone: r.get("phone")?,
                current: r.get::<_, i64>("current")? == 1,
                overdue: r.get::<_, i64>("overdue")? > 0,
                balance: usd(balance),
                has_balance: balance != 0,
                retainer: usd(retainer),
                has_retainer: retainer != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let first = if matched == 0 { 0 } else { (page - 1) * PAGE_SIZE + 1 };
    Ok(Table { query, first, last: first + rows.len() as i64 - i64::from(matched > 0), rows, page, pages, matched })
}

fn load_totals(conn: &rusqlite::Connection) -> Result<Totals, AppError> {
    let sql = format!(
        "{} ",
        CUSTOMERS_SQL.replacen(
            "SELECT * FROM c",
            "SELECT COUNT(*), SUM(current), SUM(balance), SUM(balance <> 0), SUM(overdue), SUM(overdue > 0), \
             MAX(overdue_days), SUM(retainer), SUM(retainer > 0) FROM c",
            1
        )
    );
    Ok(conn.query_row(&sql, [], |r| {
        Ok(Totals {
            customers: r.get(0)?,
            current: r.get(1)?,
            balance_due: usd(r.get(2)?),
            with_balance: r.get(3)?,
            overdue: usd(r.get(4)?),
            overdue_customers: r.get(5)?,
            oldest_overdue_days: r.get(6)?,
            retainer: usd(r.get(7)?),
            with_retainer: r.get(8)?,
        })
    })?)
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let table = load_table(&conn, &query)?;
    if is_htmx(&headers) {
        return render(&table);
    }
    let totals = load_totals(&conn)?;
    render(&ListPage { nav: Nav::Customers, totals, table })
}

#[derive(Template)]
#[template(path = "customers/new_dialog.html")]
struct NewDialog {
    name: String,
    email: String,
    error: Option<&'static str>,
}

/// The "New customer" dialog body, loaded by htmx into the page's <dialog>.
pub async fn new_dialog() -> Result<Html<String>, AppError> {
    render(&NewDialog { name: String::new(), email: String::new(), error: None })
}

#[derive(Deserialize)]
pub struct NewCustomer {
    name: String,
    email: String,
}

/// Validates the form on the server and answers with the dialog again (with the error) or a toast.
/// Creating the party is I2 (OP-PARTY-01); the spike only shows the form round trip.
pub async fn create(Form(form): Form<NewCustomer>) -> Result<Response, AppError> {
    let name = form.name.trim().to_string();
    let email = form.email.trim().to_string();
    let error = if name.is_empty() {
        Some("Enter a name.")
    } else if !email.is_empty() && !email.contains('@') {
        Some("Enter a valid email address, or leave it empty.")
    } else {
        None
    };
    if error.is_some() {
        return Ok(render(&NewDialog { name, email, error })?.into_response());
    }
    let trigger = serde_json::json!({ "karbon-close-dialog": null, "karbon-toast": format!("{name}: saving customers comes in I2") });
    Ok(([("HX-Trigger", trigger.to_string())], Html(String::new())).into_response())
}
