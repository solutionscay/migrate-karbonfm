//! Route handlers (05). Each handler queries or calls a command, then renders a template.

mod customers;
mod dashboard;
mod invoices;

use crate::AppState;
use axum::routing::get;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard::page))
        .route("/customers", get(customers::list))
        .route("/customers/new", get(customers::new_dialog).post(customers::create))
        .route("/invoices/{doc_number}", get(invoices::page))
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
