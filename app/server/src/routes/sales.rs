//! Invoice view (UI-12, read only) and the invoice PDF (UI-20). Spike S-02 compares the two ways to get a PDF:
//! the page printed by the webview ("Print (webview)") and the file generated in Rust (`GET .../invoice.pdf`).

use super::Nav;
use crate::invoice::{display_date, Invoice};
use crate::pdf::Engine;
use crate::web::{render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};

#[derive(Template)]
#[template(path = "sales/invoice.html")]
struct InvoicePage {
    nav: Nav,
    inv: Invoice,
    initial: String,
    date_text: String,
    due_text: String,
    status_class: &'static str,
}

fn load(state: &AppState, doc_number: &str) -> Result<Invoice, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Invoice::load(&conn, doc_number)?)
}

pub async fn page(State(state): State<AppState>, Path(doc_number): Path<String>) -> Result<Html<String>, AppError> {
    let inv = load(&state, &doc_number)?;
    let status_class = match inv.status.as_str() {
        "Paid" => "ok",
        "Partially paid" => "warn",
        "Overdue" => "bad",
        _ => "info",
    };
    render(&InvoicePage {
        nav: Nav::Invoices,
        initial: inv.seller.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default(),
        date_text: display_date(&inv.date),
        due_text: display_date(&inv.due_date),
        status_class,
        inv,
    })
}

#[derive(serde::Deserialize)]
pub struct PdfQuery {
    engine: Option<String>,
}

/// The invoice PDF drawn in Rust, shown inline; the file name is the document number. `?engine=printpdf` writes
/// it with printpdf instead of krilla (S-02 compares the two).
pub async fn pdf(
    State(state): State<AppState>,
    Path(doc_number): Path<String>,
    Query(q): Query<PdfQuery>,
) -> Result<Response, AppError> {
    let inv = load(&state, &doc_number)?;
    let engine = match q.engine.as_deref() {
        Some("printpdf") => Engine::Printpdf,
        _ => Engine::Krilla,
    };
    let bytes = crate::pdf::invoice_pdf(&inv, engine);
    let disposition = format!("inline; filename=\"{}.pdf\"", inv.doc_number.replace(['"', '\\'], ""));
    Ok(([(header::CONTENT_TYPE, "application/pdf".to_string()), (header::CONTENT_DISPOSITION, disposition)], bytes)
        .into_response())
}
