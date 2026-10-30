//! KarbonFM local server: the axum router the desktop window loads over 127.0.0.1.

use axum::{response::Html, routing::get, Router};
use std::net::{Ipv4Addr, SocketAddr};
use tokio::net::TcpListener;

pub fn router() -> Router {
    Router::new().route("/", get(|| async { Html("<h1>KarbonFM</h1><p>The local server is running.</p>") }))
}

/// Binds 127.0.0.1 on a free port. Loopback only (AP-10, AP-13).
pub async fn bind() -> std::io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

pub async fn serve(listener: TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router()).await
}
