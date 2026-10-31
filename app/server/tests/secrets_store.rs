//! Secrets on a computer without a keychain (D-09 as changed 2026-10-31, issue #7), through the router with the full
//! AP-13 guard on. The keychain side is a test double: `NoKeychain` (Linux without a Secret Service) or `MemoryStore`
//! (a running keychain). Every app has its own temp data folder: no test touches the real karbonfm.db or keychain.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::db::{self, Seed};
use karbon_server::secrets::{
    secrets_path, FileStore, MemoryStore, NoKeychain, Secret, SecretKey, SecretStore, SECRETS_FILE,
};
use karbon_server::session::Session;
use karbon_server::{backup, router, AppState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:4324";
const ORIGIN: &str = "http://127.0.0.1:4324";
const PASSWORD: &str = "Fi1e-sm7p-pa55-4417";
const NOTICE: &str =
    "KarbonFM keeps passwords and API keys in your system keychain, and this computer doesn't have one running.";
const DISABLED: &str = "Needs a keychain or the file option";

struct App {
    router: axum::Router,
    cookie: String,
    data: PathBuf,
}

/// A guarded router on the demo database in `data`, past the launch URL (as the window is).
async fn start(data: &Path, keychain: Arc<dyn SecretStore>) -> App {
    let (conn, _) = db::open(&db::database_path(data), Seed::Demo).unwrap();
    let session = Arc::new(Session::new(4324));
    let launch = session.launch_path().unwrap();
    let state =
        AppState::new(conn).with_origin(ORIGIN).with_session(session).with_secrets(keychain).with_data_dir(data);
    let router = router(state);
    let (status, headers, _) = send(&router, Request::get(&launch).header(header::HOST, HOST)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
    App { router, cookie, data: data.to_owned() }
}

fn no_keychain() -> Arc<dyn SecretStore> {
    Arc::new(NoKeychain::new("test double: no Secret Service on the session bus"))
}

async fn send(router: &axum::Router, req: axum::http::request::Builder) -> (StatusCode, HeaderMap, String) {
    let res = router.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

impl App {
    async fn get(&self, uri: &str) -> (StatusCode, HeaderMap, String) {
        send(&self.router, Request::get(uri).header(header::HOST, HOST).header(header::COOKIE, &self.cookie)).await
    }
    async fn write(&self, method: &str, uri: &str, body: &str) -> (StatusCode, HeaderMap, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("HX-Request", "true");
        let res = self.router.clone().oneshot(req.body(Body::from(body.to_owned())).unwrap()).await.unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }
    fn file(&self) -> PathBuf {
        secrets_path(&self.data)
    }
    fn tax_rate_bp(&self) -> i64 {
        rusqlite::Connection::open(db::database_path(&self.data))
            .unwrap()
            .query_row("SELECT tax_rate_bp FROM system_setting WHERE singleton = 1", [], |r| r.get(0))
            .unwrap()
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The settings form as the page posts it, with `changes`.
fn settings_body(changes: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(&str, &str)> = vec![
        ("owner_id", "p-ellison"),
        ("tax_rate", "5.5"),
        ("default_tax_code", ""),
        ("country_code", "1"),
        ("area_code", "207"),
        ("format_phone_numbers", "on"),
        ("email_address", ""),
        ("email_full_name", ""),
        ("email_user_name", ""),
        ("smtp_server", ""),
        ("smtp_port", ""),
        ("smtp_encryption", "STARTTLS"),
        ("smtp_authentication", "Password"),
        ("system_smtp_password", ""),
        ("google_places", ""),
        ("smartystreets", ""),
        ("fullcontact", ""),
    ];
    for (k, v) in changes {
        pairs.iter_mut().find(|(name, _)| name == k).expect("a settings field").1 = v;
    }
    pairs.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&")
}

/// The `<input>` tag of a secret field.
fn input_tag<'a>(html: &'a str, name: &str) -> &'a str {
    let at = html.find(&format!(r#"name="{name}""#)).unwrap_or_else(|| panic!("no {name} field"));
    let start = html[..at].rfind("<input").unwrap();
    &html[start..start + html[start..].find('>').unwrap()]
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn settings_without_a_keychain_opens_with_the_notice_and_disabled_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let app = start(tmp.path(), no_keychain()).await;
    let (status, _, html) = app.get("/settings").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("No system keychain found.") && html.contains(NOTICE));
    assert!(html.contains("install GNOME Keyring or KWallet, then restart KarbonFM"));
    assert!(html.contains("any program running as your user can read it"));
    assert!(html.contains("email and the integrations stay off"));
    assert!(html.contains(r#"hx-get="/settings/secrets/file""#) && html.contains("Store secrets in a file instead"));
    // The button sits inside the settings form: it must not send the form (a typed secret goes only to PUT).
    let button = &html[html.find(r#"hx-get="/settings/secrets/file""#).unwrap()..];
    assert!(button[..button.find('>').unwrap()].contains(r#"hx-params="none""#));
    // The notice is above the secrets panels.
    assert!(html.find(NOTICE).unwrap() < html.find("System email account").unwrap());
    for name in ["system_smtp_password", "google_places", "smartystreets", "fullcontact"] {
        let tag = input_tag(&html, name);
        assert!(tag.contains("disabled") && tag.contains(DISABLED), "{tag}");
    }
    assert!(!html.contains("Saved. Type a new value"), "nothing can look saved");
    assert!(html.contains("Keys need a system keychain or the file option."));
    assert!(!app.file().exists(), "opening Settings creates no file");
}

#[tokio::test]
async fn put_without_a_keychain_saves_the_rest_and_refuses_a_typed_secret() {
    let tmp = tempfile::tempdir().unwrap();
    let app = start(tmp.path(), no_keychain()).await;

    // No secret typed (the fields are disabled, so the page sends none): the settings save as usual.
    let (status, headers, html) = app.write("PUT", "/settings", &settings_body(&[("tax_rate", "6.25")])).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("Settings saved."));
    assert!(headers.get("HX-Trigger").is_some_and(|v| v.to_str().unwrap().contains("Settings saved")));
    assert_eq!(app.tax_rate_bp(), 625);

    // A secret typed anyway: a field error (422), nothing saved, no "saved" anywhere.
    let body = settings_body(&[("tax_rate", "7"), ("system_smtp_password", PASSWORD)]);
    let (status, headers, html) = app.write("PUT", "/settings", &body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{html}");
    assert!(html.contains("Not saved: this computer has no system keychain."));
    assert!(html.contains("Check the fields marked below. Nothing was saved."));
    assert!(!html.contains("Settings saved") && !html.contains("Saved. Type a new value"));
    assert!(headers.get("HX-Trigger").is_none(), "no toast");
    assert!(!html.contains(PASSWORD), "a typed secret is never echoed");
    assert!(input_tag(&html, "system_smtp_password").contains(r#"aria-invalid="true""#));
    assert_eq!(app.tax_rate_bp(), 625, "the settings row was not saved either");
    assert!(!app.file().exists(), "no silent fallback to a file");
}

#[tokio::test]
async fn the_file_option_asks_first_in_an_in_page_dialog() {
    let tmp = tempfile::tempdir().unwrap();
    let app = start(tmp.path(), no_keychain()).await;
    let (status, _, html) = app.get("/settings/secrets/file").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    // A body for the shared <dialog id="modal">: an htmx form, no native confirm().
    assert!(html.contains(r#"id="modal-title""#) && html.contains("Store secrets in a file?"));
    assert!(html.contains(r#"hx-post="/settings/secrets/file""#) && html.contains("Yes, store them in a file"));
    assert!(
        html.contains("less protected than a keychain")
            && html.contains("any program running as your user can read it")
    );
    assert!(html.contains("never put in a backup"));
    assert!(!html.contains("<script") && !html.contains("confirm("));
    assert!(!app.file().exists(), "asking creates nothing");
}

#[tokio::test]
async fn opting_in_creates_a_private_file_and_the_fields_work_write_only() {
    let tmp = tempfile::tempdir().unwrap();
    let app = start(tmp.path(), no_keychain()).await;
    let (status, headers, _) = app.write("POST", "/settings/secrets/file", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["HX-Redirect"], "/settings");
    assert!(app.file().is_file());
    #[cfg(unix)]
    assert_eq!(mode(&app.file()), 0o600, "owner read/write only");

    let (status, _, html) = app.get("/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Secrets:</span> local file (no system keychain)"));
    assert!(!html.contains(NOTICE) && !html.contains("Store secrets in a file instead"));
    assert!(!html.contains("Move secrets to the system keychain"), "no keychain to move to yet");
    let tag = input_tag(&html, "system_smtp_password");
    assert!(!tag.contains("disabled") && tag.contains("Not set"), "{tag}");
    assert!(html.contains("secrets file on this computer."));

    // Write-only, as with the keychain.
    let (status, _, html) = app.write("PUT", "/settings", &settings_body(&[("system_smtp_password", PASSWORD)])).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("Settings saved.") && !html.contains(PASSWORD));
    let store = FileStore::open(&app.file()).unwrap();
    assert_eq!(store.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), PASSWORD);
    #[cfg(unix)]
    assert_eq!(mode(&app.file()), 0o600, "still 0600 after a write");
    let (_, _, html) = app.get("/settings").await;
    let tag = input_tag(&html, "system_smtp_password");
    assert!(tag.contains(r#"value="""#) && tag.contains("Saved. Type a new value to replace it."), "{tag}");
    assert!(!html.contains(PASSWORD));
    // An empty field keeps it.
    let (status, _, _) = app.write("PUT", "/settings", &settings_body(&[])).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(store.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), PASSWORD);
    // SQLite has no copy.
    assert_eq!(hits_in_sqlite(&app.data, PASSWORD), 0, "the password is in SQLite");
    // Opting in again is refused (already in the file) and changes nothing.
    let (status, _, html) = app.write("POST", "/settings/secrets/file", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("already kept in the file"));
    assert_eq!(store.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), PASSWORD);
}

#[tokio::test]
async fn with_a_keychain_nothing_changes_and_the_file_option_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let app = start(tmp.path(), Arc::new(MemoryStore::default())).await;
    let (_, _, html) = app.get("/settings").await;
    assert!(!html.contains("No system keychain found.") && !html.contains("local file"));
    assert!(!input_tag(&html, "system_smtp_password").contains("disabled"));
    assert!(html.contains("Keys are kept in the system keychain."));
    let (status, _, html) = app.get("/settings/secrets/file").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("A system keychain is running") && !html.contains("Yes, store them in a file"));
    let (status, _, _) = app.write("POST", "/settings/secrets/file", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!app.file().exists());
    let (status, _, html) = app.write("POST", "/settings/secrets/keychain", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Secrets are not kept in a file."));
}

#[tokio::test]
async fn switching_back_moves_the_secrets_to_the_keychain_and_removes_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let app = start(tmp.path(), no_keychain()).await;
    app.write("POST", "/settings/secrets/file", "").await;
    app.write("PUT", "/settings", &settings_body(&[("system_smtp_password", PASSWORD), ("fullcontact", "fc-9")])).await;

    // Still no keychain: the switch is refused and the file stays.
    let (status, _, html) = app.write("POST", "/settings/secrets/keychain", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains(r#"id="secrets-store""#) && html.contains("No system keychain is running."));
    assert!(app.file().is_file());

    // The next start, with GNOME Keyring installed: the file is still the store until the user moves the secrets.
    let keychain = Arc::new(MemoryStore::default());
    let later = start(tmp.path(), keychain.clone()).await;
    let (_, _, html) = later.get("/settings").await;
    assert!(html.contains("local file (no system keychain)"));
    assert!(
        html.contains(r#"hx-post="/settings/secrets/keychain""#)
            && html.contains("Move secrets to the system keychain")
    );
    assert!(input_tag(&html, "system_smtp_password").contains("Saved. Type a new value"));
    let (status, headers, _) = later.write("POST", "/settings/secrets/keychain", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["HX-Redirect"], "/settings");
    assert!(!later.file().exists(), "the file is removed");
    assert_eq!(keychain.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), PASSWORD);
    assert_eq!(keychain.get(SecretKey::FullContactApiKey).unwrap().unwrap().expose(), "fc-9");
    let (_, _, html) = later.get("/settings").await;
    assert!(!html.contains("local file") && html.contains("Keys are kept in the system keychain."));
    assert!(input_tag(&html, "system_smtp_password").contains("Saved. Type a new value"));
}

/// How many cells of any table of the database in `data` contain `text`.
fn hits_in_sqlite(data: &Path, text: &str) -> i64 {
    let db = rusqlite::Connection::open(db::database_path(data)).unwrap();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut hits = 0;
    for table in tables {
        let cols: Vec<String> = db
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for col in cols {
            let sql = format!("SELECT COUNT(*) FROM \"{table}\" WHERE CAST(\"{col}\" AS TEXT) LIKE '%' || ?1 || '%'");
            hits += db.query_row(&sql, [text], |r| r.get::<_, i64>(0)).unwrap();
        }
    }
    hits
}

/// Every file under `dir`, at any depth.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn a_backup_never_holds_the_secrets_file_and_a_restore_keeps_it() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let (mut conn, _) = db::open(&db::database_path(&data), Seed::Demo).unwrap();
    let files = backup::files_dir(&data);
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("note.txt"), b"an attachment").unwrap();
    let file = FileStore::create(&secrets_path(&data)).unwrap();
    file.set(SecretKey::SystemSmtpPassword, &Secret::new(PASSWORD)).unwrap();

    let parent = tmp.path().join("Backups");
    std::fs::create_dir_all(&parent).unwrap();
    let made = backup::create(&conn, &files, &parent).unwrap();
    for path in files_under(&made.dir) {
        assert_ne!(path.file_name().unwrap(), SECRETS_FILE, "{}", path.display());
        let bytes = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(PASSWORD), "{} holds the password", path.display());
    }

    // The machine's secrets change after the backup; a restore must not take them back or remove them.
    file.set(SecretKey::SystemSmtpPassword, &Secret::new("newer-password")).unwrap();
    let before = std::fs::read(secrets_path(&data)).unwrap();
    backup::restore(&mut conn, &data, &made.dir).unwrap();
    assert_eq!(std::fs::read(secrets_path(&data)).unwrap(), before, "secrets.toml is untouched");
    let after = FileStore::open(&secrets_path(&data)).unwrap();
    assert_eq!(after.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), "newer-password");
    #[cfg(unix)]
    assert_eq!(mode(&secrets_path(&data)), 0o600);
}
