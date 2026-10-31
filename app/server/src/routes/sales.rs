//! Invoice view (UI-12, read only) and the invoice PDF (UI-20). Spike S-02 compares the two ways to get a PDF:
//! the page printed by the webview ("Print (webview)") and the file generated in Rust (`GET .../invoice.pdf`).

use super::Nav;
use crate::invoice::{display_date, Invoice};
use crate::pdf::Engine;
use crate::web::{render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
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
    /// The desktop shell can save files (native save dialog); in a browser the page links the PDF for download.
    can_save: bool,
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
        can_save: state.save.is_some(),
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
    let disposition = format!("inline; filename=\"{}\"", file_name(&inv));
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
            // Pages of this server may frame the PDF (the preview dialog); other sites may not.
            (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'self'".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// Saves the invoice PDF through the desktop shell: the native save dialog, then the system PDF viewer (S-02 plan for
/// I5). The page posts here, so it needs no Tauri IPC (AP-01). The answer is 204 when the user cancels, or an empty
/// body with a toast when the file is saved. Without a shell (the dev server in a browser) it answers 501; that page
/// links the PDF with `download` instead.
pub async fn save_pdf(State(state): State<AppState>, Path(doc_number): Path<String>) -> Result<Response, AppError> {
    let Some(save) = state.save.clone() else {
        return Ok(StatusCode::NOT_IMPLEMENTED.into_response());
    };
    let inv = load(&state, &doc_number)?;
    let name = file_name(&inv);
    let bytes = crate::pdf::invoice_pdf(&inv, Engine::Krilla);
    // The dialog blocks until the user answers; keep it off the async workers.
    let saved = tokio::task::spawn_blocking(move || save(name, bytes))
        .await
        .map_err(|e| AppError::Internal(format!("save dialog: {e}")))?
        .map_err(AppError::Internal)?;
    let Some(path) = saved else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let shown = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let trigger = serde_json::json!({ "karbon-toast": format!("Saved {shown}") });
    Ok(([("HX-Trigger", trigger.to_string())], Html(String::new())).into_response())
}

/// The PDF's file name: the document number, without characters that would break a header or a path.
fn file_name(inv: &Invoice) -> String {
    let stem: String = inv.doc_number.chars().filter(|c| !matches!(c, '"' | '\\' | '/' | ':')).collect();
    format!("{stem}.pdf")
}
