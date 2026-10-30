//! Dashboard (UI-02): key numbers and the monthly revenue chart (Chart.js, data from sales_summary_by_month).

use super::Nav;
use crate::money::usd;
use crate::web::{render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::State;
use axum::response::Html;

#[derive(Template)]
#[template(path = "dashboard.html")]
struct Dashboard {
    nav: Nav,
    owner_first_name: String,
    balance_due: String,
    open_invoices: i64,
    revenue_this_month: String,
    pipeline: String,
    open_opportunities: i64,
    /// JSON for the chart: {"labels": [...], "revenue": [...dollars]}; read by app.js from a data attribute.
    chart_json: String,
}

pub async fn page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let owner_first_name: String = conn
        .query_row(
            "SELECT pe.first_name FROM user u JOIN person pe ON pe.id = u.party_id ORDER BY u.id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "there".into());
    let (balance, open_invoices): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(balance), 0), COUNT(*) FROM sales_transaction
         WHERE invoice_ind = 1 AND locked_ind = 1 AND COALESCE(deleted, 0) = 0 AND balance > 0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (pipeline, open_opportunities): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(o.value), 0), COUNT(*) FROM opportunity o JOIN opportunity_status s ON s.id = o.opportunity_status_id
         WHERE s.active_ind = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    // The last 12 months, oldest first, with 0 for a month without posted sales.
    let mut stmt = conn.prepare(
        "WITH RECURSIVE m(n, d) AS (SELECT 0, date('now', 'localtime', 'start of month')
                                     UNION ALL SELECT n + 1, date(d, '-1 month') FROM m WHERE n < 11)
         SELECT strftime('%m', d), COALESCE(s.revenue, 0)
         FROM m LEFT JOIN sales_summary_by_month s ON s.period = CAST(strftime('%Y%m', d) AS INTEGER)
         ORDER BY d",
    )?;
    let months: Vec<(String, i64)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    const NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let labels: Vec<&str> = months.iter().map(|(m, _)| NAMES[m.parse::<usize>().unwrap_or(1).clamp(1, 12) - 1]).collect();
    let revenue: Vec<f64> = months.iter().map(|(_, cents)| *cents as f64 / 100.0).collect();
    let this_month = months.last().map(|(_, c)| *c).unwrap_or(0);

    render(&Dashboard {
        nav: Nav::Dashboard,
        owner_first_name,
        balance_due: usd(balance),
        open_invoices,
        revenue_this_month: usd(this_month),
        pipeline: usd(pipeline),
        open_opportunities,
        chart_json: serde_json::json!({ "labels": labels, "revenue": revenue }).to_string(),
    })
}
