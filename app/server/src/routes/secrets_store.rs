//! Where the secrets are kept when the computer has no keychain (D-09 as changed 2026-10-31, issue #7; OP-ADMIN-03).
//!
//! - `GET /settings/secrets/file`: the confirm dialog body, into the shared in-page `<dialog>` (never a native
//!   dialog). It explains the trade-off; 422 with the reason when the file option does not apply.
//! - `POST /settings/secrets/file`: the opt-in. Creates `secrets.toml` (0600) in the data folder, then
//!   `HX-Redirect: /settings`. Refused (422) while a keychain is running, or without a data folder.
//! - `POST /settings/secrets/keychain`: moves the secrets of the file to the keychain and removes the file, then
//!   `HX-Redirect: /settings`. Refused (422, the notice with the reason) without a running keychain; a failure keeps
//!   the file and says so.
//!
//! The choice is the file itself: while `secrets.toml` exists it is the store, so a backup or a restore (which never
//! touch it) cannot change it. Nothing here falls back silently, and nothing reports a save that did not happen.

use super::current_user::CurrentUser;
use crate::applog::{log_write, Entry};
use crate::secrets::{Secrets, StoreKind};
use crate::web::AppError;
use crate::AppState;
use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

/// The notice at the top of the secrets panels (`settings/secrets_store.html`); empty while the keychain is used.
#[derive(Template)]
#[template(path = "settings/secrets_store.html")]
pub struct StorePanel {
    pub kind: StoreKind,
    pub file_offered: bool,
    pub keychain_available: bool,
    pub error: Option<String>,
}

impl StorePanel {
    /// Reads the data folder (does `secrets.toml` exist?): call it off the async workers.
    pub fn for_store(secrets: &Secrets, error: Option<String>) -> Self {
        Self {
            kind: secrets.kind(),
            file_offered: secrets.file_offered(),
            keychain_available: secrets.keychain_available(),
            error,
        }
    }
}

#[derive(Template)]
#[template(path = "settings/secrets_file_confirm.html")]
struct Confirm {
    can_confirm: bool,
    error: Option<String>,
}

/// Why the file option does not apply now, if it does not.
fn refusal(secrets: &Secrets) -> Option<&'static str> {
    match secrets.kind() {
        StoreKind::File => Some("Secrets are already kept in the file."),
        StoreKind::Keychain => Some("A system keychain is running, so passwords and keys stay there."),
        StoreKind::Unavailable if !secrets.file_offered() => {
            Some("The file option needs KarbonFM's data folder, which this build does not have.")
        }
        StoreKind::Unavailable => None,
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AppError::Internal(format!("secrets task: {e}")))
}

fn dialog(status: StatusCode, can_confirm: bool, error: Option<String>) -> Result<Response, AppError> {
    Ok((status, Html(Confirm { can_confirm, error }.render()?)).into_response())
}

/// A full reload of the settings page, which then says where the secrets are.
fn reload_settings() -> Response {
    (StatusCode::OK, [("HX-Redirect", "/settings")]).into_response()
}

/// `GET /settings/secrets/file`.
pub async fn confirm(State(state): State<AppState>, _user: CurrentUser) -> Result<Response, AppError> {
    let secrets = state.secret_store();
    match blocking(move || refusal(&secrets)).await? {
        None => dialog(StatusCode::OK, true, None),
        Some(why) => dialog(StatusCode::UNPROCESSABLE_ENTITY, false, Some(why.into())),
    }
}

/// `POST /settings/secrets/file`.
pub async fn use_file(State(state): State<AppState>, _user: CurrentUser) -> Result<Response, AppError> {
    let secrets = state.secret_store();
    let result = blocking(move || match refusal(&secrets) {
        Some(why) => Err(Ok(why)),
        None => secrets.use_file().map_err(Err),
    })
    .await?;
    match result {
        Ok(path) => {
            let message = format!("the user chose to keep secrets in {} (no system keychain)", path.display());
            log_write(&Entry::info("secrets_use_file", &message));
            Ok(reload_settings())
        }
        Err(Ok(why)) => dialog(StatusCode::UNPROCESSABLE_ENTITY, false, Some(why.into())),
        Err(Err(e)) => {
            log_write(&Entry::error("secrets_use_file", "Storage", &e.to_string()));
            let text = "The file could not be created, so nothing changed. The log has the details.";
            dialog(StatusCode::UNPROCESSABLE_ENTITY, true, Some(text.into()))
        }
    }
}

/// `POST /settings/secrets/keychain`.
pub async fn use_keychain(State(state): State<AppState>, _user: CurrentUser) -> Result<Response, AppError> {
    let secrets = state.secret_store();
    let (result, panel) = blocking(move || {
        let result = match secrets.kind() {
            StoreKind::File if secrets.keychain_available() => secrets.use_keychain().map_err(Err),
            StoreKind::File => {
                Err(Ok("No system keychain is running. Start GNOME Keyring or KWallet, then restart KarbonFM."))
            }
            _ => Err(Ok("Secrets are not kept in a file.")),
        };
        let panel = StorePanel::for_store(&secrets, None);
        (result, panel)
    })
    .await?;
    let refused = |panel: StorePanel, text: String| -> Result<Response, AppError> {
        let panel = StorePanel { error: Some(text), ..panel };
        Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(panel.render()?)).into_response())
    };
    match result {
        Ok(moved) => {
            let message = format!("moved {moved} secrets from the file to the system keychain; the file is removed");
            log_write(&Entry::info("secrets_use_keychain", &message));
            Ok(reload_settings())
        }
        Err(Ok(why)) => refused(panel, why.into()),
        Err(Err(e)) => {
            log_write(&Entry::error("secrets_use_keychain", "Storage", &e.to_string()));
            refused(
                panel,
                "The keychain did not take the secrets, so they are still in the file. The log has the details.".into(),
            )
        }
    }
}
