//! KarbonFM local server: the axum router the desktop window loads over 127.0.0.1.
//!
//! The page talks to this server only (no Tauri IPC). Each route renders an askama template: a full page for a
//! normal request, a fragment for an htmx request (`HX-Request` header).

pub mod admin;
pub mod applog;
pub mod assets;
pub mod audit;
pub mod backup;
pub mod db;
pub mod ids;
pub mod import;
pub mod invoice;
pub mod money;
pub mod net;
pub mod parties;
pub mod pdf;
pub mod reference;
mod routes;
pub mod secrets;
pub mod session;
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

/// Opens the native print dialog of the window that shows the app (set by the desktop shell).
pub type PrintHook = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

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
/// the server, the server asks the shell, the same pattern as the print hook.
pub type FolderHook = Arc<dyn Fn(FolderPick) -> Result<Option<PathBuf>, String> + Send + Sync>;

/// Shared state of the server. One SQLite connection: the app is single-user and the server is the only writer.
#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    print: Option<PrintHook>,
    save: Option<SaveHook>,
    /// The server's own origin, `http://127.0.0.1:<port>`. Requests must name it in Host (and in Origin for
    /// writes), which blocks DNS rebinding and cross-site requests from other pages (AP-13, R-07).
    origin: Option<String>,
    /// The per-launch session cookie that only the app window gets (AP-13).
    session: Option<Arc<Session>>,
    /// Keychain secrets (AP-03): `Keychain` in the desktop app, `MemoryStore` (the default) in tests and the dev server.
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
}

impl AppState {
    pub fn new(db: Connection) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            print: None,
            save: None,
            origin: None,
            session: None,
            secrets: Arc::new(MemoryStore::default()),
            setup_done: Arc::new(AtomicBool::new(false)),
            folder: None,
            data_dir: None,
            pending_restore: Arc::new(Mutex::new(None)),
        }
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

    /// The secret store; the desktop shell passes `secrets::Keychain`.
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.secrets = secrets;
        self
    }

    /// The desktop shell passes the window's print function. `window.print()` does nothing in WKWebView on macOS
    /// (S-01), so the page asks the server, and the server asks the window; the page itself has no IPC access.
    pub fn with_print(mut self, print: PrintHook) -> Self {
        self.print = Some(print);
        self
    }

    /// The desktop shell passes its save function. The window never navigates to a file (it would replace the app
    /// page and strand the window), and the page has no IPC, so the page asks the server and the server asks the shell.
    pub fn with_save(mut self, save: SaveHook) -> Self {
        self.save = Some(save);
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
