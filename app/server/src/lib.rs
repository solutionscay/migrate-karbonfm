//! KarbonFM local server: the axum router the desktop window loads over 127.0.0.1.
//!
//! The page talks to this server only (no Tauri IPC). Each route renders an askama template: a full page for a
//! normal request, a fragment for an htmx request (`HX-Request` header).

pub mod assets;
pub mod db;
pub mod money;
mod routes;
mod web;

use axum::Router;
use rusqlite::Connection;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

/// Opens the native print dialog of the window that shows the app (set by the desktop shell).
pub type PrintHook = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

/// Shared state of the server. One SQLite connection: the app is single-user and the server is the only writer.
#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    print: Option<PrintHook>,
    /// The server's own origin, `http://127.0.0.1:<port>`. Requests must name it in Host (and in Origin for
    /// writes), which blocks DNS rebinding and cross-site requests from other pages (AP-13, R-07).
    origin: Option<String>,
}

impl AppState {
    pub fn new(db: Connection) -> Self {
        Self { db: Arc::new(Mutex::new(db)), print: None, origin: None }
    }

    /// The desktop shell passes the window's print function. `window.print()` does nothing in WKWebView on macOS
    /// (S-01), so the page asks the server, and the server asks the window; the page itself has no IPC access.
    pub fn with_print(mut self, print: PrintHook) -> Self {
        self.print = Some(print);
        self
    }

    /// Set by `serve` from the bound address; tests set it to check the guard.
    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }
}

pub fn router(state: AppState) -> Router {
    routes::router()
        .merge(assets::router())
        .layer(axum::middleware::from_fn_with_state(state.clone(), web::origin_guard))
        .layer(axum::middleware::from_fn(web::security_headers))
        .with_state(state)
}

/// Binds 127.0.0.1 on a free port. Loopback only (AP-10, AP-13).
pub async fn bind() -> std::io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

pub async fn serve(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    let origin = format!("http://{}", listener.local_addr()?);
    axum::serve(listener, router(state.with_origin(origin))).await
}
