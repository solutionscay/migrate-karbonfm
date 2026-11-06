//! The sales transaction page (UI-12: the editor or the read-only view) and the invoice PDF (UI-20), generated in Rust
//! (`GET .../invoice.pdf`, S-02). A sales receipt has no PDF (D-38).

use super::Nav;
use crate::invoice::{display_date, Invoice};
use crate::web::{render, AppError};
use crate::AppState;
use askama::Template;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};

#[derive(Template)]
#[template(path = "sales/read_only.html")]
struct ReadOnlyPage {
    nav: Nav,
    demo: bool,
    inv: Invoice,
    initial: String,
    date_text: String,
    due_text: String,
    status_class: &'static str,
    /// The desktop shell can save files (native save dialog); in a browser the page links the PDF for download.
    can_save: bool,
    /// The attachments panel (UI-19).
    files: String,
}

/// The editor of an unlocked transaction, in the app's layout (layouts 69, 94).
#[derive(Template)]
#[template(path = "sales/editor_page.html")]
struct EditorPage {
    nav: Nav,
    demo: bool,
    title: String,
    customer: String,
    party_id: String,
    editor: String,
    /// The attachments panel (UI-19).
    files: String,
}

fn load(state: &AppState, doc_number: &str) -> Result<Invoice, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Invoice::load_at(&conn, doc_number, &state.today())?)
}

/// `GET /sales/{doc_number}` (UI-12): an unlocked transaction opens in the editor, a locked one read only (kontacts 224
/// s31-41).
pub async fn page(State(state): State<AppState>, Path(doc_number): Path<String>) -> Result<Html<String>, AppError> {
    let inv = load(&state, &doc_number)?;
    if !inv.locked {
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        let editor = super::sales_forms::editor(&conn, &inv.id, None, Vec::new())?.render()?;
        let files = super::files::EntityFiles::load(&state, &conn, 12, &inv.id)?.render()?;
        return render(&EditorPage {
            nav: Nav::Invoices,
            demo: crate::db::has_demo_data(&conn)?,
            title: doc_number,
            customer: inv.customer,
            party_id: inv.party_id,
            editor,
            files,
        });
    }
    let status_class = inv.state.chip();
    let files = {
        let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        super::files::EntityFiles::load(&state, &conn, 12, &inv.id)?.render()?
    };
    render(&ReadOnlyPage {
        nav: Nav::Invoices,
        demo: super::demo(&state)?,
        initial: inv.seller.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default(),
        date_text: display_date(&inv.date),
        due_text: if inv.is_invoice { display_date(&inv.due_date) } else { String::new() },
        status_class,
        can_save: state.save.is_some(),
        files,
        inv,
    })
}

/// An invoice; a sales receipt has no PDF (F2, D-38).
fn load_invoice(state: &AppState, doc_number: &str) -> Result<Invoice, AppError> {
    let inv = load(state, doc_number)?;
    if inv.is_invoice {
        Ok(inv)
    } else {
        Err(AppError::NotFound)
    }
}

/// The invoice PDF drawn in Rust, shown inline; the file name is the document number.
pub async fn pdf(State(state): State<AppState>, Path(doc_number): Path<String>) -> Result<Response, AppError> {
    let inv = load_invoice(&state, &doc_number)?;
    let bytes = crate::pdf::invoice_pdf(&inv);
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
    let inv = load_invoice(&state, &doc_number)?;
    let name = file_name(&inv);
    let bytes = crate::pdf::invoice_pdf(&inv);
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
