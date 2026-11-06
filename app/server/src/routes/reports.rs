//! UI-20, the screen: monthly posted revenue as a chart and a table (OP-SALES-11, BR-SALES-17; D-38: a screen, no print).

use super::parties::page_error;
use super::Nav;
use crate::admin::{CommandError, FieldError};
use crate::money::usd;
use crate::reports::{self, MonthRow};
use crate::web::{is_htmx, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RangeQuery {
    from: String,
    to: String,
}

struct RowView {
    period: String,
    revenue: String,
    transactions: i64,
}

#[derive(Template)]
#[template(path = "reports/body.html")]
struct Body {
    from: String,
    to: String,
    errors: Vec<FieldError>,
    chart_json: String,
    rows: Vec<RowView>,
    total_revenue: String,
    total_transactions: i64,
}

impl Body {
    fn error(&self, field: &str) -> Option<&str> {
        self.errors.iter().find(|e| e.field == field).map(|e| e.message.as_str())
    }
}

#[derive(Template)]
#[template(path = "reports/sales_by_month.html")]
struct Page {
    nav: Nav,
    demo: bool,
    body: String,
}

fn body_of(from: String, to: String, months: &[MonthRow]) -> Body {
    let labels: Vec<String> = months.iter().map(|m| m.period.label()).collect();
    let revenue: Vec<f64> = months.iter().map(|m| m.revenue as f64 / 100.0).collect();
    Body {
        from,
        to,
        errors: Vec::new(),
        chart_json: serde_json::json!({ "labels": labels, "revenue": revenue }).to_string(),
        total_revenue: usd(months.iter().map(|m| m.revenue).sum()),
        total_transactions: months.iter().map(|m| m.transactions).sum(),
        rows: months
            .iter()
            .map(|m| RowView { period: m.period.label(), revenue: usd(m.revenue), transactions: m.transactions })
            .collect(),
    }
}

/// `GET /reports/sales-by-month?from=YYYYMM&to=YYYYMM` (Z2): the last 12 months by default; a bad month is 422 with the
/// form and its message.
pub async fn sales_by_month(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<RangeQuery>,
) -> Result<Response, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let (body, status) = match reports::parse_range(&q.from, &q.to, &state.today()) {
        Ok((from, to)) => {
            let months = reports::sales_summary_by_month(&conn, from, to)?;
            (body_of(from.key().to_string(), to.key().to_string(), &months), StatusCode::OK)
        }
        Err(CommandError::Validation(errors)) => {
            (Body { errors, ..body_of(q.from.clone(), q.to.clone(), &[]) }, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(e) => return Err(page_error("sales_summary_by_month", e)),
    };
    let html = body.render()?;
    if is_htmx(&headers) {
        return Ok((status, Html(html)).into_response());
    }
    let page = Page { nav: Nav::Reports, demo: crate::db::has_demo_data(&conn)?, body: html };
    Ok((status, Html(page.render()?)).into_response())
}
