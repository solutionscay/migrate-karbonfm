//! Serves the app on http://127.0.0.1:<port>/ for working on pages in a normal browser.
//! The desktop app does the same thing inside its window. Pass a port as the first argument (default 3000).
//! The database is in memory: migrated, reference-seeded and demo-seeded at every start. Log lines go to stderr.
//!
//! The server has the same guard as the app (AP-13): open the one-time URL it prints once per run. The browser then
//! keeps the session cookie until the server stops; reloads and other tabs of the same browser work.

use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::args().nth(1).map(|p| p.parse()).transpose()?.unwrap_or(3000);
    let (db, _) = karbon_server::db::open_in_memory(karbon_server::db::Seed::Demo)?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let addr = listener.local_addr()?;
    let session = Arc::new(karbon_server::session::Session::new(addr.port()));
    let launch = session.launch_path().ok_or("session already used")?;
    println!("KarbonFM dev server on http://{addr}/");
    println!("Open this one-time URL in your browser: http://{addr}{launch}");
    karbon_server::serve(listener, karbon_server::AppState::new(db).with_session(session)).await?;
    Ok(())
}
