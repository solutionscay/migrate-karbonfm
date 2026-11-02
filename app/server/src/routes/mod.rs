//! Route handlers (05). Each handler queries or calls a command, then renders a template.

mod activities;
mod backup;
mod contact_forms;
pub mod current_user;
mod customers;
mod dashboard;
mod discussions;
mod parties;
mod party_forms;
mod relation_forms;
mod sales;
mod sales_lists;
mod secrets_store;
mod settings;
mod setup;
mod templates;
mod timeline;
mod wizard;

pub use setup::setup_gate;

use crate::web::AppError;
use crate::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;

/// Every route the server registers, as (method, path) (AP-01). The page has no other way to reach SQLite: no generic
/// table route, no SQL, file-system or shell access, no Tauri IPC. A unit test below checks this list against the
/// `.route(...)` calls in the source, so a new route must be added here; `tests/auth.rs` (T-08-01) checks that each
/// one answers and that anything else is 404. A GET route also answers HEAD.
pub const ROUTES: &[(&str, &str)] = &[
    ("GET", "/"),
    ("GET", "/customers"),
    ("GET", "/parties"),
    ("GET", "/parties/new"),
    ("POST", "/parties/wizard"),
    ("GET", "/parties/by-contact"),
    ("GET", "/pickers/{kind}"),
    ("GET", "/parties/people/new"),
    ("POST", "/parties/people"),
    ("GET", "/parties/organizations/new"),
    ("POST", "/parties/organizations"),
    ("GET", "/parties/{id}"),
    ("PATCH", "/parties/{id}"),
    ("GET", "/parties/{id}/edit"),
    ("GET", "/parties/{id}/archive"),
    ("POST", "/parties/{id}/archive"),
    ("POST", "/parties/{id}/roles"),
    ("GET", "/parties/{id}/contact-methods/new"),
    ("POST", "/parties/{id}/contact-methods"),
    ("GET", "/contact-methods/{id}/edit"),
    ("PUT", "/contact-methods/{id}"),
    ("DELETE", "/contact-methods/{id}"),
    ("POST", "/contact-methods/{id}/primary"),
    ("POST", "/contact-methods/{id}/purposes/{type_id}"),
    ("GET", "/parties/{id}/relations/new"),
    ("POST", "/parties/{id}/relations"),
    ("GET", "/relations/{id}/edit"),
    ("PATCH", "/relations/{id}"),
    ("DELETE", "/relations/{id}"),
    ("GET", "/activities"),
    ("GET", "/activities/new"),
    ("POST", "/activities"),
    ("GET", "/activities/{id}"),
    ("PATCH", "/activities/{id}"),
    ("DELETE", "/activities/{id}"),
    ("GET", "/activities/{id}/edit"),
    ("GET", "/activities/{id}/parties/new"),
    ("PUT", "/activities/{id}/parties"),
    ("GET", "/activity-parties/{id}/edit"),
    ("DELETE", "/activity-parties/{id}"),
    ("POST", "/tasks/{id}/complete"),
    ("GET", "/discussions/new"),
    ("POST", "/discussions"),
    ("GET", "/discussions/{id}"),
    ("PATCH", "/discussions/{id}"),
    ("GET", "/discussions/{id}/subscribers"),
    ("POST", "/discussions/{id}/comments"),
    ("GET", "/notifications"),
    ("POST", "/notifications/read"),
    ("GET", "/sales"),
    ("GET", "/payments"),
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
    ("GET", "/settings/secrets/file"),
    ("POST", "/settings/secrets/file"),
    ("POST", "/settings/secrets/keychain"),
    ("GET", "/templates"),
    ("GET", "/templates/new"),
    ("POST", "/templates"),
    ("GET", "/templates/{id}"),
    ("PUT", "/templates/{id}"),
    ("PUT", "/templates/{id}/merge-fields"),
    ("POST", "/templates/{id}/merge"),
    // assets.rs: the embedded static files.
    ("GET", "/assets/{*path}"),
    // session.rs: the one-time launch URL of AP-13.
    ("GET", "/session/{token}"),
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard::page))
        .route("/customers", get(customers::list))
        .route("/parties", get(parties::list))
        .route("/parties/new", get(wizard::page))
        .route("/parties/wizard", post(wizard::submit))
        .route("/parties/by-contact", get(relation_forms::by_contact))
        .route("/pickers/{kind}", get(relation_forms::picker))
        .route("/parties/people/new", get(party_forms::new_person))
        .route("/parties/people", post(party_forms::create_person))
        .route("/parties/organizations/new", get(party_forms::new_organization))
        .route("/parties/organizations", post(party_forms::create_organization))
        .route("/parties/{id}", get(parties::detail).patch(party_forms::update).fallback(not_found))
        .route("/parties/{id}/edit", get(party_forms::edit))
        .route("/parties/{id}/archive", get(party_forms::archive_dialog).post(party_forms::archive))
        .route("/parties/{id}/roles", post(party_forms::add_role))
        .route("/parties/{id}/contact-methods/new", get(contact_forms::new_dialog))
        .route("/parties/{id}/contact-methods", post(contact_forms::create))
        .route("/contact-methods/{id}/edit", get(contact_forms::edit_dialog))
        .route("/contact-methods/{id}", put(contact_forms::update).delete(contact_forms::delete))
        .route("/contact-methods/{id}/primary", post(contact_forms::set_primary))
        .route("/contact-methods/{id}/purposes/{type_id}", post(contact_forms::toggle_purpose))
        .route("/parties/{id}/relations/new", get(relation_forms::new_dialog))
        .route("/parties/{id}/relations", post(relation_forms::create))
        .route("/relations/{id}/edit", get(relation_forms::edit_dialog))
        .route("/relations/{id}", patch(relation_forms::update).delete(relation_forms::delete))
        .route("/activities/new", get(activities::new_dialog))
        .route("/activities", get(timeline::page).post(activities::create))
        .route("/activities/{id}", get(activities::detail).patch(activities::update).delete(activities::delete))
        .route("/activities/{id}/edit", get(activities::edit_dialog))
        .route("/activities/{id}/parties/new", get(activities::party_new))
        .route("/activities/{id}/parties", put(activities::party_set))
        .route("/activity-parties/{id}/edit", get(activities::party_edit_dialog))
        .route("/activity-parties/{id}", delete(activities::party_remove))
        .route("/tasks/{id}/complete", post(timeline::complete))
        .route("/discussions/new", get(discussions::new_dialog))
        .route("/discussions", post(discussions::create))
        .route("/discussions/{id}", get(discussions::detail).patch(discussions::update))
        .route("/discussions/{id}/subscribers", get(discussions::subscribers_dialog))
        .route("/discussions/{id}/comments", post(discussions::comment))
        .route("/notifications", get(discussions::notifications))
        .route("/notifications/read", post(discussions::mark_read))
        .route("/sales", get(sales_lists::sales))
        .route("/payments", get(sales_lists::payments))
        .route("/sales/{doc_number}", get(sales::page))
        .route("/sales/{doc_number}/invoice.pdf", get(sales::pdf))
        .route("/sales/{doc_number}/save-pdf", post(sales::save_pdf))
        .route("/print", post(print))
        .route("/setup", get(setup::page).post(setup::submit))
        .route("/settings", get(settings::page).put(settings::update))
        .route("/settings/backup", post(backup::create))
        .route("/settings/restore/check", post(backup::check))
        .route("/settings/restore", post(backup::restore))
        .route("/settings/secrets/file", get(secrets_store::confirm).post(secrets_store::use_file))
        .route("/settings/secrets/keychain", post(secrets_store::use_keychain))
        .route("/templates", get(templates::list).post(templates::create))
        .route("/templates/new", get(templates::new_dialog))
        .route("/templates/{id}", get(templates::detail).put(templates::save))
        .route("/templates/{id}/merge-fields", put(templates::save_merge_fields))
        .route("/templates/{id}/merge", post(templates::merge))
}

/// Opens the native print dialog for the current page (204). Without a desktop window (the dev server in a
/// browser) it answers 501 and the page falls back to window.print().
async fn print(State(state): State<AppState>) -> Result<StatusCode, AppError> {
    match &state.print {
        Some(print) => print().map(|()| StatusCode::NO_CONTENT).map_err(AppError::Internal),
        None => Ok(StatusCode::NOT_IMPLEMENTED),
    }
}

/// A 404 for the other methods of a path (T-08-07: a party has no delete route, it is only archived).
async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
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
    Activity,
    Customers,
    Parties,
    Invoices,
    Payments,
    Templates,
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
