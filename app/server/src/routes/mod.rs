//! Route handlers (05). Each handler queries or calls a command, then renders a template.

mod backup;
pub mod current_user;
mod customers;
mod dashboard;
mod parties;
mod sales;
mod settings;
mod setup;

pub use setup::setup_gate;

use crate::web::AppError;
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;

/// Every route the server registers, as (method, path) (AP-01). The page has no other way to reach SQLite: no generic
/// table route, no SQL, file-system or shell access, no Tauri IPC. A unit test below checks this list against the
/// `.route(...)` calls in the source, so a new route must be added here; `tests/auth.rs` (T-08-01) checks that each
/// one answers and that anything else is 404. A GET route also answers HEAD.
pub const ROUTES: &[(&str, &str)] = &[
    ("GET", "/"),
    ("GET", "/customers"),
    ("GET", "/customers/new"),
    ("GET", "/parties"),
    ("POST", "/customers/new"),
    ("GET", "/sales/{doc_number}"),
    ("GET", "/sales/{doc_number}/invoice.pdf"),
    ("POST", "/sales/{doc_number}/save-pdf"),
    ("POST", "/print"),
    ("GET", "/setup"),
    ("POST", "/setup"),
    ("GET", "/settings"),
    ("PUT", "/settings"),
    ("POST", "/settings/backup"),
    ("POST", "/settings/restore/check"),
    ("POST", "/settings/restore"),
    // assets.rs: the embedded static files.
    ("GET", "/assets/{*path}"),
    // session.rs: the one-time launch URL of AP-13.
    ("GET", "/session/{token}"),
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard::page))
        .route("/customers", get(customers::list))
        .route("/customers/new", get(customers::new_dialog).post(customers::create))
        .route("/parties", get(parties::list))
        .route("/sales/{doc_number}", get(sales::page))
        .route("/sales/{doc_number}/invoice.pdf", get(sales::pdf))
        .route("/sales/{doc_number}/save-pdf", post(sales::save_pdf))
        .route("/print", post(print))
        .route("/setup", get(setup::page).post(setup::submit))
        .route("/settings", get(settings::page).put(settings::update))
        .route("/settings/backup", post(backup::create))
        .route("/settings/restore/check", post(backup::check))
        .route("/settings/restore", post(backup::restore))
}

/// Opens the native print dialog for the current page (204). Without a desktop window (the dev server in a
/// browser) it answers 501 and the page falls back to window.print().
async fn print(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    match &state.print {
        Some(print) => print().map(|()| StatusCode::NO_CONTENT).map_err(AppError::Internal),
        None => Ok(StatusCode::NOT_IMPLEMENTED),
    }
}

/// Whether the sidebar shows the DEMO badge: the database holds the demo data (see [`crate::db::has_demo_data`]).
pub fn demo(state: &AppState) -> Result<bool, AppError> {
    let conn = state.db.lock().map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(crate::db::has_demo_data(&conn)?)
}

/// Which sidebar link is current; the layout marks it with aria-current.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Dashboard,
    Customers,
    Parties,
    Invoices,
    Settings,
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
    /// (method, path) of each `.route("path", get(..).post(..))` call in a source file.
    fn routes_in(src: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for call in src.split(".route(\"").skip(1) {
            let (path, rest) = call.split_once('"').unwrap();
            // The handler chain ends at the newline after the call (one route per line, rustfmt keeps it so).
            let chain = rest.lines().next().unwrap();
            for method in ["get", "post", "put", "patch", "delete"] {
                if chain.contains(&format!("{method}(")) {
                    found.push((method.to_uppercase(), path.to_owned()));
                }
            }
        }
        found
    }

    #[test]
    fn route_list_matches_the_router() {
        let mut registered: Vec<_> =
            [include_str!("mod.rs"), include_str!("../assets.rs"), include_str!("../session.rs")]
                .iter()
                .flat_map(|src| routes_in(src))
                // Skips this test's own split pattern and doc comment.
                .filter(|(_, p)| p.starts_with('/'))
                .collect();
        registered.sort();
        let mut listed: Vec<_> = super::ROUTES.iter().map(|(m, p)| (m.to_string(), p.to_string())).collect();
        listed.sort();
        assert_eq!(registered, listed, "routes::ROUTES must list exactly the registered routes (AP-01)");
    }

    #[test]
    fn initials() {
        assert_eq!(super::initials("Halden & Pryce Architects"), "HP");
        assert_eq!(super::initials("Tomás Ferreira"), "TF");
        assert_eq!(super::initials(""), "");
    }
}
