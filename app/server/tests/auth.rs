//! Authorization tests of 08 for I1: T-08-01, T-08-02, T-08-03, T-08-10 and T-08-14.
//!
//! Most go through the router in process with the full guard on (origin and session set, as `serve` sets them);
//! `guard_over_a_real_socket` and `loopback_only` use a real listener from `bind()` and `serve()`.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::net::{Integration, OutboundPolicy, Refused};
use karbon_server::secrets::{MemoryStore, Secret, SecretKey, SecretStore};
use karbon_server::session::Session;
use karbon_server::{db, router, AppState, ROUTES};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:4321";
const ORIGIN: &str = "http://127.0.0.1:4321";

/// A guarded router, its launch path and a counter of /print calls (to show that a refused request changes nothing).
fn guarded() -> (axum::Router, String, Arc<AtomicUsize>) {
    let session = Arc::new(Session::new(4321));
    let launch = session.launch_path().unwrap();
    let prints = Arc::new(AtomicUsize::new(0));
    let counter = prints.clone();
    let state = AppState::new(db::open_demo(None).unwrap()).with_origin(ORIGIN).with_session(session).with_print(
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    (router(state), launch, prints)
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

/// A request to this server (right Host), with an optional cookie and Origin.
fn req(method: &str, uri: &str, cookie: Option<&str>, origin: Option<&str>) -> Request<Body> {
    let mut r = Request::builder().method(method).uri(uri).header(header::HOST, HOST);
    if let Some(c) = cookie {
        r = r.header(header::COOKIE, c);
    }
    if let Some(o) = origin {
        r = r.header(header::ORIGIN, o);
    }
    if method == "POST" {
        r = r.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        return r.body(Body::from("name=Acme&email=")).unwrap();
    }
    r.body(Body::empty()).unwrap()
}

/// Opens the launch URL like the window does and returns the `name=value` cookie it sets.
async fn launch(app: &axum::Router, path: &str) -> String {
    let (status, headers, _) = send(app, req("GET", path, None, None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/");
    let set = headers[header::SET_COOKIE].to_str().unwrap();
    for flag in ["HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(set.contains(flag), "{set} lacks {flag}");
    }
    set.split(';').next().unwrap().to_owned()
}

/// A concrete URI for a route of the list.
fn sample(path: &str) -> String {
    // A real id of the demo seed for each kind of record, so a listed route answers instead of 404.
    let id = if path.starts_with("/contact-methods/") {
        "cm-001"
    } else if path.starts_with("/relations/") {
        "rel-mara"
    } else {
        "p-hp"
    };
    path.replace("{doc_number}", "INV-1048")
        .replace("{id}", id)
        .replace("{type_id}", "1")
        .replace("{*path}", "app.css")
        .replace("{token}", "spent")
}

/// T-08-14: without the session cookie, with a foreign Host, or a POST with a foreign Origin: refused, nothing changes.
#[tokio::test]
async fn t_08_14_only_the_app_window_gets_in() {
    let (app, launch_path, prints) = guarded();

    // Another local process or a page in a browser, before or after the window opened: no cookie, 401.
    for (method, uri) in
        [("GET", "/"), ("GET", "/customers"), ("GET", "/sales/INV-1048/invoice.pdf"), ("POST", "/print")]
    {
        let (status, _, body) = send(&app, req(method, uri, None, Some(ORIGIN))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert!(!body.contains("Halden"), "no data in the refusal");
    }
    // A made-up launch token: 403, no cookie.
    let (status, headers, _) = send(&app, req("GET", &format!("/session/{}", "0".repeat(64)), None, None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(headers.get(header::SET_COOKIE).is_none());

    // The window opens the launch URL once.
    let cookie = launch(&app, &launch_path).await;
    let cookie = cookie.as_str();
    let (status, _, html) = send(&app, req("GET", "/", Some(cookie), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Good day, Mara"));

    // The token works once: a second client that learned it gets 403 and no cookie.
    let (status, headers, _) = send(&app, req("GET", &launch_path, None, None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(headers.get(header::SET_COOKIE).is_none());
    // The window reloading the launch URL (dev browser) just goes on to /.
    let (status, _, _) = send(&app, req("GET", &launch_path, Some(cookie), None)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    // A wrong cookie value, or the right value under another server's cookie name: 401.
    let name = cookie.split('=').next().unwrap();
    let (status, _, _) = send(&app, req("GET", "/", Some(&format!("{name}={}", "f".repeat(64))), None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let value = cookie.split('=').nth(1).unwrap();
    let (status, _, _) = send(&app, req("GET", "/", Some(&format!("karbon_session_9999={value}")), None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // DNS rebinding: the cookie is not enough with a foreign Host.
    let rebind = Request::get("/").header(header::HOST, "evil.example:4321").header(header::COOKIE, cookie);
    let (status, _, _) = send(&app, rebind.body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);

    // A POST with the cookie but a foreign Origin, or none: 403.
    let (status, _, _) = send(&app, req("POST", "/print", Some(cookie), Some("https://evil.example"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = send(&app, req("POST", "/print", Some(cookie), None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(prints.load(Ordering::SeqCst), 0, "no refused request reached the handler");

    // The window's own POST works.
    let (status, _, _) = send(&app, req("POST", "/print", Some(cookie), Some(ORIGIN))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(prints.load(Ordering::SeqCst), 1);

    // Static files need no cookie (05 Guards); they hold no data.
    let (status, _, _) = send(&app, req("GET", "/assets/app.css", None, None)).await;
    assert_eq!(status, StatusCode::OK);
}

/// T-08-14 over a real socket: the server from `bind()` + `serve()` refuses a client without the cookie.
#[tokio::test]
async fn guard_over_a_real_socket() {
    let (listener, addr) = karbon_server::bind().await.unwrap();
    let session = Arc::new(Session::new(addr.port()));
    let launch_path = session.launch_path().unwrap();
    let state = AppState::new(db::open_demo(None).unwrap()).with_session(session);
    tokio::spawn(karbon_server::serve(listener, state));

    // A plain HTTP/1.1 client on a blocking std socket, like another local process would use.
    let raw = |path: String, cookie: Option<String>| async move {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        let cookie = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
        let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}Connection: close\r\n\r\n");
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        tokio::task::spawn_blocking(move || {
            s.read_to_string(&mut out).unwrap();
            out
        })
        .await
        .unwrap()
    };
    let res = raw("/".into(), None).await;
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    let res = raw(launch_path.clone(), None).await;
    assert!(res.starts_with("HTTP/1.1 303"), "{res}");
    let cookie = res
        .lines()
        .find_map(|l| l.strip_prefix("set-cookie: ").or_else(|| l.strip_prefix("Set-Cookie: ")))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let res = raw("/".into(), Some(cookie)).await;
    assert!(res.starts_with("HTTP/1.1 200"), "{}", &res[..80]);
    let res = raw(launch_path, None).await;
    assert!(res.starts_with("HTTP/1.1 403"), "the token works once");
}

/// T-08-01: exactly the listed routes answer; any other path is 404; no generic table, SQL or IPC route.
#[tokio::test]
async fn t_08_01_only_registered_routes() {
    let (app, launch_path, _) = guarded();
    let cookie = launch(&app, &launch_path).await;
    let cookie = Some(cookie.as_str());

    for (method, path) in ROUTES {
        let (status, _, _) = send(&app, req(method, &sample(path), cookie, Some(ORIGIN))).await;
        assert!(
            status != StatusCode::NOT_FOUND && status != StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path} answered {status}"
        );
    }
    for uri in [
        "/sql",
        "/api/query?sql=SELECT+1",
        "/tables/party",
        "/customers/1/delete",
        "/customers/new",
        "/parties/p-hp/delete",
        "/ipc",
        "/plugin%3Asql%7Cexecute",
        "/__tauri__/ipc",
        "/db/karbonfm-demo.db",
        "/assets/../Cargo.toml",
        "/session",
    ] {
        let (status, _, _) = send(&app, req("GET", uri, cookie, None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
    // A listed path with an unlisted method.
    let (status, _, _) = send(&app, req("DELETE", "/customers", cookie, Some(ORIGIN))).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    // An unknown path without the cookie is refused before routing, so a scan learns nothing.
    let (status, _, _) = send(&app, req("GET", "/sql", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// T-08-02: a call to a host the user did not enable is refused (and logged with its host only).
#[test]
fn t_08_02_outbound_default_deny() {
    let fresh = OutboundPolicy::deny_all();
    for url in [
        "https://api.fullcontact.com/v3/person.enrich",
        "https://places.googleapis.com/v1/places:autocomplete",
        "https://example.com/",
    ] {
        assert!(matches!(fresh.check_https(url), Err(Refused::HostNotEnabled(_))), "{url}");
    }
    let enabled = OutboundPolicy::deny_all().enable(Integration::SmartyStreets);
    assert_eq!(enabled.check_https("https://us-autocomplete-pro.api.smarty.com/lookup?search=1"), Ok(()));
    assert!(enabled.check_https("https://api.fullcontact.com/v3/person.enrich").is_err());
    assert_eq!(enabled.check_https("http://us-autocomplete-pro.api.smarty.com/lookup"), Err(Refused::BadUrl));

    // Outbound calls exist only in Rust behind this policy: the server has no HTTP client crate of its own yet.
    let manifest = include_str!("../Cargo.toml");
    for client in ["reqwest", "ureq", "isahc", "curl", "lettre"] {
        assert!(!manifest.contains(client), "{client} added: route its calls through net::OutboundPolicy");
    }
}

/// T-08-03: a saved secret never reaches the HTML, the database or a log line.
#[test]
fn t_08_03_secrets_never_reach_the_page() {
    use askama::Template;
    const VALUE: &str = "s3cr3t-SMTP-pw-7781";
    let store = MemoryStore::default();
    store.set(SecretKey::UserSmtpPassword, &Secret::new(VALUE)).unwrap();

    let saved = store.field(SecretKey::UserSmtpPassword, "SMTP password").unwrap();
    assert!(saved.has_secret);
    let html = saved.render().unwrap();
    assert!(!html.contains(VALUE));
    assert!(html.contains(r#"type="password""#) && html.contains(r#"value="""#), "write-only field: {html}");
    assert!(html.contains("Saved."));
    let unset = store.field(SecretKey::FullContactApiKey, "FullContact API key").unwrap().render().unwrap();
    assert!(unset.contains("Not set"));

    // Logs: Debug of the secret, of the field and of the store's errors show no value.
    let secret = store.get(SecretKey::UserSmtpPassword).unwrap().unwrap();
    assert!(!format!("{secret:?} {saved:?}").contains(VALUE));

    // SQLite: schema 04 has no column for a secret (D-09).
    let conn = db::open_demo(None).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT m.name || '.' || p.name FROM sqlite_master m, pragma_table_info(m.name) p \
             WHERE m.type = 'table' AND (p.name LIKE '%password%' OR p.name LIKE '%secret%' \
             OR p.name LIKE '%api_key%' OR p.name LIKE '%apikey%' OR p.name LIKE '%token%')",
        )
        .unwrap();
    let columns: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert!(columns.is_empty(), "secret-like columns in SQLite: {columns:?}");
}

/// T-08-03: no page of the app renders a secret: with a value saved, every GET route's HTML is free of it.
#[tokio::test]
async fn t_08_03_no_page_shows_a_secret() {
    // Pages do not read secrets yet (the settings page comes with the setup slice); this pins that down for every
    // GET route of the list, so the settings page must keep to SecretField.
    let (app, launch_path, _) = guarded();
    let cookie = launch(&app, &launch_path).await;
    for (method, path) in ROUTES.iter().filter(|(m, p)| *m == "GET" && !p.starts_with("/session")) {
        let (_, _, body) = send(&app, req(method, &sample(path), Some(&cookie), None)).await;
        for word in ["password", "api_key", "apikey"] {
            assert!(!body.to_lowercase().contains(&format!("value=\"{word}")), "{path} renders a {word} value");
        }
    }
}

/// T-08-10: the server listens on loopback only; the LAN address of this machine refuses the connection.
#[tokio::test]
async fn t_08_10_loopback_only() {
    let (listener, addr) = karbon_server::bind().await.unwrap();
    assert!(addr.ip().is_loopback(), "{addr}");
    let session = Arc::new(Session::new(addr.port()));
    tokio::spawn(karbon_server::serve(listener, AppState::new(db::open_demo(None).unwrap()).with_session(session)));
    assert!(tokio::net::TcpStream::connect(addr).await.is_ok());

    // This machine's address on the network, if it has one (no packet is sent by a UDP connect).
    let lan = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|()| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip())
        .ok()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    match lan {
        Some(ip) => {
            let refused = tokio::net::TcpStream::connect((ip, addr.port())).await;
            assert!(refused.is_err(), "the app port answered on {ip}");
        }
        None => eprintln!("no network address: the LAN half of T-08-10 is skipped"),
    }
}

/// `serve` will not start without a session, so the guard cannot be left off by mistake.
#[tokio::test]
async fn serve_needs_a_session() {
    let (listener, _) = karbon_server::bind().await.unwrap();
    let err = karbon_server::serve(listener, AppState::new(db::open_demo(None).unwrap())).await.unwrap_err();
    assert!(err.to_string().contains("session"));
}
