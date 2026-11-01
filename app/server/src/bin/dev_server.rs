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
//! A big database (I3), to look at the app with a few thousand invoices:
//! - `--import EXPORT_DIR --data-dir DIR` loads a FileMaker export (one CSV per table) into DIR when DIR has no
//!   database yet (OP-IMPORT-01), then serves it; later runs reuse it;
//! - `--big-demo` (build with `--features big-demo`) generates the synthetic export of karbon-synth into
//!   `<data dir>/../export` and imports it the same way. Without `--data-dir` it uses `<temp>/karbonfm-big-demo`.
//!
//! The server has the same guard as the app (AP-13): open the one-time URL it prints once per run. The browser then
//! keeps the session cookie until the server stops; reloads and other tabs of the same browser work.

use karbon_server::db::{self, Seed};
use karbon_server::{FolderHook, FolderPick};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (mut port, mut fresh, mut data_dir, mut backup_folder) = (3000u16, false, None::<PathBuf>, None);
    let (mut import, mut big_demo) = (None::<PathBuf>, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fresh" => fresh = true,
            "--data-dir" => data_dir = Some(PathBuf::from(args.next().ok_or("--data-dir needs a folder")?)),
            "--backup-folder" => {
                backup_folder = Some(PathBuf::from(args.next().ok_or("--backup-folder needs a folder")?))
            }
            "--import" => import = Some(PathBuf::from(args.next().ok_or("--import needs an export folder")?)),
            "--big-demo" => big_demo = true,
            p => port = p.parse()?,
        }
    }
    if big_demo {
        let dir = data_dir.get_or_insert_with(|| std::env::temp_dir().join("karbonfm-big-demo").join("data")).clone();
        let export = dir.parent().unwrap_or(&dir).join("export");
        if !db::database_path(&dir).exists() {
            generate_big_demo(&export)?;
        }
        import = Some(export);
    }
    if let Some(export) = &import {
        let dir = data_dir.as_ref().ok_or("--import needs --data-dir (the folder of the new database)")?;
        if db::database_path(dir).exists() {
            eprintln!("{} already has a database: serving it as it is", dir.display());
        } else {
            let r = karbon_server::import::import_export(export, dir, &Default::default())?;
            eprintln!(
                "imported {} rows in {} ms ({} rejected, {} changed); report: {}",
                r.loaded(),
                r.elapsed_ms,
                r.rejected.len(),
                r.changed.len(),
                r.report_path.display()
            );
        }
        fresh = true; // an imported database never takes the demo seed
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

#[cfg(feature = "big-demo")]
fn generate_big_demo(export: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let s = karbon_synth::generate(karbon_synth::Config::default(), export)?;
    eprintln!("generated the synthetic export: {} rows in {}", s.total_rows(), export.display());
    Ok(())
}

#[cfg(not(feature = "big-demo"))]
fn generate_big_demo(_: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    Err("--big-demo needs a build with --features big-demo".into())
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
