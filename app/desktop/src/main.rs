// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use karbon_server::applog;
use karbon_server::db::Seed;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(history_menu::build).on_menu_event(history_menu::handle);
    builder
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
            // the demo builds filled with demo data when it is new (03, "Demo seed data").
            let data_dir = match &override_dir {
                Some(dir) => dir.clone(),
                None => app.path().app_data_dir()?,
            };
            let db_path = karbon_server::db::database_path(&data_dir);
            // The shipped build: a folder with no database gets the big demo (under a second in a release build,
            // before the window opens). An existing database is left as it is.
            #[cfg(feature = "big-demo")]
            {
                let outcome = karbon_server::big_demo::create(&data_dir).inspect_err(|e| {
                    applog::log_write(&applog::Entry::error("start", "Storage", &e.to_string()));
                })?;
                applog::log_write(&applog::Entry::info("start", &format!("big demo: {outcome:?}")));
            }
            let seed = if cfg!(feature = "demo") && !cfg!(feature = "big-demo") {
                Seed::DemoAsOf(karbon_server::demo_dates::local_today())
            } else {
                Seed::Reference
            };
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

            // The local server: 127.0.0.1 on a free port, inside this process.
            let (listener, addr) = tauri::async_runtime::block_on(karbon_server::bind())?;
            // AP-13: the window opens the one-time launch URL, which gives it the session cookie; any other
            // client of the port gets 401.
            let session = Arc::new(karbon_server::session::Session::new(addr.port()));
            let launch = session.launch_path().ok_or("session already used")?;
            // Secrets live in the OS keychain (AP-03); the settings page reads and writes them off the async workers.
            // Without a keychain (Linux without a Secret Service) this logs why once, and Settings offers the opt-in
            // secrets.toml (D-09, issue #7).
            let secrets = karbon_server::secrets::system_store();
            // Backup and restore (OP-ADMIN-06, OP-ADMIN-07): the server asks for a folder, the shell shows the native
            // dialog. The page itself never gets a path or file-system access (AP-01).
            let handle = app.handle().clone();
            let folder: karbon_server::FolderHook = Arc::new(move |what| pick_folder(&handle, what));
            // Saving a file (the invoice PDF): the native save dialog, then the system viewer (fix/pdf-view).
            let handle = app.handle().clone();
            let save: karbon_server::SaveHook = Arc::new(move |name, bytes| save_file(&handle, name, bytes));
            // Opening a stored file (UI-19): the server resolves the path inside the file store, the shell hands it
            // to the system app. The page only names a file id (AP-01).
            let open: karbon_server::OpenHook = Arc::new(|path| open_in_viewer(&path));
            let state = karbon_server::AppState::new(db)
                .with_save(save)
                .with_open(open)
                .with_session(session)
                .with_secrets(secrets)
                .with_folder_picker(folder)
                .with_data_dir(&data_dir);
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
            #[cfg(target_os = "macos")]
            app.manage(history_menu::Home(format!("http://{addr}/").parse()?));
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

/// The server's folder hook: the native folder dialog on the window, shown on the main thread (rfd needs it there);
/// the server's blocking task waits for the answer. `None` when the user cancels.
fn pick_folder(handle: &AppHandle, what: karbon_server::FolderPick) -> Result<Option<PathBuf>, String> {
    let window = handle.get_webview_window("main").ok_or("no main window")?;
    let title = match what {
        karbon_server::FolderPick::BackupDestination => "Choose where to save the backup",
        karbon_server::FolderPick::RestoreSource => "Choose the backup folder to restore",
    };
    let (tx, rx) = mpsc::channel();
    handle
        .run_on_main_thread(move || {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title(title)
                .set_can_create_directories(true)
                .set_parent(&window)
                .pick_folder();
            std::thread::spawn(move || tx.send(tauri::async_runtime::block_on(dialog)));
        })
        .map_err(|e| e.to_string())?;
    let picked = rx.recv().map_err(|e| e.to_string())?;
    Ok(picked.map(|f| f.path().to_path_buf()))
}

/// The server's save hook: the native save dialog on the window (as tauri-plugin-dialog shows it, but without
/// registering that plugin, whose page script reroutes alert() and confirm() through IPC the page does not have),
/// then write the file and open it in the system viewer (WebKitGTK cannot show a PDF in the window).
fn save_file(handle: &AppHandle, name: String, bytes: Vec<u8>) -> Result<Option<PathBuf>, String> {
    let window = handle.get_webview_window("main").ok_or("no main window")?;
    let (tx, rx) = mpsc::channel();
    // rfd creates the dialog on the main thread; the server thread waits for the answer.
    handle
        .run_on_main_thread(move || {
            let dialog = rfd::AsyncFileDialog::new()
                .set_file_name(name)
                .add_filter("PDF", &["pdf"])
                .set_can_create_directories(true)
                .set_parent(&window)
                .save_file();
            std::thread::spawn(move || tx.send(tauri::async_runtime::block_on(dialog)));
        })
        .map_err(|e| e.to_string())?;
    let Some(file) = rx.recv().map_err(|e| e.to_string())? else { return Ok(None) };
    let path = file.path().to_path_buf();
    std::fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    open_in_viewer(&path)?;
    Ok(Some(path))
}

/// Opens a file the user just saved, or a stored file (UI-19), in the system's default app (Preview, the Windows default, xdg-open on Linux).
/// A direct process call instead of tauri-plugin-opener, so the shell links no Tauri plugin at all (T-08-01). The path
/// comes from the native save dialog or the server's file store, never from the page, and goes to the program as one argument (no shell).
fn open_in_viewer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let mut child = std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map_err(|e| format!("could not open {} with {program}: {e}", path.display()))?;
    // Reap the helper when it exits (explorer.exe's exit code says nothing about success, so it is not checked).
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// macOS menu bar: the standard menus plus History > Back (Cmd+[) and Home (Shift+Cmd+H), so the window can always get
/// back to the app, even from a page that is not the app's (a file shown full-window has no sidebar or back button).
/// Windows and Linux keep no menu bar: WebView2 has Alt+Left built in, and WebKitGTK cannot show a PDF at all.
#[cfg(target_os = "macos")]
mod history_menu {
    use tauri::menu::{Menu, MenuEvent, MenuItem, Submenu};
    use tauri::{AppHandle, Manager};

    /// The app's start page on the local server (the History > Home menu item navigates back to it).
    pub struct Home(pub tauri::Url);

    pub fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
        let menu = Menu::default(app)?;
        let back = MenuItem::with_id(app, "go-back", "Back", true, Some("CmdOrCtrl+["))?;
        let home = MenuItem::with_id(app, "go-home", "Home", true, Some("CmdOrCtrl+Shift+H"))?;
        // After View, as in Safari. Not "Go": macOS 26 left a top-level menu with that title out of the menu bar.
        menu.insert(&Submenu::with_items(app, "History", true, &[&back, &home])?, 4)?;
        Ok(menu)
    }

    pub fn handle(app: &AppHandle, event: MenuEvent) {
        let Some(window) = app.get_webview_window("main") else { return };
        let result = match event.id().as_ref() {
            "go-back" => window.eval("history.back()"),
            "go-home" => window.navigate(app.state::<Home>().0.clone()),
            _ => return,
        };
        if let Err(e) = result {
            eprintln!("menu {}: {e}", event.id().as_ref());
        }
    }
}
