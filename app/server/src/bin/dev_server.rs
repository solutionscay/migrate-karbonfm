//! Serves the app on http://127.0.0.1:<port>/ for working on pages in a normal browser.
//! The desktop app does the same thing inside its window. Pass a port as the first argument (default 3000).
//! The database is in memory: migrated, reference-seeded and demo-seeded at every start. Log lines go to stderr.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::args().nth(1).map(|p| p.parse()).transpose()?.unwrap_or(3000);
    let (db, _) = karbon_server::db::open_in_memory(karbon_server::db::Seed::Demo)?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!("KarbonFM dev server on http://{}/", listener.local_addr()?);
    karbon_server::serve(listener, karbon_server::AppState::new(db)).await?;
    Ok(())
}
