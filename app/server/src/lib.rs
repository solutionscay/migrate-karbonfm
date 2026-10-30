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

/// Shared state of the server. One SQLite connection: the app is single-user and the server is the only writer.
#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
}

impl AppState {
    pub fn new(db: Connection) -> Self {
        Self { db: Arc::new(Mutex::new(db)) }
    }
}

pub fn router(state: AppState) -> Router {
    routes::router().merge(assets::router()).with_state(state).layer(axum::middleware::from_fn(web::security_headers))
}

/// Binds 127.0.0.1 on a free port. Loopback only (AP-10, AP-13).
pub async fn bind() -> std::io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

pub async fn serve(listener: TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}
