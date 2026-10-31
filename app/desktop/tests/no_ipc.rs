//! The window's side of T-08-01 and T-08-10 (AP-01, AP-02, AP-10): the page gets no Tauri IPC, no plugin and no URL
//! scheme. Checked on the files that decide it: the Tauri config, the shell's source, its manifest and the lock file.
//! S-01 checked the result in the running window (`__TAURI_INTERNALS__.invoke` rejected).

const CONF: &str = include_str!("../tauri.conf.json");
const MAIN: &str = include_str!("../src/main.rs");
const MANIFEST: &str = include_str!("../Cargo.toml");
const LOCK: &str = include_str!("../../Cargo.lock");

/// T-08-01: no IPC permission is granted to the window, and no command handler exists.
#[test]
fn t_08_01_window_has_no_ipc() {
    assert!(CONF.contains(r#""capabilities": []"#), "tauri.conf.json grants a capability");
    assert!(CONF.contains(r#""withGlobalTauri": false"#));
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities");
    assert!(!std::path::Path::new(dir).exists(), "a capabilities/ folder would grant permissions to the window");
    for call in ["invoke_handler", "generate_handler", ".plugin("] {
        assert!(!MAIN.contains(call), "main.rs uses {call}");
    }
}

/// T-08-01 / AP-02: no sql, fs, shell, http or other plugin is linked.
#[test]
fn t_08_01_no_tauri_plugin() {
    assert!(!MANIFEST.contains("tauri-plugin"), "desktop/Cargo.toml adds a Tauri plugin");
    let plugins: Vec<&str> = LOCK
        .lines()
        .filter_map(|l| l.strip_prefix("name = \""))
        .map(|n| n.trim_end_matches('"'))
        .filter(|n| n.starts_with("tauri-plugin"))
        .collect();
    assert!(plugins.is_empty(), "Tauri plugins in Cargo.lock: {plugins:?}");
    assert!(!CONF.contains(r#""plugins""#), "tauri.conf.json configures a plugin");
}

/// T-08-10: no URL scheme handler or deep link (the fmp:// Commander is not migrated), and the shell starts the
/// server through `karbon_server::bind`, which binds 127.0.0.1 only.
#[test]
fn t_08_10_no_url_scheme_and_loopback_only() {
    for needle in
        ["deep-link", "deepLink", "CFBundleURLTypes", "register_uri_scheme_protocol", "register_asynchronous_uri"]
    {
        assert!(!CONF.contains(needle) && !MAIN.contains(needle), "{needle} found");
    }
    assert!(!std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Info.plist")).exists());
    assert!(MAIN.contains("karbon_server::bind()"), "the shell binds through karbon_server::bind (loopback)");
    assert!(!MAIN.contains("0.0.0.0") && !MAIN.contains("UNSPECIFIED"));
    // The window opens the one-time launch URL of AP-13, not the bare server URL.
    assert!(MAIN.contains("launch_path()"));
}
