//! KarbonFM local server: the axum router the desktop window loads over 127.0.0.1.
//!
//! The page talks to this server only (no Tauri IPC). Each route renders an askama template: a full page for a
//! normal request, a fragment for an htmx request (`HX-Request` header).

pub mod activities;
pub mod admin;
pub mod applog;
pub mod assets;
pub mod audit;
pub mod backup;
#[cfg(feature = "big-demo")]
pub mod big_demo;
pub mod contacts;
pub mod db;
pub mod demo_dates;
pub mod discussions;
pub mod email;
pub mod files;
pub mod ids;
pub mod import;
pub mod invoice;
pub mod items;
pub mod money;
pub mod net;
pub mod notifications;
pub mod opportunities;
pub mod parties;
pub mod payments;
pub mod pdf;
pub mod reference;
pub mod reports;
mod routes;
pub mod sales;
pub mod sales_list;
pub mod secrets;
pub mod session;
pub mod templates;
mod web;

use axum::Router;
pub use routes::ROUTES;
use rusqlite::Connection;
use secrets::{MemoryStore, SecretStore};
use session::Session;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

/// Saves a file through the desktop shell: native save dialog with the suggested name, write, open in the system
/// viewer. Returns the saved path, or `None` when the user cancels. It blocks until the dialog closes.
pub type SaveHook = Arc<dyn Fn(String, Vec<u8>) -> Result<Option<PathBuf>, String> + Send + Sync>;

/// Which folder the shell's native folder dialog asks for (its title and button say so).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderPick {
    /// Where to write a new backup (OP-ADMIN-06).
    BackupDestination,
    /// The backup folder to restore (OP-ADMIN-07).
    RestoreSource,
}

/// Shows the native folder dialog through the desktop shell and returns the chosen folder, or `None` when the user
/// cancels. It blocks until the dialog closes. The page has no IPC and no file-system access (AP-01, AP-02): it asks
/// the server, the server asks the shell, the same pattern as the save hook.
pub type FolderHook = Arc<dyn Fn(FolderPick) -> Result<Option<PathBuf>, String> + Send + Sync>;

/// Opens a stored file in the system's default app through the desktop shell (UI-19 "Open"). The server resolves the
/// path inside the file store first (`files::resolve`); the page only ever names a file id (AP-01, AP-02).
pub type OpenHook = Arc<dyn Fn(PathBuf) -> Result<(), String> + Send + Sync>;

/// Shared state of the server. One SQLite connection: the app is single-user and the server is the only writer.
#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    save: Option<SaveHook>,
    /// Opens a stored file with the system app; none in the dev server and the tests (`POST /files/{id}/open` 501).
    open: Option<OpenHook>,
    /// Sends email (OP-INTEG-01): lettre's SMTP transport; tests may pass another.
    mailer: Arc<dyn email::Mailer>,
    /// The server's own origin, `http://127.0.0.1:<port>`. Requests must name it in Host (and in Origin for
    /// writes), which blocks DNS rebinding and cross-site requests from other pages (AP-13, R-07).
    origin: Option<String>,
    /// The per-launch session cookie that only the app window gets (AP-13).
    session: Option<Arc<Session>>,
    /// The keychain side of the secrets (AP-03): `secrets::system_store()` in the desktop app (the keychain, or
    /// `NoKeychain` when the computer has none), `MemoryStore` (the default) in tests and the dev server. Routes use
    /// [`AppState::secret_store`], which adds the opt-in `secrets.toml` of the data folder (D-09).
    secrets: Arc<dyn SecretStore>,
    /// Set once setup is seen done (it is never undone), so the setup gate stops asking SQLite.
    setup_done: Arc<AtomicBool>,
    /// The native folder dialog of the shell (backup and restore); none in the dev server unless it is given one.
    folder: Option<FolderHook>,
    /// The folder of karbonfm.db and the file store; restore needs it (an in-memory database cannot be swapped).
    data_dir: Option<PathBuf>,
    /// A backup folder that passed the restore checks, waiting for the user to confirm: (id, folder). The page only
    /// ever sees the id, never a path (AP-01).
    pending_restore: Arc<Mutex<Option<(String, PathBuf)>>>,
    /// A fixed "today" for the pages that read the date (tests, `karbon-dev-server --as-of`); none means the local date.
    today: Option<String>,
}

impl AppState {
    pub fn new(db: Connection) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            save: None,
            open: None,
            mailer: Arc::new(email::LettreMailer::default()),
            origin: None,
            session: None,
            secrets: Arc::new(MemoryStore::default()),
            setup_done: Arc::new(AtomicBool::new(false)),
            folder: None,
            data_dir: None,
            pending_restore: Arc::new(Mutex::new(None)),
            today: None,
        }
    }

    /// Pins "today" (`YYYY-MM-DD`) for the pages that read the date: overdue, revenue this month, the day headers.
    /// Stamps keep the real clock (`Stamp::at` serves tests).
    pub fn with_today(mut self, today: impl Into<String>) -> Self {
        self.today = Some(today.into());
        self
    }

    /// Today as the pages read it: the pinned date, or the local date.
    pub fn today(&self) -> String {
        self.today.clone().unwrap_or_else(|| audit::local_now()[..10].to_owned())
    }

    /// The desktop shell passes its native folder dialog (backup and restore).
    pub fn with_folder_picker(mut self, folder: FolderHook) -> Self {
        self.folder = Some(folder);
        self
    }

    /// The data folder that holds karbonfm.db and the file store (`backup::files_dir`).
    pub fn with_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(dir.into());
        self
    }

    /// The secret store; the desktop shell passes `secrets::system_store()`.
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.secrets = secrets;
        self
    }

    /// The secrets as the routes use them: `secrets.toml` in the data folder while it exists, else the keychain.
    pub fn secret_store(&self) -> secrets::Secrets {
        secrets::Secrets::new(self.secrets.clone(), self.data_dir.as_deref().map(secrets::secrets_path))
    }

    /// The desktop shell passes its save function. The window never navigates to a file (it would replace the app
    /// page and strand the window), and the page has no IPC, so the page asks the server and the server asks the shell.
    pub fn with_save(mut self, save: SaveHook) -> Self {
        self.save = Some(save);
        self
    }

    /// The desktop shell passes its "open with the system app" function (the same one that shows a saved PDF).
    /// The email sender (OP-INTEG-01); the default is lettre's SMTP transport.
    pub fn with_mailer(mut self, mailer: Arc<dyn email::Mailer>) -> Self {
        self.mailer = mailer;
        self
    }

    pub fn with_open(mut self, open: OpenHook) -> Self {
        self.open = Some(open);
        self
    }

    /// Set by `serve` from the bound address; tests set it to check the guard.
    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }

    /// The session the window opens with `Session::launch_path` (AP-13). `serve` refuses to start without one.
    pub fn with_session(mut self, session: Arc<Session>) -> Self {
        self.session = Some(session);
        self
    }
}

pub fn router(state: AppState) -> Router {
    routes::router()
        .merge(assets::router())
        .merge(session::router())
        .fallback(routes::page_not_found)
        .layer(axum::middleware::from_fn_with_state(state.clone(), routes::setup_gate))
        .layer(axum::middleware::from_fn_with_state(state.clone(), web::origin_guard))
        .layer(axum::middleware::from_fn(web::log_errors))
        .layer(axum::middleware::from_fn(web::security_headers))
        .with_state(state)
}

/// Binds 127.0.0.1 on a free port. Loopback only (AP-10, AP-13).
pub async fn bind() -> std::io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

/// Serves the router with the full guard on. Refuses a listener that is not on loopback (AP-10) and a state without
/// a session (AP-13).
pub async fn serve(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    let addr = listener.local_addr()?;
    if !addr.ip().is_loopback() {
        return Err(std::io::Error::other(format!("refusing to serve on {addr}: loopback only")));
    }
    if state.session.is_none() {
        return Err(std::io::Error::other("refusing to serve without a session (AP-13)"));
    }
    let origin = format!("http://{addr}");
    axum::serve(listener, router(state.with_origin(origin))).await
}
