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
    demo: bool,
    owner_first_name: String,
    balance_due: String,
    open_invoices: i64,
    revenue_this_month: String,
    /// The Overdue tile (B5): the same figures as /customers.
    overdue: String,
    overdue_customers: i64,
    pipeline: String,
    open_opportunities: i64,
    /// JSON for the chart: {"labels": [...], "revenue": [...dollars]}; read by app.js from a data attribute.
    chart_json: String,
    /// Home "Tasks due" (UI-02, layout 101): (task id, subject, who it is about, due text, overdue).
    tasks: Vec<TaskRow>,
    tasks_open: i64,
    /// "Recent activity": the latest activities (OP-ACTIVITY-08).
    recent: String,
}

struct TaskRow {
    id: String,
    href: String,
    subject: String,
    who: String,
    due: String,
    late: bool,
}

pub async fn page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let owner_first_name: String = conn
        .query_row(
            "SELECT pe.first_name FROM user u JOIN person pe ON pe.id = u.party_id WHERE u.id = ?1",
            [crate::admin::current_user(&conn)?.and_then(|a| a.user_id)],
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

    let today = state.today();
    let totals = super::customers::load_totals(&conn, &today)?;
    // The last 12 months, oldest first, with 0 for a month without posted sales.
    let mut stmt = conn.prepare(
        "WITH RECURSIVE m(n, d) AS (SELECT 0, date(?1, 'start of month')
                                     UNION ALL SELECT n + 1, date(d, '-1 month') FROM m WHERE n < 11)
         SELECT strftime('%m', d), COALESCE(s.revenue, 0)
         FROM m LEFT JOIN sales_summary_by_month s ON s.period = CAST(strftime('%Y%m', d) AS INTEGER)
         ORDER BY d",
    )?;
    let months: Vec<(String, i64)> =
        stmt.query_map([&today], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    const NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let labels: Vec<&str> =
        months.iter().map(|(m, _)| NAMES[m.parse::<usize>().unwrap_or(1).clamp(1, 12) - 1]).collect();
    let revenue: Vec<f64> = months.iter().map(|(_, cents)| *cents as f64 / 100.0).collect();
    let this_month = months.last().map(|(_, c)| *c).unwrap_or(0);

    let actor = crate::admin::current_user(&conn)?;
    let (tasks, tasks_open) = match &actor {
        Some(a) => {
            let due = crate::activities::tasks_due(&conn, a, 8).map_err(|e| AppError::Internal(e.to_string()))?;
            let rows = due
                .rows
                .iter()
                .map(|r| TaskRow {
                    id: r.id.clone(),
                    href: super::activities::activity_href(&r.id),
                    subject: if r.subject.is_empty() { "Task".into() } else { r.subject.clone() },
                    who: r.parties.clone(),
                    due: match r.due_date.as_str() {
                        "" => String::new(),
                        d if d == today => "Today".into(),
                        d => crate::invoice::display_date(d),
                    },
                    late: !r.due_date.is_empty() && r.due_date <= today,
                })
                .collect();
            (rows, due.matched)
        }
        None => (Vec::new(), 0),
    };
    let recent_page = crate::activities::activity_list(
        &conn,
        &crate::activities::ListQuery { page_size: 6, today: Some(today.clone()), ..Default::default() },
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;
    let recent = super::timeline::Rows {
        rows: super::timeline::rows(&recent_page, false, &today),
        matched: recent_page.rows.len() as i64,
        page: 1,
        page_size: 6,
        base: "/activities?kind=all".into(),
        empty: "No activities yet.",
    }
    .render()?;

    render(&Dashboard {
        tasks,
        tasks_open,
        recent,
        nav: Nav::Dashboard,
        demo: crate::db::has_demo_data(&conn)?,
        owner_first_name,
        balance_due: usd(balance),
        open_invoices,
        revenue_this_month: usd(this_month),
        overdue: totals.overdue,
        overdue_customers: totals.overdue_customers,
        pipeline: usd(pipeline),
        open_opportunities,
        chart_json: serde_json::json!({ "labels": labels, "revenue": revenue }).to_string(),
    })
}
