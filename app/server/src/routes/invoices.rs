//! Invoice view (UI-12, read-only for the spike) and its print layout. "Print / Save as PDF" uses the webview's
//! print dialog (window.print); spike S-02 compares this with printpdf.

use super::Nav;
use crate::money::usd;
use crate::web::{render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, State};
use axum::response::Html;

struct Line {
    description: String,
    qty: String,
    rate: String,
    amount: String,
}

#[derive(Template)]
#[template(path = "invoices/page.html")]
struct InvoicePage {
    nav: Nav,
    doc_number: String,
    date: String,
    due_date: String,
    terms: String,
    status: String,
    customer: String,
    bill_to: Vec<String>,
    seller: String,
    lines: Vec<Line>,
    subtotal: String,
    tax_label: String,
    tax: String,
    total: String,
    paid: String,
    balance: String,
    memo: String,
}

pub async fn page(State(state): State<AppState>, Path(doc_number): Path<String>) -> Result<Html<String>, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    let (id, date, due_date, terms, status, customer, party_id, subtotal, tax_bp, tax, total, balance, memo): (
        String, String, String, String, String, String, String, i64, i64, i64, i64, i64, String,
    ) = conn.query_row(
        "SELECT st.id, st.date, COALESCE(st.due_date, ''), COALESCE(i.id_term, ''), COALESCE(st.status, ''),
                p.name_cached, p.id, COALESCE(st.subtotal, 0), COALESCE(st.tax_rate_bp, 0), COALESCE(st.total_tax, 0),
                COALESCE(st.total_amt, 0), COALESCE(st.balance, 0), COALESCE(st.customer_memo, '')
         FROM sales_transaction st JOIN party_role pr ON pr.id = st.id_customer JOIN party p ON p.id = pr.party_id
         LEFT JOIN invoice i ON i.id = st.id
         WHERE st.doc_number = ?1 AND COALESCE(st.deleted, 0) = 0",
        [&doc_number],
        |r| {
            Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?,
                r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?,
            ))
        },
    )?;

    // Billing address: the billing contact method of the customer, if any.
    let bill_to: Vec<String> = conn
        .query_row(
            "SELECT a.address_line_1, a.address_line_2, a.city, a.state_province, a.postal_code
             FROM contact_method cm JOIN address a ON a.id = cm.id
             WHERE cm.party_id = ?1 AND cm.address_ind = 1 ORDER BY cm.billing_ind_cached DESC LIMIT 1",
            [&party_id],
            |r| {
                let l1: Option<String> = r.get(0)?;
                let l2: Option<String> = r.get(1)?;
                let city: Option<String> = r.get(2)?;
                let st: Option<String> = r.get(3)?;
                let zip: Option<String> = r.get(4)?;
                let last = format!("{}, {} {}", city.unwrap_or_default(), st.unwrap_or_default(), zip.unwrap_or_default());
                Ok([l1, l2, Some(last)].into_iter().flatten().filter(|s| !s.trim().is_empty()).collect())
            },
        )
        .unwrap_or_default();
    let seller: String = conn
        .query_row("SELECT p.name_cached FROM system_setting s JOIN party p ON p.id = s.owner_id", [], |r| r.get(0))
        .unwrap_or_default();

    let mut stmt = conn.prepare(
        "SELECT COALESCE(description, ''), COALESCE(qty, 0), COALESCE(unit_price, 0), COALESCE(amount, 0)
         FROM sales_line WHERE sales_transaction_id = ?1 ORDER BY id",
    )?;
    let lines = stmt
        .query_map([&id], |r| {
            let qty: f64 = r.get(1)?;
            Ok(Line {
                description: r.get(0)?,
                qty: if qty.fract() == 0.0 { format!("{qty:.0}") } else { qty.to_string() },
                rate: usd(r.get(2)?),
                amount: usd(r.get(3)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    render(&InvoicePage {
        nav: Nav::Invoices,
        doc_number,
        date,
        due_date,
        terms,
        status,
        customer,
        bill_to,
        seller,
        lines,
        subtotal: usd(subtotal),
        tax_label: format!("Tax {}%", tax_bp as f64 / 100.0),
        tax: usd(tax),
        total: usd(total),
        paid: usd(total - balance),
        balance: usd(balance),
        memo,
    })
}
