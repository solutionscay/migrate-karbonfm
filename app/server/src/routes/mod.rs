//! Route handlers (05). Each handler queries or calls a command, then renders a template.

mod customers;
mod dashboard;
mod sales;

use crate::web::AppError;
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard::page))
        .route("/customers", get(customers::list))
        .route("/customers/new", get(customers::new_dialog).post(customers::create))
        .route("/sales/{doc_number}", get(sales::page))
        .route("/sales/{doc_number}/invoice.pdf", get(sales::pdf))
        .route("/print", post(print))
}

/// Opens the native print dialog for the current page (204). Without a desktop window (the dev server in a
/// browser) it answers 501 and the page falls back to window.print().
async fn print(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    match &state.print {
        Some(print) => print().map(|()| StatusCode::NO_CONTENT).map_err(AppError::Internal),
        None => Ok(StatusCode::NOT_IMPLEMENTED),
    }
}

/// Which sidebar link is current; the layout marks it with aria-current.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Dashboard,
    Customers,
    Invoices,
}

/// Two-letter initials for an avatar: "Halden & Pryce Architects" -> "HP".
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn initials() {
        assert_eq!(super::initials("Halden & Pryce Architects"), "HP");
        assert_eq!(super::initials("Tomás Ferreira"), "TF");
        assert_eq!(super::initials(""), "");
    }
}
