//! The pages and fragments of I6, through the router with the full AP-13 guard on (UI-17 Integrations panel, UI-06
//! address picker in the address dialog, UI-04 "Look up on FullContact…", OP-INTEG-04 status line; AP-12, BR-INTEG-02,
//! BR-INTEG-03). No test here makes an outbound call: the calls are in `integrations_contract.rs`.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::admin;
use karbon_server::db::{self, Seed};
use karbon_server::integrations::AddressProvider;
use karbon_server::secrets::{MemoryStore, Secret, SecretKey, SecretStore};
use karbon_server::session::Session;
use karbon_server::{router, AppState};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:4377";
const ORIGIN: &str = "http://127.0.0.1:4377";

struct App {
    router: axum::Router,
    cookie: String,
    store: Arc<MemoryStore>,
    path: PathBuf,
    _dir: Option<tempfile::TempDir>,
}

async fn send(router: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

/// A guarded router on the database at `path`, already past the launch URL (as the window is).
async fn around(conn: Connection, path: PathBuf, dir: Option<tempfile::TempDir>) -> App {
    let session = Arc::new(Session::new(4377));
    let launch = session.launch_path().unwrap();
    let store = Arc::new(MemoryStore::default());
    let state = AppState::new(conn).with_origin(ORIGIN).with_session(session).with_secrets(store.clone());
    let router = router(state);
    let req = Request::get(&launch).header(header::HOST, HOST).body(Body::empty()).unwrap();
    let (status, headers, _) = send(&router, req).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
    App { router, cookie, store, path, _dir: dir }
}

async fn start(seed: Seed) -> App {
    let dir = tempfile::tempdir().unwrap();
    let path = db::database_path(dir.path());
    let (conn, _) = db::open(&path, seed).unwrap();
    around(conn, path, Some(dir)).await
}

impl App {
    async fn get(&self, uri: &str) -> (StatusCode, HeaderMap, String) {
        let req = Request::get(uri)
            .header(header::HOST, HOST)
            .header(header::COOKIE, &self.cookie)
            .header("HX-Request", "true")
            .body(Body::empty())
            .unwrap();
        send(&self.router, req).await
    }
    async fn page(&self, uri: &str) -> String {
        let req = Request::get(uri)
            .header(header::HOST, HOST)
            .header(header::COOKIE, &self.cookie)
            .body(Body::empty())
            .unwrap();
        let (status, _, html) = send(&self.router, req).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        html
    }
    async fn form(&self, method: &str, uri: &str, body: &str) -> (StatusCode, HeaderMap, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("HX-Request", "true")
            .body(Body::from(body.to_owned()))
            .unwrap();
        send(&self.router, req).await
    }
    fn db(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }
    fn switches(&self) -> (Option<String>, i64) {
        self.db()
            .query_row(
                "SELECT address_suggestion_provider, COALESCE(fullcontact_enabled_ind, 0) FROM system_setting",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
    fn set_switches(&self, provider: Option<&str>, fullcontact: bool) {
        self.db()
            .execute(
                "UPDATE system_setting SET address_suggestion_provider = ?1, fullcontact_enabled_ind = ?2",
                rusqlite::params![provider, i64::from(fullcontact)],
            )
            .unwrap();
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

/// The demo's settings form, overridden by `changes`.
fn settings_body(changes: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(&str, &str)> = vec![
        ("owner_id", "p-ellison"),
        ("tax_rate", "5.5"),
        ("country_code", "1"),
        ("area_code", "207"),
        ("format_phone_numbers", "on"),
        ("smtp_encryption", "STARTTLS"),
        ("smtp_authentication", "Password"),
    ];
    for (k, v) in changes {
        match pairs.iter_mut().find(|(name, _)| name == k) {
            Some(p) => p.1 = v,
            None => pairs.push((k, v)),
        }
    }
    pairs.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&")
}

/// What "every integration off" looks like on the settings page.
fn assert_settings_show_all_off(html: &str) {
    assert!(html.contains(r#"<option value="" selected>Off</option>"#), "address suggestions off: {html}");
    for p in AddressProvider::ALL {
        assert!(html.contains(&format!(r#"<option value="{0}" >{0}</option>"#, p.stored())), "{}", p.stored());
    }
    let checkbox = html.split(r#"name="fullcontact_enabled""#).nth(1).expect("the FullContact switch");
    assert!(!checkbox[..checkbox.find('>').unwrap()].contains("checked"), "FullContact off");
    for key in ["google_places", "smartystreets_auth_id", "smartystreets", "fullcontact"] {
        assert!(html.contains(&format!(r#"type="password" name="{key}" value="""#)), "{key}");
    }
    assert!(html.contains("Each service is off until you turn it on here and save its key."));
    assert!(html.contains("Accounting:</span> Not implemented"));
}

fn assert_no_integration_actions(header: &str, dialog: &str) {
    assert!(!header.contains("Look up on FullContact"), "no enrichment action when off");
    assert!(
        !dialog.contains("Find an address") && !dialog.contains("/address-suggestions"),
        "no address search when off"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Off by default (AP-12)
// ---------------------------------------------------------------------------------------------------------------

/// A fresh install (setup done, no demo) shows every integration off, and so do the small and the big demo.
#[tokio::test]
async fn a_fresh_install_and_the_demos_show_every_integration_off() {
    // Fresh: through setup first, as a user would.
    let app = start(Seed::Reference).await;
    let s = admin::settings_get(&app.db()).unwrap();
    assert_eq!((s.address_provider, s.fullcontact_enabled), (None, false));
    let setup = "step=settings&organization_name=Harbor+%26+Pine+Design&first_name=Rosa&last_name=Lindqvist\
                 &job_title=Principal&email=rosa%40harborpine.example&tax_rate=5.5&country_code=1&area_code=207";
    let (status, _, html) = app.form("POST", "/setup", setup).await;
    assert!(status.is_success() || status.is_redirection(), "{status} {html}");
    assert_settings_show_all_off(&app.page("/settings").await);
    assert_eq!(app.switches(), (None, 0));

    // The small demo.
    let app = start(Seed::Demo).await;
    assert_settings_show_all_off(&app.page("/settings").await);
    let header = app.page("/parties/p-hp-eh").await;
    let (_, _, dialog) = app.get("/parties/p-hp-eh/contact-methods/new?kind=address").await;
    assert_no_integration_actions(&header, &dialog);

    // The big demo, the database the desktop app opens on (import of the synthetic export).
    let tmp = tempfile::tempdir().unwrap();
    let (export, data) = (tmp.path().join("export"), tmp.path().join("data"));
    karbon_synth::generate(karbon_synth::Config::default(), &export).unwrap();
    karbon_server::import::import_export(&export, &data, &Default::default()).unwrap();
    let path = db::database_path(&data);
    let conn = db::open(&path, Seed::Reference).unwrap().0;
    let app = around(conn, path, Some(tmp)).await;
    assert_eq!(app.switches(), (None, 0));
    assert_settings_show_all_off(&app.page("/settings").await);
}

// ---------------------------------------------------------------------------------------------------------------
// UI-17: turning an integration on (BR-INTEG-02: "only when the user enables them and stores a key")
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_integration_turns_on_only_with_its_keys() {
    let app = start(Seed::Demo).await;

    // On without a key: a field error, and nothing at all is saved (not the typed FullContact key, not the tax rate).
    let (status, _, html) = app
        .form(
            "PUT",
            "/settings",
            &settings_body(&[("address_provider", "Google Places"), ("tax_rate", "7"), ("fullcontact", "fc-typed")]),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Add the Google Places API key below to turn on Google Places."), "{html}");
    assert!(html.contains(r#"aria-describedby="err-address_provider""#));
    assert!(!html.contains("fc-typed"), "a typed key is not echoed");
    assert_eq!(app.switches(), (None, 0));
    assert!(!app.store.has_secret(SecretKey::FullContactApiKey).unwrap());
    let rate: i64 = app.db().query_row("SELECT tax_rate_bp FROM system_setting", [], |r| r.get(0)).unwrap();
    assert_eq!(rate, 550);

    // SmartyStreets needs its auth ID as well as its token.
    let (status, _, html) = app
        .form("PUT", "/settings", &settings_body(&[("address_provider", "SmartyStreets"), ("smartystreets", "tok-1")]))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Add the SmartyStreets auth ID below to turn on SmartyStreets."), "{html}");
    assert!(!app.store.has_secret(SecretKey::SmartyStreetsApiKey).unwrap());

    // FullContact on without its key.
    let (status, _, html) = app.form("PUT", "/settings", &settings_body(&[("fullcontact_enabled", "on")])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Add the FullContact API key below to turn on FullContact."), "{html}");

    // An unknown service.
    let (status, _, html) = app.form("PUT", "/settings", &settings_body(&[("address_provider", "Bing Maps")])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Choose Off, Google Places or SmartyStreets."));

    // With the keys typed in the same save: on, and the keys in the keychain, not in SQLite.
    let (status, _, html) = app
        .form(
            "PUT",
            "/settings",
            &settings_body(&[
                ("address_provider", "SmartyStreets"),
                ("smartystreets_auth_id", "id-123"),
                ("smartystreets", "tok-456"),
                ("fullcontact_enabled", "on"),
                ("fullcontact", "fc-789"),
            ]),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert_eq!(app.switches(), (Some("SmartyStreets".into()), 1));
    assert_eq!(app.store.get(SecretKey::SmartyStreetsAuthId).unwrap().unwrap().expose(), "id-123");
    assert_eq!(app.store.get(SecretKey::SmartyStreetsApiKey).unwrap().unwrap().expose(), "tok-456");
    assert!(!html.contains("id-123") && !html.contains("tok-456") && !html.contains("fc-789"));
    assert!(html.contains(r#"<option value="SmartyStreets" selected>SmartyStreets</option>"#), "{html}");
    let dump: String = app
        .db()
        .query_row("SELECT group_concat(COALESCE(address_suggestion_provider,'') || fullcontact_enabled_ind) FROM system_setting", [], |r| r.get(0))
        .unwrap();
    assert!(!dump.contains("tok-456"));

    // Saved keys are enough later; turning a service off keeps its key but stops the calls.
    let (status, _, _) = app
        .form(
            "PUT",
            "/settings",
            &settings_body(&[("address_provider", "SmartyStreets"), ("fullcontact_enabled", "on")]),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app.form("PUT", "/settings", &settings_body(&[])).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.switches(), (None, 0));
    assert!(app.store.has_secret(SecretKey::FullContactApiKey).unwrap());
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("turned off"));
}

// ---------------------------------------------------------------------------------------------------------------
// UI-06 address picker and UI-04 enrichment action, when on
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_address_dialog_offers_the_search_only_when_on() {
    let app = start(Seed::Demo).await;
    app.set_switches(Some("Google Places"), false);
    app.store.set(SecretKey::GooglePlacesApiKey, &Secret::new("gp-key")).unwrap();

    let (status, _, html) = app.get("/parties/p-hp-eh/contact-methods/new?kind=address").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Find an address") && html.contains(r#"hx-get="/address-suggestions""#), "{html}");
    assert!(html.contains("Suggestions from Google Places."));
    assert!(html.contains(r#"<div id="address-fields""#) && html.contains(r#"name="line1" value="""#));
    // Edit of an existing address: the same search, the fields filled.
    let (status, _, html) = app.get("/contact-methods/cm-003/edit").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Find an address") && html.contains(r#"name="line1" value="12 Fore Street""#), "{html}");
    // Not on a phone or email dialog.
    let (_, _, html) = app.get("/parties/p-hp-eh/contact-methods/new?kind=phone").await;
    assert!(!html.contains("Find an address"));

    // Saving the dialog with the search box's text saves the address; the search text is not stored.
    let body = "kind=address&q=12+Fore&line1=48+Wharf+Street&line2=Suite+3&city=Portland&state=ME&postal_code=04101&country=US&label=&comment=";
    let (status, _, html) = app.form("POST", "/parties/p-hp-eh/contact-methods", body).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let n: i64 = app
        .db()
        .query_row(
            "SELECT COUNT(*) FROM address a JOIN contact_method cm ON cm.id = a.id WHERE cm.party_id = 'p-hp-eh' AND a.address_line_1 = '48 Wharf Street'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    // A validation error re-renders the dialog with the search still there.
    let (status, _, html) =
        app.form("POST", "/parties/p-hp-eh/contact-methods", "kind=address&q=x&line1=&city=&postal_code=").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Find an address") && html.contains("Enter a street, a city or a postal code."));
}

#[tokio::test]
async fn the_fullcontact_action_is_on_a_person_only_when_on() {
    let app = start(Seed::Demo).await;
    assert!(!app.page("/parties/p-hp-eh").await.contains("Look up on FullContact"));
    app.set_switches(None, true);
    let html = app.page("/parties/p-hp-eh").await;
    assert!(
        html.contains(
            r##"<button hx-post="/parties/p-hp-eh/enrich" hx-target="#modal-body">Look up on FullContact…</button>"##
        ),
        "{html}"
    );
    assert_eq!(html.matches("Look up on FullContact").count(), 1, "one action, in the More menu (AGENTS.md)");
    assert!(!app.page("/parties/p-hp").await.contains("Look up on FullContact"), "not on an organization");
    // The header that a contact-method change sends back keeps the action.
    let (status, _, html) = app.form("POST", "/contact-methods/cm-036/primary", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Look up on FullContact"));
    // Not on an archived person.
    let (status, _, _) = app.form("POST", "/parties/p-hp-jw/archive", "reason=Left").await;
    assert!(status.is_success() || status.is_redirection());
    let archived = app.page("/parties/p-hp-jw").await;
    assert!(archived.contains("Archived") && !archived.contains("Look up on FullContact"));
}

/// Keep with nothing checked changes nothing and makes no call (the dialog's form), and an apply on an organization is
/// refused in the dialog.
#[tokio::test]
async fn enrich_apply_with_nothing_kept_or_on_an_organization() {
    let app = start(Seed::Demo).await;
    let before: String = app
        .db()
        .query_row("SELECT COALESCE(modification_timestamp,'') FROM party WHERE id='p-hp-eh'", [], |r| r.get(0))
        .unwrap();
    let (status, headers, _) =
        app.form("POST", "/parties/p-hp-eh/enrich/apply", "keep_job_title=&job_title=Architect").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("Nothing changed"));
    let after: String = app
        .db()
        .query_row("SELECT COALESCE(modification_timestamp,'') FROM party WHERE id='p-hp-eh'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    let (status, _, html) = app.form("POST", "/parties/p-hp/enrich/apply", "keep_bio=on&bio=x").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("this party is an organization"), "{html}");
    let (status, _, _) = app.form("POST", "/parties/p-hp-eh/enrich/apply", "unknown=1").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown fields are refused (05)");
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-04 accounting_integration_status (BR-INTEG-03)
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_accounting_stub_says_not_implemented_and_changes_nothing() {
    let app = start(Seed::Demo).await;
    let state = |db: &Connection| -> (i64, String) {
        db.query_row(
            "SELECT (SELECT COUNT(*) FROM external_link), COALESCE((SELECT modification_timestamp FROM system_setting), '')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    let before = state(&app.db());
    let (status, _, html) = app.get("/integrations/accounting").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        html.trim(),
        r#"<p class="text-[13px]" id="accounting-status"><span class="muted">Accounting:</span> Not implemented</p>"#
    );
    assert_eq!(state(&app.db()), before, "no QBO data read, sent or stored");
    let s = karbon_server::integrations::accounting_integration_status();
    assert!(!s.implemented && s.provider.is_none());
}

// ---------------------------------------------------------------------------------------------------------------
// AP-13: the new routes answer the window only
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn the_new_routes_need_the_session() {
    let app = start(Seed::Demo).await;
    for uri in ["/address-suggestions?q=1", "/address-suggestions/pick?line1=1", "/integrations/accounting"] {
        let req = Request::get(uri).header(header::HOST, HOST).body(Body::empty()).unwrap();
        let (status, _, _) = send(&app.router, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    for uri in ["/parties/p-hp-eh/enrich", "/parties/p-hp-eh/enrich/apply"] {
        let req = Request::post(uri).header(header::HOST, HOST).body(Body::empty()).unwrap();
        let (status, _, _) = send(&app.router, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
}
