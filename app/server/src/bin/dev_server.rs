//! Serves the app on http://127.0.0.1:<port>/ for working on pages in a normal browser.
//! The desktop app does the same thing inside its window. Pass a port as the first argument (default 3000).

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::args().nth(1).map(|p| p.parse()).transpose()?.unwrap_or(3000);
    let db = karbon_server::db::open_demo(None)?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!("KarbonFM dev server on http://{}/", listener.local_addr()?);
    karbon_server::serve(listener, karbon_server::AppState::new(db)).await?;
    Ok(())
}
