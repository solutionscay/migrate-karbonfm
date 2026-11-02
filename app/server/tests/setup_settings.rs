//! UI-01 setup (OP-ADMIN-01, BR-ADMIN-01), UI-17 settings (OP-ADMIN-03, BR-ADMIN-05, AP-03 / T-08-03) and
//! OP-ADMIN-04 current_user (BR-ADMIN-04), through the router with the full AP-13 guard on.
//!
//! Each test uses a database file in a temp folder: the router owns one connection, the test reads with a second
//! one (WAL), as another process would.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::admin::{self, CommandError, SetupInput};
use karbon_server::audit::{self, Actor};
use karbon_server::db::{self, Seed};
use karbon_server::secrets::{MemoryStore, SecretKey, SecretStore};
use karbon_server::session::Session;
use karbon_server::{router, AppState};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:4322";
const ORIGIN: &str = "http://127.0.0.1:4322";

struct App {
    router: axum::Router,
    cookie: String,
    store: Arc<MemoryStore>,
    path: PathBuf,
    _dir: tempfile::TempDir,
}

/// A guarded router on a new database file, already past the launch URL (as the window is).
async fn start(seed: Seed) -> App {
    let dir = tempfile::tempdir().unwrap();
    let path = db::database_path(dir.path());
    let (conn, _) = db::open(&path, seed).unwrap();
    let session = Arc::new(Session::new(4322));
    let launch = session.launch_path().unwrap();
    let store = Arc::new(MemoryStore::default());
    let state = AppState::new(conn).with_origin(ORIGIN).with_session(session).with_secrets(store.clone());
    let router = router(state);
    let (status, headers, _) = send(&router, get_req(&launch, None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
    App { router, cookie, store, path, _dir: dir }
}

async fn send(router: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

fn get_req(uri: &str, cookie: Option<&str>) -> Request<Body> {
    let mut r = Request::get(uri).header(header::HOST, HOST);
    if let Some(c) = cookie {
        r = r.header(header::COOKIE, c);
    }
    r.body(Body::empty()).unwrap()
}

fn form_req(method: &str, uri: &str, cookie: Option<&str>, origin: &str, body: &str) -> Request<Body> {
    let mut r = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("HX-Request", "true");
    if let Some(c) = cookie {
        r = r.header(header::COOKIE, c);
    }
    r.body(Body::from(body.to_owned())).unwrap()
}

impl App {
    async fn get(&self, uri: &str) -> (StatusCode, HeaderMap, String) {
        send(&self.router, get_req(uri, Some(&self.cookie))).await
    }
    async fn htmx_get(&self, uri: &str) -> (StatusCode, HeaderMap, String) {
        let mut req = get_req(uri, Some(&self.cookie));
        req.headers_mut().insert("HX-Request", "true".parse().unwrap());
        send(&self.router, req).await
    }
    async fn post(&self, uri: &str, body: &str) -> (StatusCode, HeaderMap, String) {
        send(&self.router, form_req("POST", uri, Some(&self.cookie), ORIGIN, body)).await
    }
    async fn put(&self, uri: &str, body: &str) -> (StatusCode, HeaderMap, String) {
        send(&self.router, form_req("PUT", uri, Some(&self.cookie), ORIGIN, body)).await
    }
    /// A second connection on the app's database file.
    fn db(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }
    fn count(&self, sql: &str) -> i64 {
        self.db().query_row(sql, [], |r| r.get(0)).unwrap()
    }
    fn text(&self, sql: &str) -> String {
        self.db().query_row(sql, [], |r| r.get(0)).unwrap()
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

fn body(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&")
}

const ORG: (&str, &str) = ("organization_name", "Harbor & Pine Design");
const PERSON: [(&str, &str); 4] = [
    ("first_name", "Rosa"),
    ("last_name", "Lindqvist"),
    ("job_title", "Principal"),
    ("email", "rosa@harborpine.example"),
];
const SETTINGS: [(&str, &str); 4] =
    [("tax_rate", "5.5"), ("country_code", "1"), ("area_code", "207"), ("format_phone_numbers", "on")];

/// The last step's post, with every field.
fn finish() -> String {
    let mut pairs = vec![("step", "settings"), ORG];
    pairs.extend(PERSON);
    pairs.extend(SETTINGS);
    body(&pairs)
}

// ---------------------------------------------------------------------------------------------------------------
// Setup gate
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_fresh_database_sends_every_page_to_setup() {
    let app = start(Seed::Reference).await;
    for uri in ["/", "/customers", "/sales/INV-1048", "/settings", "/sales/INV-1048/invoice.pdf", "/nowhere"] {
        let (status, headers, _) = app.get(uri).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(headers[header::LOCATION], "/setup", "{uri}");
    }
    let (status, headers, _) = app.htmx_get("/customers?page=2").await;
    assert_eq!((status, headers["HX-Redirect"].to_str().unwrap()), (StatusCode::OK, "/setup"), "htmx: HX-Redirect");
    let (status, headers, _) = app.post("/customers/new", "name=Acme&email=").await;
    assert_eq!((status, headers["HX-Redirect"].to_str().unwrap()), (StatusCode::OK, "/setup"));
    let (status, _, html) = app.get("/setup").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>") && html.contains("Set up KarbonFM") && html.contains("Your business"));
    assert!(!html.contains("class=\"sidebar\""), "no app navigation before setup");
    let (status, _, _) = app.get("/assets/app.css").await;
    assert_eq!(status, StatusCode::OK, "static files are not gated");
}

#[tokio::test]
async fn a_demo_database_counts_as_set_up() {
    let app = start(Seed::Demo).await;
    let (status, _, html) = app.get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Mara"));
    let (status, headers, _) = app.get("/setup").await;
    assert_eq!((status, headers[header::LOCATION].to_str().unwrap()), (StatusCode::SEE_OTHER, "/"));
    let parties = app.count("SELECT COUNT(*) FROM party");
    let (status, headers, _) = app.post("/setup", &finish()).await;
    assert_eq!((status, headers["HX-Redirect"].to_str().unwrap()), (StatusCode::OK, "/"));
    assert_eq!(app.count("SELECT COUNT(*) FROM party"), parties, "setup never runs on a set-up database");
}

#[tokio::test]
async fn the_guard_still_applies_to_setup_and_settings() {
    let app = start(Seed::Reference).await;
    // No cookie: 401 before the setup gate is asked.
    for uri in ["/setup", "/settings", "/"] {
        let (status, _, _) = send(&app.router, get_req(uri, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    // A cross-site POST of the setup form, with the cookie: 403, nothing written.
    let (status, _, _) =
        send(&app.router, form_req("POST", "/setup", Some(&app.cookie), "https://evil.example", &finish())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(app.count("SELECT COUNT(*) FROM party"), 0);
    // Foreign Host: 421.
    let req = Request::get("/settings").header(header::HOST, "evil.example").header(header::COOKIE, &app.cookie);
    let (status, _, _) = send(&app.router, req.body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    // A PUT of the settings from another site, on a set-up database: 403, nothing changed.
    let demo = start(Seed::Demo).await;
    let (status, _, _) = send(
        &demo.router,
        form_req("PUT", "/settings", Some(&demo.cookie), "https://evil.example", &settings_body(&[("tax_rate", "1")])),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(demo.count("SELECT tax_rate_bp FROM system_setting"), 550);
}

// ---------------------------------------------------------------------------------------------------------------
// BR-ADMIN-01 setup
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn setup_walks_the_three_steps_and_creates_the_owner_person_and_user() {
    let app = start(Seed::Reference).await;

    // Step 1: the name is required.
    let (status, _, html) = app.post("/setup", "step=organization&organization_name=+").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter the name of your business.") && html.contains("aria-invalid=\"true\""));
    let (status, _, html) = app.post("/setup", &body(&[("step", "organization"), ORG])).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("First name"));
    assert!(html.contains(r#"type="hidden" name="organization_name" value="Harbor &#38; Pine Design""#), "{html}");

    // Step 2: last name missing and a bad email.
    let (status, _, html) = app
        .post(
            "/setup",
            &body(&[("step", "person"), ORG, ("first_name", "Rosa"), ("last_name", ""), ("email", "rosa@")]),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter your last name.") && html.contains("Enter an email address"));
    // Back keeps what was typed and shows step 1.
    let (_, _, html) =
        app.post("/setup", &body(&[("step", "person"), ("go", "back"), ORG, ("first_name", "Rosa")])).await;
    assert!(
        html.contains("Your business") && html.contains(r#"type="hidden" name="first_name" value="Rosa""#),
        "{html}"
    );
    let mut person = vec![("step", "person"), ORG];
    person.extend(PERSON);
    let (status, _, html) = app.post("/setup", &body(&person)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Sales tax rate") && html.contains("Finish setup"));

    // Step 3: a tax rate with 3 decimals is refused, not rounded.
    let mut bad = vec![("step", "settings"), ORG];
    bad.extend(PERSON);
    bad.extend([("tax_rate", "5.555"), ("country_code", "1"), ("area_code", "20")]);
    let (status, _, html) = app.post("/setup", &body(&bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Use at most 2 decimal places.") && html.contains("Enter 3 digits"));
    assert_eq!(app.count("SELECT COUNT(*) FROM party"), 0, "nothing is written before the last step");

    // Finish: HX-Redirect to the dashboard.
    let (status, headers, _) = app.post("/setup", &finish()).await;
    assert_eq!((status, headers["HX-Redirect"].to_str().unwrap()), (StatusCode::OK, "/"));

    // BR-ADMIN-01: the organization is the system owner, the person is its employee and the single user.
    assert_eq!(app.count("SELECT COUNT(*) FROM party"), 2);
    assert_eq!(app.count("SELECT COUNT(*) FROM organization WHERE organization_name = 'Harbor & Pine Design'"), 1);
    assert_eq!(
        app.count("SELECT COUNT(*) FROM person WHERE first_name = 'Rosa' AND last_name = 'Lindqvist' AND job_title = 'Principal'"),
        1
    );
    assert_eq!(
        app.count(
            "SELECT COUNT(*) FROM system_setting s JOIN organization o ON o.id = s.owner_id
             WHERE s.singleton = 1 AND s.initial_setup_done_ind = 1 AND s.tax_rate_bp = 550 AND s.area_code = 207
               AND s.country_code = '1' AND s.automatically_format_phone_numbers_ind = 1"
        ),
        1
    );
    assert_eq!(
        app.count(
            "SELECT COUNT(*) FROM party_role r JOIN employee e ON e.id = r.id JOIN person p ON p.id = r.party_id
             WHERE r.employee_ind = 1 AND r.current_ind = 1 AND e.name_cached = 'Rosa Lindqvist' AND e.initials = 'RL'"
        ),
        1
    );
    assert_eq!(
        app.count(
            "SELECT COUNT(*) FROM user u JOIN person p ON p.id = u.party_id
             WHERE u.email_address = 'rosa@harborpine.example' AND u.account_name <> ''"
        ),
        1
    );
    assert_eq!(app.count("SELECT COUNT(*) FROM user"), 1, "one user per install (BR-ADMIN-04)");
    assert_eq!(
        app.count(
            "SELECT COUNT(*) FROM party_relation r JOIN system_setting s ON s.owner_id = r.party_id_to
             JOIN user u ON u.party_id = r.party_id_from WHERE r.current_ind = 1"
        ),
        1,
        "the person works for the owner organization"
    );
    assert!(db::foreign_key_check(&app.db()).unwrap().is_empty());

    // BR-PARTY-07: every row setup wrote carries the new user's stamps (local civil time, D-05).
    let user_id = app.text("SELECT id FROM user");
    for table in ["party", "organization", "person", "party_relation", "party_role", "employee", "user"] {
        let unstamped = app.count(&format!(
            "SELECT COUNT(*) FROM {table} WHERE creation_user_id IS NOT '{user_id}' OR modification_user_id IS NOT '{user_id}'
               OR creation_timestamp IS NULL OR creation_account_name IS NULL"
        ));
        assert_eq!(unstamped, 0, "{table}");
    }
    assert_eq!(
        app.count(&format!(
            "SELECT COUNT(*) FROM system_setting WHERE creation_user_id = '{user_id}' AND modification_user_id = '{user_id}'
               AND modification_account_name <> '' AND modification_timestamp IS NOT NULL"
        )),
        1
    );
    let ts = app.text("SELECT creation_timestamp FROM party LIMIT 1");
    assert!(audit::is_timestamp(&ts), "{ts}");

    // The app opens now, and setup cannot run again.
    let (status, _, html) = app.get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Rosa"), "the dashboard greets the new user");
    let (status, headers, _) = app.get("/setup").await;
    assert_eq!((status, headers[header::LOCATION].to_str().unwrap()), (StatusCode::SEE_OTHER, "/"));
    let _ = app.post("/setup", &finish()).await;
    assert_eq!(app.count("SELECT COUNT(*) FROM party"), 2, "a second setup writes nothing");
}

fn input() -> SetupInput {
    SetupInput {
        organization_name: "Harbor & Pine Design".into(),
        first_name: "Rosa".into(),
        last_name: "Lindqvist".into(),
        tax_rate_bp: 550,
        country_code: "1".into(),
        account_name: "rosa".into(),
        ..Default::default()
    }
}

/// The tables setup writes, counted.
fn setup_rows(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM party) + (SELECT COUNT(*) FROM organization) + (SELECT COUNT(*) FROM person)
              + (SELECT COUNT(*) FROM party_role) + (SELECT COUNT(*) FROM employee) + (SELECT COUNT(*) FROM party_relation)
              + (SELECT COUNT(*) FROM user) + (SELECT COUNT(*) FROM system_setting)",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn a_setup_failure_leaves_the_database_empty() {
    // The user insert fails after the parties, roles and relation are written: everything rolls back.
    let (mut conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_user BEFORE INSERT ON user BEGIN SELECT RAISE(ABORT, 'disk full'); END;")
        .unwrap();
    match admin::setup_welcome(&mut conn, &input()) {
        Err(CommandError::Storage(e)) => assert!(e.to_string().contains("disk full"), "{e}"),
        other => panic!("expected a storage error, got {other:?}"),
    }
    assert_eq!(setup_rows(&conn), 0);
    assert!(!admin::setup_done(&conn).unwrap());
    assert_eq!(admin::current_user(&conn).unwrap(), None);

    // The last write (the settings row) failing rolls back too.
    conn.execute_batch(
        "DROP TRIGGER fail_user;
         CREATE TRIGGER fail_settings BEFORE INSERT ON system_setting BEGIN SELECT RAISE(ABORT, 'locked'); END;",
    )
    .unwrap();
    assert!(admin::setup_welcome(&mut conn, &input()).is_err());
    assert_eq!(setup_rows(&conn), 0);

    // With the cause gone, setup works on the same database.
    conn.execute_batch("DROP TRIGGER fail_settings;").unwrap();
    let done = admin::setup_welcome(&mut conn, &input()).unwrap();
    assert!(admin::setup_done(&conn).unwrap());
    assert_eq!(admin::current_user(&conn).unwrap(), Some(done.user));
    assert!(matches!(admin::setup_welcome(&mut conn, &input()), Err(CommandError::AlreadySetUp)));
}

#[tokio::test]
async fn a_failed_setup_shows_a_message_and_stays_on_setup() {
    let app = start(Seed::Reference).await;
    app.db()
        .execute_batch("CREATE TRIGGER fail_user BEFORE INSERT ON user BEGIN SELECT RAISE(ABORT, 'disk full'); END;")
        .unwrap();
    let (status, _, html) = app.post("/setup", &finish()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Setup could not be saved, so nothing was changed."));
    assert!(!html.contains("disk full"), "the cause goes to the log, not the page");
    assert_eq!(setup_rows(&app.db()), 0);
    let (status, headers, _) = app.get("/").await;
    assert_eq!((status, headers[header::LOCATION].to_str().unwrap()), (StatusCode::SEE_OTHER, "/setup"));
}

#[test]
fn current_user_is_the_single_local_user() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    assert_eq!(admin::current_user(&conn).unwrap(), Some(Actor::user("u-mara", "mara")));
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    assert_eq!(admin::current_user(&conn).unwrap(), None);
}

#[tokio::test]
async fn unknown_setup_fields_are_refused() {
    let app = start(Seed::Reference).await;
    let (status, _, _) = app.post("/setup", &format!("{}&initial_setup_done_ind=1", finish())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "deny_unknown_fields (05)");
    assert_eq!(app.count("SELECT COUNT(*) FROM party"), 0);
}

// ---------------------------------------------------------------------------------------------------------------
// BR-ADMIN-05 settings, AP-03 / T-08-03
// ---------------------------------------------------------------------------------------------------------------

/// The settings form with the demo values, overridden by `changes`.
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
        match pairs.iter_mut().find(|(name, _)| name == k) {
            Some(p) => p.1 = v,
            None => pairs.push((k, v)),
        }
    }
    body(&pairs)
}

const PASSWORD: &str = "Sm7p-pa55-w0rd-9931";

#[tokio::test]
async fn settings_page_shows_the_settings_and_write_only_secrets() {
    let app = start(Seed::Demo).await;
    let (status, _, html) = app.get("/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<h1 class=\"page-title\">Settings</h1>"));
    assert!(html.contains(r#"href="/settings" aria-current="page""#), "the sidebar links Settings");
    assert!(html.contains(r#"name="tax_rate" value="5.5""#), "550 bp shown as a percent");
    assert!(html.contains(r#"<option value="p-ellison" selected>Ellison Studio</option>"#));
    assert!(html.contains(r#"name="area_code" value="207""#));
    assert!(html.contains(r#"name="system_smtp_password" value="""#) && html.contains("Not set"));
    for key in ["google_places", "smartystreets", "fullcontact"] {
        assert!(html.contains(&format!(r#"type="password" name="{key}" value="""#)), "{key}");
    }
    assert!(html.contains("Accounting:</span> Not implemented"), "the OP-INTEG-04 stub line");
    assert!(!html.contains("QuickBooks") && !html.contains("QBO"));
    // htmx gets the form only.
    let (_, _, fragment) = app.htmx_get("/settings").await;
    assert!(fragment.starts_with("<form id=\"settings-form\"") || fragment.contains("<form id=\"settings-form\""));
    assert!(!fragment.contains("<!doctype html>"));
}

#[tokio::test]
async fn settings_round_trip_with_stamps() {
    let app = start(Seed::Demo).await;
    let changes = [
        ("tax_rate", "6.25"),
        ("default_tax_code", "TAX"),
        ("country_code", "+1"),
        ("area_code", "603"),
        ("format_phone_numbers", ""),
        ("email_address", "billing@ellisonstudio.example"),
        ("email_full_name", "Ellison Studio"),
        ("email_user_name", "billing@ellisonstudio.example"),
        ("smtp_server", "smtp.ellisonstudio.example"),
        ("smtp_port", "465"),
        ("smtp_encryption", "SSL/TLS"),
        ("system_smtp_password", PASSWORD),
        ("fullcontact", "fc-key-123456"),
    ];
    let (status, headers, html) = app.put("/settings", &settings_body(&changes)).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert_eq!(headers["HX-Trigger"], r#"{"karbon-toast":"Settings saved"}"#);
    assert!(html.contains("Settings saved."));
    assert!(!html.contains(PASSWORD) && !html.contains("fc-key-123456"), "the answer does not echo a secret");

    let db = app.db();
    let row: (i64, String, String, i64, i64, String, String, i64, String, String) = db
        .query_row(
            "SELECT tax_rate_bp, default_tax_code_id, country_code, area_code, automatically_format_phone_numbers_ind,
                    system_email_address, system_email_smtp_server, system_email_smtp_server_port,
                    system_email_smtp_server_encryption_type, modification_user_id
             FROM system_setting",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        (
            625,
            "TAX".into(),
            "1".into(),
            603,
            0,
            "billing@ellisonstudio.example".into(),
            "smtp.ellisonstudio.example".into(),
            465,
            "SSL/TLS".into(),
            "u-mara".into()
        )
    );
    let ts = app.text("SELECT modification_timestamp FROM system_setting");
    assert!(audit::is_timestamp(&ts), "BR-PARTY-07 stamp: {ts}");

    // The secrets went to the store, not to SQLite.
    assert_eq!(app.store.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), PASSWORD);
    assert_eq!(app.store.get(SecretKey::FullContactApiKey).unwrap().unwrap().expose(), "fc-key-123456");
    assert!(!app.store.has_secret(SecretKey::GooglePlacesApiKey).unwrap());

    // GET shows the saved values, and only "Saved." for the secrets.
    let (_, _, html) = app.get("/settings").await;
    assert!(html.contains(r#"name="tax_rate" value="6.25""#) && html.contains(r#"name="area_code" value="603""#));
    assert!(html.contains(r#"<option selected>SSL/TLS</option>"#));
    assert!(html.contains(r#"name="smtp_port" value="465""#));
    assert!(!html.contains(r#"name="format_phone_numbers" value="on" class="checkbox checkbox-sm" checked"#));
    assert_eq!(html.matches("Saved. Type a new value to replace it.").count(), 2, "SMTP password and FullContact key");

    // An empty secret field keeps the saved secret.
    let (status, _, _) = app.put("/settings", &settings_body(&[("tax_rate", "6.25")])).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.store.get(SecretKey::SystemSmtpPassword).unwrap().unwrap().expose(), PASSWORD);
}

#[tokio::test]
async fn bad_settings_are_refused_and_nothing_is_saved() {
    let app = start(Seed::Demo).await;
    let (status, _, html) = app
        .put(
            "/settings",
            &settings_body(&[
                ("tax_rate", "abc"),
                ("smtp_port", "70000"),
                ("email_address", "not-an-email"),
                ("smtp_encryption", "ROT13"),
                ("system_smtp_password", PASSWORD),
            ]),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    for msg in
        ["Enter a percent", "Enter a port from 1 to 65535", "Enter an email address", "Choose an encryption type"]
    {
        assert!(html.contains(msg), "{msg}");
    }
    assert!(html.contains("Nothing was saved."));
    assert!(html.contains(r#"name="tax_rate" value="abc""#), "the typed value is kept");
    assert!(!html.contains(PASSWORD), "a typed secret is not rendered back");
    assert!(!app.store.has_secret(SecretKey::SystemSmtpPassword).unwrap(), "a refused form saves no secret");
    assert_eq!(app.count("SELECT tax_rate_bp FROM system_setting"), 550);

    // An owner that is not an organization (a person) is refused, again without saving the secret.
    let (status, _, html) =
        app.put("/settings", &settings_body(&[("owner_id", "p-mara"), ("system_smtp_password", PASSWORD)])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Choose the organization that owns this install."));
    assert!(!app.store.has_secret(SecretKey::SystemSmtpPassword).unwrap());

    // An unknown field is refused (05: deny_unknown_fields).
    let (status, _, _) = app.put("/settings", &settings_body(&[("initial_setup_done_ind", "0")])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// T-08-03 on the real settings page: with secrets saved, no page and no table has the value.
#[tokio::test]
async fn t_08_03_settings_page_never_shows_a_saved_secret() {
    let app = start(Seed::Demo).await;
    let (status, _, _) = app
        .put("/settings", &settings_body(&[("system_smtp_password", PASSWORD), ("google_places", "gp-AIza-777")]))
        .await;
    assert_eq!(status, StatusCode::OK);
    for (method, path) in karbon_server::ROUTES.iter().filter(|(m, p)| *m == "GET" && !p.starts_with("/session")) {
        let uri = path.replace("{doc_number}", "INV-1048").replace("{*path}", "app.js");
        let (_, _, html) = app.get(&uri).await;
        assert!(!html.contains(PASSWORD) && !html.contains("gp-AIza-777"), "{method} {path} shows a secret");
    }
    let (_, _, html) = app.get("/settings").await;
    assert!(html.contains(r#"name="system_smtp_password" value="""#) && html.contains("Saved."));
    // SQLite has no copy: search every text value of every table.
    let db = app.db();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for table in tables {
        let cols: Vec<String> = db
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for col in cols {
            let hits: i64 = db
                .query_row(
                    &format!("SELECT COUNT(*) FROM \"{table}\" WHERE CAST(\"{col}\" AS TEXT) LIKE '%' || ?1 || '%'"),
                    [PASSWORD],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(hits, 0, "{table}.{col}");
        }
    }
}

/// I4 (BR-INTEG-01, AP-03): the user's own email account saves on the user row; its password goes to the keychain
/// and never back to the page; empty server, port, encryption and authentication mean "as the system account".
#[tokio::test]
async fn the_user_email_account_round_trip() {
    let app = start(Seed::Demo).await;
    const USER_PW: &str = "user-Sm7p-0042";
    let body = settings_body(&[
        ("user_email_address", "mara@ellisonstudio.example"),
        ("user_full_name", "Mara Ellison"),
        ("user_user_name", "mara"),
        ("user_smtp_server", ""),
        ("user_smtp_port", ""),
        ("user_smtp_encryption", ""),
        ("user_smtp_authentication", "Password"),
        ("user_smtp_password", USER_PW),
    ]);
    let (status, _, html) = app.put("/settings", &body).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(!html.contains(USER_PW));
    let (address, name, user, server, auth): (String, String, String, Option<String>, String) = app
        .db()
        .query_row(
            "SELECT smtp_email_address, smtp_full_name, smtp_user_name, smtp_server, smtp_server_authentication
             FROM user WHERE id = 'u-mara'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        (address.as_str(), name.as_str(), user.as_str(), server, auth.as_str()),
        ("mara@ellisonstudio.example", "Mara Ellison", "mara", None, "Password")
    );
    let (_, _, html) = app.get("/settings").await;
    assert!(html.contains("Your email account") && html.contains(r#"name="user_smtp_password" value="""#));
    assert!(!html.contains(USER_PW));
    assert_eq!(app.count(&format!("SELECT COUNT(*) FROM user WHERE CAST(smtp_server AS TEXT) LIKE '%{USER_PW}%'")), 0);

    let (status, _, html) =
        app.put("/settings", &settings_body(&[("user_smtp_port", "99999"), ("user_email_address", "nope")])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enter a port from 1 to 65535, or leave it empty."));
}
