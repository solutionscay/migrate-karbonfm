//! UI-17 Backup panel: OP-ADMIN-06 `backup_create` and OP-ADMIN-07 `backup_restore` (BR-ADMIN-06, BR-ADMIN-07).
//!
//! - `POST /settings/backup`: the shell's folder dialog, then a new backup folder in the chosen folder. Answers the
//!   panel with the result (200, or 422 with the reason); 204 when the dialog is cancelled.
//! - `POST /settings/restore/check`: the folder dialog, then [`backup::validate`]. A good backup answers the confirm
//!   dialog body (into the shared `<dialog>`); a refused one answers the panel with the reason (422, retargeted).
//! - `POST /settings/restore` with the id of that check: the restore itself, then `HX-Redirect` to the settings
//!   page, which says what was restored and where the safety copy is.
//!
//! The folder dialog and the file work block, so they run on `spawn_blocking`. The restore holds the server's only
//! connection from the safety copy to the reopen: no request reads a half-swapped database.

use super::current_user::CurrentUser;
use crate::applog::{log_write, Entry};
use crate::backup::{self, BackupError};
use crate::web::AppError;
use crate::{AppState, FolderPick};
use askama::Template;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// A result line on the panel.
pub struct Notice {
    pub ok: bool,
    pub text: String,
    pub detail: Option<String>,
}

#[derive(Template)]
#[template(path = "settings/backup_panel.html")]
pub struct Panel {
    pub available: bool,
    pub notice: Option<Notice>,
}

impl Panel {
    pub fn for_state(state: &AppState, notice: Option<Notice>) -> Self {
        Self { available: state.folder.is_some(), notice }
    }
}

#[derive(Template)]
#[template(path = "settings/restore_confirm.html")]
struct Confirm {
    pending: String,
    error: Option<String>,
    created_at: String,
    folder_name: String,
    parties: i64,
    transactions: i64,
    total_rows: i64,
    files: u64,
    app_version: String,
    schema_version: i64,
}

impl Confirm {
    fn new(pending: String, b: &backup::Backup) -> Self {
        let m = &b.manifest;
        Self {
            pending,
            error: None,
            created_at: m.created_at.clone(),
            folder_name: b.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            parties: m.rows("party"),
            transactions: m.rows("sales_transaction"),
            total_rows: m.total_rows(),
            files: m.files,
            app_version: m.app_version.clone(),
            schema_version: m.schema_version,
        }
    }
}

fn panel(state: &AppState, status: StatusCode, notice: Notice) -> Result<Response, AppError> {
    Ok((status, Html(Panel::for_state(state, Some(notice)).render()?)).into_response())
}

fn refused(state: &AppState, text: String) -> Result<Response, AppError> {
    panel(state, StatusCode::UNPROCESSABLE_ENTITY, Notice { ok: false, text, detail: None })
}

/// Runs the shell's folder dialog off the async workers. `Ok(None)`: cancelled, or no dialog in this build.
async fn pick(state: &AppState, what: FolderPick) -> Result<Option<PathBuf>, AppError> {
    let Some(hook) = state.folder.clone() else { return Ok(None) };
    tokio::task::spawn_blocking(move || hook(what))
        .await
        .map_err(|e| AppError::Internal(format!("folder dialog task: {e}")))?
        .map_err(|e| AppError::Internal(format!("folder dialog: {e}")))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AppError::Internal(format!("backup task: {e}")))
}

/// `POST /settings/backup` (OP-ADMIN-06).
pub async fn create(State(state): State<AppState>, _user: CurrentUser) -> Result<Response, AppError> {
    if state.folder.is_none() {
        return refused(&state, "Backup is available in the desktop app.".into());
    }
    let Some(parent) = pick(&state, FolderPick::BackupDestination).await? else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let db = state.db.clone();
    let files = state.data_dir.as_deref().map(backup::files_dir).unwrap_or_default();
    let result = blocking(move || {
        let conn = db.lock().map_err(|e| BackupError::Io(e.to_string()))?;
        backup::create(&conn, &files, &parent)
    })
    .await?;
    match result {
        Ok(b) => {
            let m = &b.manifest;
            let text = format!("Backed up {} rows and {} attached files at {}.", m.total_rows(), m.files, m.created_at);
            log_write(&Entry::info("backup_create", &format!("backup written to {}", b.dir.display())));
            let mut res =
                panel(&state, StatusCode::OK, Notice { ok: true, text, detail: Some(b.dir.display().to_string()) })?;
            res.headers_mut().insert("HX-Trigger", HeaderValue::from_static(r#"{"karbon-toast":"Backup saved"}"#));
            Ok(res)
        }
        Err(e) => {
            log_write(&Entry::error("backup_create", "Storage", &e.to_string()));
            refused(&state, format!("The backup was not made: {e}"))
        }
    }
}

/// `POST /settings/restore/check` (first half of OP-ADMIN-07): nothing changes here.
pub async fn check(State(state): State<AppState>, _user: CurrentUser) -> Result<Response, AppError> {
    let retarget = |mut res: Response| {
        res.headers_mut().insert("HX-Retarget", HeaderValue::from_static("#backup-panel"));
        res.headers_mut().insert("HX-Reswap", HeaderValue::from_static("outerHTML"));
        Ok(res)
    };
    if state.folder.is_none() || state.data_dir.is_none() {
        return retarget(refused(&state, "Restore is available in the desktop app.".into())?);
    }
    let Some(dir) = pick(&state, FolderPick::RestoreSource).await? else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let checked = blocking({
        let dir = dir.clone();
        move || backup::validate(&dir)
    })
    .await?;
    match checked {
        Ok(b) => {
            let id = crate::ids::new_id();
            *state.pending_restore.lock().map_err(|e| AppError::Internal(e.to_string()))? = Some((id.clone(), dir));
            Ok(Html(Confirm::new(id, &b).render()?).into_response())
        }
        Err(e) => {
            log_write(&Entry::error("backup_restore", "Validation", &format!("{}: {e}", dir.display())));
            retarget(refused(&state, e.to_string())?)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreForm {
    pending: String,
}

/// `POST /settings/restore` (OP-ADMIN-07): the confirmed restore.
pub async fn restore(
    State(state): State<AppState>,
    _user: CurrentUser,
    Form(form): Form<RestoreForm>,
) -> Result<Response, AppError> {
    let taken = {
        let mut slot = state.pending_restore.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        match slot.as_ref() {
            Some((id, _)) if *id == form.pending => slot.take().map(|(_, dir)| dir),
            _ => None,
        }
    };
    let (Some(dir), Some(data_dir)) = (taken, state.data_dir.clone()) else {
        let html = r#"<div class="p-5 text-[13px]" role="alert">This restore was not checked, or was already done. Close this dialog and choose the backup again.</div>"#;
        return Ok((StatusCode::CONFLICT, Html(html)).into_response());
    };
    let db = state.db.clone();
    let result = blocking({
        let dir = dir.clone();
        move || {
            let mut conn = db.lock().map_err(|e| BackupError::Io(e.to_string()))?;
            backup::restore(&mut conn, &data_dir, &dir)
        }
    })
    .await?;
    match result {
        Ok(r) => {
            // The restored data may be another install's: let the setup gate look again.
            state.setup_done.store(false, Ordering::Relaxed);
            log_write(&Entry::info(
                "backup_restore",
                &format!(
                    "restored {} (made {}); safety copy {}",
                    dir.display(),
                    r.manifest.created_at,
                    r.safety.display()
                ),
            ));
            let to = format!(
                "/settings?restored={}&safety={}",
                encode(&r.manifest.created_at),
                encode(&r.safety.file_name().unwrap_or_default().to_string_lossy())
            );
            let mut res = StatusCode::OK.into_response();
            res.headers_mut()
                .insert("HX-Redirect", HeaderValue::from_str(&to).map_err(|e| AppError::Internal(e.to_string()))?);
            Ok(res)
        }
        Err(e) => {
            log_write(&Entry::error("backup_restore", "Storage", &format!("{}: {e}", dir.display())));
            let html = format!(
                r#"<div class="p-5 text-[13px]" role="alert">The backup was not restored: {}</div>"#,
                askama::filters::escape(&e.to_string(), askama::filters::Html).map_err(askama::Error::from)?
            );
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
        }
    }
}

/// Percent-encodes a query value (letters, digits and `-._~` stay).
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The notice the settings page shows after a restore (from the query of the redirect).
#[derive(Deserialize, Default)]
pub struct RestoredQuery {
    pub restored: Option<String>,
    pub safety: Option<String>,
}

impl RestoredQuery {
    pub fn notice(&self) -> Option<Notice> {
        let made = self.restored.as_deref()?;
        Some(Notice {
            ok: true,
            text: format!("Restored the backup made {made}."),
            detail: self.safety.as_deref().map(|s| {
                format!("The data from before the restore is kept in the app's data folder: {}/{s}", backup::SAFETY_DIR)
            }),
        })
    }
}
