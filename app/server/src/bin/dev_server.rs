//! Serves the app on http://127.0.0.1:<port>/ for working on pages in a normal browser.
//! The desktop app does the same thing inside its window. Pass a port as the first argument (default 3000).
//! The database is in memory: migrated, reference-seeded and demo-seeded at every start (`--fresh`: no demo seed, so
//! every page goes to /setup). Secrets are in memory (MemoryStore). Log lines go to stderr.
//!
//! Backup and restore (Settings) need a database file and a folder dialog, which a browser does not give the server:
//! - `--data-dir DIR` keeps karbonfm.db and the file store in DIR instead of memory (kept between runs);
//! - `--backup-folder DIR` stands in for the native folder dialog: "Back up now" writes into DIR, and "Restore from a
//!   backup" offers the newest backup folder in DIR.
//!
//! The server has the same guard as the app (AP-13): open the one-time URL it prints once per run. The browser then
//! keeps the session cookie until the server stops; reloads and other tabs of the same browser work.

use karbon_server::db::{self, Seed};
use karbon_server::{FolderHook, FolderPick};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (mut port, mut fresh, mut data_dir, mut backup_folder) = (3000u16, false, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fresh" => fresh = true,
            "--data-dir" => data_dir = Some(PathBuf::from(args.next().ok_or("--data-dir needs a folder")?)),
            "--backup-folder" => {
                backup_folder = Some(PathBuf::from(args.next().ok_or("--backup-folder needs a folder")?))
            }
            p => port = p.parse()?,
        }
    }
    let seed = if fresh { Seed::Reference } else { Seed::Demo };
    let db = match &data_dir {
        Some(dir) => db::open(&db::database_path(dir), seed)?.0,
        None => db::open_in_memory(seed)?.0,
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let addr = listener.local_addr()?;
    let session = Arc::new(karbon_server::session::Session::new(addr.port()));
    let launch = session.launch_path().ok_or("session already used")?;
    let mut state = karbon_server::AppState::new(db).with_session(session);
    if let Some(dir) = data_dir {
        state = state.with_data_dir(dir);
    }
    if let Some(folder) = backup_folder {
        std::fs::create_dir_all(&folder)?;
        state = state.with_folder_picker(folder_stand_in(folder));
    }
    println!("KarbonFM dev server on http://{addr}/");
    println!("Open this one-time URL in your browser: http://{addr}{launch}");
    karbon_server::serve(listener, state).await?;
    Ok(())
}

/// The dev server's folder "dialog": the given folder for a backup, its newest backup folder for a restore.
fn folder_stand_in(folder: PathBuf) -> FolderHook {
    Arc::new(move |what| match what {
        FolderPick::BackupDestination => Ok(Some(folder.clone())),
        FolderPick::RestoreSource => {
            let mut backups: Vec<PathBuf> = std::fs::read_dir(&folder)
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            backups.sort();
            Ok(backups.pop())
        }
    })
}
