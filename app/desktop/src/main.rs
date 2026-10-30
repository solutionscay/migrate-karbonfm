// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // The database lives in the per-user app data folder (D-17).
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db = karbon_server::db::open_demo(Some(&data_dir.join("karbonfm-demo.db")))?;

            // The local server: 127.0.0.1 on a free port, inside this process.
            let (listener, addr) = tauri::async_runtime::block_on(karbon_server::bind())?;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = karbon_server::serve(listener, karbon_server::AppState::new(db)).await {
                    eprintln!("local server stopped: {e}");
                }
            });

            // One window on the server URL. The window has no capabilities, so the page cannot call Tauri IPC.
            let url = format!("http://{addr}/").parse()?;
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
