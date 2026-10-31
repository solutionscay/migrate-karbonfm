// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(go_menu::build).on_menu_event(go_menu::handle);
    builder
        .setup(|app| {
            // The database lives in the per-user app data folder (D-17).
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db = karbon_server::db::open_demo(Some(&data_dir.join("karbonfm-demo.db")))?;

            // The local server: 127.0.0.1 on a free port, inside this process. Its print route opens the
            // window's native print dialog (window.print() does nothing in WKWebView on macOS).
            let handle = app.handle().clone();
            let print: karbon_server::PrintHook = Arc::new(move || {
                let window = handle.get_webview_window("main").ok_or("no main window")?;
                window.print().map_err(|e| e.to_string())
            });
            let handle = app.handle().clone();
            let save: karbon_server::SaveHook = Arc::new(move |name, bytes| save_file(&handle, name, bytes));
            let state = karbon_server::AppState::new(db).with_print(print).with_save(save);
            let (listener, addr) = tauri::async_runtime::block_on(karbon_server::bind())?;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = karbon_server::serve(listener, state).await {
                    eprintln!("local server stopped: {e}");
                }
            });

            // One window on the server URL. The window has no capabilities, so the page cannot call Tauri IPC.
            let url: tauri::Url = format!("http://{addr}/").parse()?;
            app.manage(Home(url.clone()));
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

/// The app's start page on the local server (the Go > Home menu item navigates back to it).
struct Home(tauri::Url);

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
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())?;
    Ok(Some(path))
}

/// macOS menu bar: the standard menus plus Go > Back (Cmd+[) and Home (Shift+Cmd+H), so the window can always get
/// back to the app, even from a page that is not the app's (a file shown full-window has no sidebar or back button).
/// Windows and Linux keep no menu bar: WebView2 has Alt+Left built in, and WebKitGTK cannot show a PDF at all.
#[cfg(target_os = "macos")]
mod go_menu {
    use super::Home;
    use tauri::menu::{Menu, MenuEvent, MenuItem, Submenu};
    use tauri::{AppHandle, Manager};

    pub fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
        let menu = Menu::default(app)?;
        let back = MenuItem::with_id(app, "go-back", "Back", true, Some("CmdOrCtrl+["))?;
        let home = MenuItem::with_id(app, "go-home", "Home", true, Some("CmdOrCtrl+Shift+H"))?;
        // After View, where Safari puts History: App, File, Edit, View, Go, Window, Help.
        menu.insert(&Submenu::with_items(app, "Go", true, &[&back, &home])?, 4)?;
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
