// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let (listener, addr) = tauri::async_runtime::block_on(karbon_server::bind())?;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = karbon_server::serve(listener).await {
                    eprintln!("local server stopped: {e}");
                }
            });
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
