// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use karbon_server::applog;
use karbon_server::db::Seed;
use std::sync::Arc;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // KARBONFM_DATA_DIR (development and checks only) puts the database and the log in another folder, so
            // a fresh run does not touch the user's karbonfm.db.
            let override_dir = std::env::var_os("KARBONFM_DATA_DIR").map(std::path::PathBuf::from);

            // The log file in the per-user app log folder (D-30), before anything that can fail.
            let log_dir = match &override_dir {
                Some(dir) => dir.join("logs"),
                None => app.path().app_log_dir()?,
            };
            let log_dir = karbon_server::applog::init(log_dir)?;

            // One database in the per-user app data folder (D-17): migrated by refinery, reference-seeded, and in
            // the demo build demo-seeded while it has no party (03, "Demo seed data").
            let data_dir = match &override_dir {
                Some(dir) => dir.clone(),
                None => app.path().app_data_dir()?,
            };
            let db_path = karbon_server::db::database_path(&data_dir);
            let seed = if cfg!(feature = "demo") { Seed::Demo } else { Seed::Reference };
            let (db, report) = karbon_server::db::open(&db_path, seed).inspect_err(|e| {
                applog::log_write(&applog::Entry::error("start", "Storage", &e.to_string()));
            })?;
            let started = format!(
                "database {} (migrations applied: {:?}; reference rows: {}; demo seed loaded: {}); log {}",
                db_path.display(),
                report.migrations_applied,
                report.reference_rows_inserted,
                report.demo_seed_loaded,
                log_dir.display()
            );
            applog::log_write(&applog::Entry::info("start", &started));

            // The local server: 127.0.0.1 on a free port, inside this process. Its print route opens the
            // window's native print dialog (window.print() does nothing in WKWebView on macOS).
            let handle = app.handle().clone();
            let print: karbon_server::PrintHook = Arc::new(move || {
                let window = handle.get_webview_window("main").ok_or("no main window")?;
                window.print().map_err(|e| e.to_string())
            });
            let (listener, addr) = tauri::async_runtime::block_on(karbon_server::bind())?;
            // AP-13: the window opens the one-time launch URL, which gives it the session cookie; any other
            // client of the port gets 401.
            let session = Arc::new(karbon_server::session::Session::new(addr.port()));
            let launch = session.launch_path().ok_or("session already used")?;
            // Secrets live in the OS keychain (AP-03); the settings page reads and writes them off the async workers.
            let secrets = Arc::new(karbon_server::secrets::Keychain::new());
            let state = karbon_server::AppState::new(db).with_print(print).with_session(session).with_secrets(secrets);
            tauri::async_runtime::spawn(async move {
                if let Err(e) = karbon_server::serve(listener, state).await {
                    applog::log_write(&applog::Entry::error(
                        "serve",
                        "Internal",
                        &format!("local server stopped: {e}"),
                    ));
                }
            });

            // One window on the server URL. The window has no capabilities, so the page cannot call Tauri IPC.
            let url = format!("http://{addr}{launch}").parse()?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("KarbonFM")
                .inner_size(1440.0, 900.0)
                .min_inner_size(1024.0, 640.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running KarbonFM");
}
