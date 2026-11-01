//! UI-13 Sales and payments lists, read only: server-paged tables that htmx swaps. The writes behind them
//! (OP-SALES-04 invoice_save, OP-SALES-09 payment_save) are I5.

use super::parties::{party_href, SalesView};
use super::Nav;
use crate::invoice::display_date;
use crate::money::usd;
use crate::sales_list::{self, DocKind, PaymentRow, SalesQuery, Status};
use crate::web::{is_htmx, render, urlencode, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Html;
use serde::Deserialize;

/// `GET /sales` query: `q` (document number or customer), `status`, `kind` (invoice, receipt), `page`.
#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct SalesListQuery {
    pub q: String,
    pub status: String,
    pub kind: String,
    pub page: i64,
}

impl SalesListQuery {
    pub fn status(&self) -> &'static str {
        Status::parse(&self.status).key()
    }
    pub fn kind(&self) -> &'static str {
        DocKind::parse(&self.kind).key()
    }
    pub fn href(&self, page: i64, status: &str, kind: &str) -> String {
        format!("/sales?q={}&status={status}&kind={kind}&page={page}", urlencode(&self.q))
    }
    pub fn statuses(&self) -> Vec<(&'static str, &'static str)> {
        [Status::Any, Status::Open, Status::Overdue, Status::PartiallyPaid, Status::Paid, Status::Draft]
            .into_iter()
            .map(|s| (s.key(), s.label()))
            .collect()
    }
}

#[derive(Template)]
#[template(path = "sales/list_table.html")]
pub struct SalesTable<'a> {
    query: &'a SalesListQuery,
    rows: Vec<SalesView>,
    page: i64,
    pages: i64,
    matched: i64,
    first: i64,
    last: i64,
    total: String,
    balance: String,
}

#[derive(Template)]
#[template(path = "sales/list.html")]
struct SalesPage<'a> {
    nav: Nav,
    demo: bool,
    table: SalesTable<'a>,
}

/// `GET /sales` (UI-13): every sales transaction, newest first.
pub async fn sales(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SalesListQuery>,
) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let q = SalesQuery {
        query: query.q.clone(),
        status: Status::parse(&query.status),
        kind: DocKind::parse(&query.kind),
        page: query.page,
        ..Default::default()
    };
    let found = sales_list::sales_list(&conn, &q).map_err(|e| AppError::Internal(format!("sales_list: {e}")))?;
    let (first, last) = found.span();
    let table = SalesTable {
        query: &query,
        page: found.page,
        pages: found.pages,
        matched: found.matched,
        first,
        last,
        total: usd(found.total),
        balance: usd(found.balance),
        rows: found.rows.into_iter().map(SalesView::from).collect(),
    };
    if is_htmx(&headers) {
        return render(&table);
    }
    render(&SalesPage { nav: Nav::Invoices, demo: crate::db::has_demo_data(&conn)?, table })
}

/// One payment row as the list shows it.
pub struct PaymentView {
    pub date: String,
    pub reference: String,
    pub amount: String,
    pub customer: String,
    pub customer_href: String,
    /// (document number, link) of each document it pays.
    pub applied: Vec<(String, String)>,
}

impl From<PaymentRow> for PaymentView {
    fn from(p: PaymentRow) -> Self {
        PaymentView {
            date: display_date(&p.date),
            reference: p.reference,
            amount: usd(p.amount),
            customer_href: if p.party_id.is_empty() { String::new() } else { party_href(&p.party_id) },
            customer: p.customer,
            applied: p.applied_to.into_iter().map(|d| (format!("/sales/{}", urlencode(&d)), d)).collect(),
        }
    }
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct PaymentsQuery {
    pub q: String,
    pub page: i64,
}

impl PaymentsQuery {
    pub fn href(&self, page: i64) -> String {
        format!("/payments?q={}&page={page}", urlencode(&self.q))
    }
}

#[derive(Template)]
#[template(path = "sales/payments_table.html")]
pub struct PaymentsTable<'a> {
    query: &'a PaymentsQuery,
    rows: Vec<PaymentView>,
    page: i64,
    pages: i64,
    matched: i64,
    first: i64,
    last: i64,
    total: String,
}

#[derive(Template)]
#[template(path = "sales/payments.html")]
struct PaymentsPage<'a> {
    nav: Nav,
    demo: bool,
    table: PaymentsTable<'a>,
}

/// `GET /payments` (UI-13): every payment, newest first, with the documents it pays.
pub async fn payments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PaymentsQuery>,
) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let found = sales_list::payment_list(&conn, &query.q, query.page, 0)
        .map_err(|e| AppError::Internal(format!("payment_list: {e}")))?;
    let (first, last) = found.span();
    let table = PaymentsTable {
        query: &query,
        page: found.page,
        pages: found.pages,
        matched: found.matched,
        first,
        last,
        total: usd(found.total),
        rows: found.rows.into_iter().map(PaymentView::from).collect(),
    };
    if is_htmx(&headers) {
        return render(&table);
    }
    render(&PaymentsPage { nav: Nav::Payments, demo: crate::db::has_demo_data(&conn)?, table })
}
