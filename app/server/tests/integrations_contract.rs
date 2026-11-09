//! The contract tests of I6 (EXT-02, EXT-03; OP-INTEG-02, OP-INTEG-03; BR-INTEG-02; AP-02, AP-12; T-08-02, T-08-12).
//!
//! The real routes and the real `HttpsClient` call a mock server in this file, bound to 127.0.0.1 on a free port. It
//! speaks TLS with a test certificate for the real host names (`tests/fixtures/integ_tls`), and the client built by
//! `HttpsClient::for_contract_tests` resolves those names to it and trusts only the test CA. So the URLs, the policy,
//! the TLS check and the requests are those of the app; no test reaches a real host, and no real key is used.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::net::{HttpError, HttpRequest, HttpsClient, Integration, OutboundPolicy, Refused};
use karbon_server::secrets::{MemoryStore, Secret, SecretError, SecretKey, SecretStore};
use karbon_server::{db, router, AppState};
use rusqlite::Connection;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tower::ServiceExt;

const CA: &[u8] = include_bytes!("fixtures/integ_tls/ca.pem");
const LEAF: &[u8] = include_bytes!("fixtures/integ_tls/leaf.pem");
const LEAF_KEY: &[u8] = include_bytes!("fixtures/integ_tls/leaf.key");

/// Fake keys for the mock; never real credentials.
const GOOGLE_KEY: &str = "test-google-key-5521";
const SMARTY_ID: &str = "test-smarty-id-0042";
const SMARTY_TOKEN: &str = "test-smarty-token-9917";
const FULLCONTACT_KEY: &str = "test-fullcontact-key-3308";

const HOSTS: [&str; 3] = ["places.googleapis.com", "us-autocomplete-pro.api.smarty.com", "api.fullcontact.com"];

// ---------------------------------------------------------------------------------------------------------------
// The mock server
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Hit {
    method: String,
    /// Path and query.
    target: String,
    host: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Hit {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

type Answer = Arc<dyn Fn(&Hit) -> (u16, String) + Send + Sync>;

struct Mock {
    addr: SocketAddr,
    /// TCP connections accepted: 0 means nothing was sent at all.
    connections: Arc<AtomicUsize>,
    hits: Arc<Mutex<Vec<Hit>>>,
}

impl Mock {
    fn start(answer: impl Fn(&Hit) -> (u16, String) + Send + Sync + 'static) -> Self {
        let answer: Answer = Arc::new(answer);
        let config = server_config();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let hits = Arc::new(Mutex::new(Vec::new()));
        let (count, seen) = (connections.clone(), hits.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                count.fetch_add(1, Ordering::SeqCst);
                let (config, answer, seen) = (config.clone(), answer.clone(), seen.clone());
                std::thread::spawn(move || serve(stream, config, answer, seen));
            }
        });
        Mock { addr, connections, hits }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn hits(&self) -> Vec<Hit> {
        self.hits.lock().unwrap().clone()
    }

    /// A client that resolves every integration host to this mock.
    fn client(&self) -> HttpsClient {
        let resolve: Vec<(&str, SocketAddr)> = HOSTS.iter().map(|h| (*h, self.addr)).collect();
        HttpsClient::for_contract_tests(CA, &resolve)
    }
}

fn server_config() -> Arc<rustls::ServerConfig> {
    let certs = CertificateDer::pem_slice_iter(LEAF).collect::<Result<Vec<_>, _>>().unwrap();
    let key = PrivateKeyDer::from_pem_slice(LEAF_KEY).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
    )
}

/// One HTTP/1.1 exchange over TLS: read the request, record it, answer, close.
fn serve(tcp: TcpStream, config: Arc<rustls::ServerConfig>, answer: Answer, hits: Arc<Mutex<Vec<Hit>>>) {
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let conn = rustls::ServerConnection::new(config).unwrap();
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        match tls.read(&mut chunk) {
            Ok(0) | Err(_) => return, // no request (a refused TLS handshake ends here)
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (first.next().unwrap_or_default().to_owned(), first.next().unwrap_or_default().to_owned());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize =
        headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    while buf.len() < head_end + length {
        match tls.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
    let host = headers.iter().find(|(k, _)| k == "host").map(|(_, v)| v.clone()).unwrap_or_default();
    let hit = Hit { method, target, host, headers, body };
    let (status, reply) = answer(&hit);
    hits.lock().unwrap().push(hit);
    let response = format!(
        "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
        reply.len()
    );
    let _ = tls.write_all(response.as_bytes());
    tls.conn.send_close_notify();
    let _ = tls.flush();
}

/// A port nothing listens on: bound, then released.
fn closed_addr() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap()
}

// ---------------------------------------------------------------------------------------------------------------
// The app around it
// ---------------------------------------------------------------------------------------------------------------

/// The app log of this test binary, once (`applog` keeps one logger per process).
fn log_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("karbonfm-integ-contract-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        karbon_server::applog::init(&dir).unwrap();
        dir
    })
}

fn log_text() -> String {
    std::fs::read_to_string(log_dir().join("karbonfm.log")).unwrap_or_default()
}

/// A memory keychain that counts reads: an off integration must not read its key (AP-12).
#[derive(Default)]
struct CountingStore {
    inner: MemoryStore,
    reads: AtomicUsize,
}

impl SecretStore for CountingStore {
    fn get(&self, key: SecretKey) -> Result<Option<Secret>, SecretError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(key)
    }
    fn set(&self, key: SecretKey, value: &Secret) -> Result<(), SecretError> {
        self.inner.set(key, value)
    }
    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        self.inner.delete(key)
    }
}

struct App {
    router: axum::Router,
    store: Arc<CountingStore>,
    path: PathBuf,
    _tmp: tempfile::TempDir,
}

/// Every key saved, so only the switches decide.
const ALL_KEYS: [(SecretKey, &str); 4] = [
    (SecretKey::GooglePlacesApiKey, GOOGLE_KEY),
    (SecretKey::SmartyStreetsAuthId, SMARTY_ID),
    (SecretKey::SmartyStreetsApiKey, SMARTY_TOKEN),
    (SecretKey::FullContactApiKey, FULLCONTACT_KEY),
];

impl App {
    /// The small demo in a temp data folder, with the switches and keys given, calling through `client`.
    fn new(client: HttpsClient, provider: Option<&str>, fullcontact: bool, keys: &[(SecretKey, &str)]) -> Self {
        log_dir();
        let tmp = tempfile::tempdir().unwrap();
        let path = db::database_path(tmp.path());
        let conn = db::open_demo(Some(&path)).unwrap();
        conn.execute(
            "UPDATE system_setting SET address_suggestion_provider = ?1, fullcontact_enabled_ind = ?2",
            rusqlite::params![provider, i64::from(fullcontact)],
        )
        .unwrap();
        let store = Arc::new(CountingStore::default());
        for (k, v) in keys {
            store.set(*k, &Secret::new(*v)).unwrap();
        }
        let state = AppState::new(conn).with_secrets(store.clone()).with_data_dir(tmp.path()).with_http(client);
        App { router: router(state), store, path, _tmp: tmp }
    }

    fn reads(&self) -> usize {
        self.store.reads.load(Ordering::SeqCst)
    }

    async fn send(&self, method: &str, uri: &str, body: &str) -> (StatusCode, HeaderMap, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("HX-Request", "true")
            .body(Body::from(body.to_owned()))
            .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    async fn get(&self, uri: &str) -> (StatusCode, HeaderMap, String) {
        self.send("GET", uri, "").await
    }

    fn db(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }

    /// The person row and the party's contact methods, to show that a call changed nothing.
    fn snapshot(&self, party: &str) -> String {
        let db = self.db();
        let person: String = db
            .query_row(
                "SELECT COALESCE(job_title,'') || '|' || COALESCE(gender,'') || '|' || COALESCE(bio,'') || '|' ||
                        COALESCE(modification_timestamp,'') FROM person WHERE id = ?1",
                [party],
                |r| r.get(0),
            )
            .unwrap();
        let methods: i64 =
            db.query_row("SELECT COUNT(*) FROM contact_method WHERE party_id = ?1", [party], |r| r.get(0)).unwrap();
        let stamp: String = db
            .query_row("SELECT COALESCE(modification_timestamp,'') FROM party WHERE id = ?1", [party], |r| r.get(0))
            .unwrap();
        format!("{person}#{methods}#{stamp}")
    }
}

fn assert_no_key_in(text: &str) {
    for key in [GOOGLE_KEY, SMARTY_ID, SMARTY_TOKEN, FULLCONTACT_KEY] {
        assert!(!text.contains(key), "a key leaked: {key}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// T-08-02, T-08-12, BR-INTEG-02
// ---------------------------------------------------------------------------------------------------------------

/// T-08-02: an HTTP call to a host that is not enabled is refused, logged with its host only, and never sent: the
/// mock, which the host resolves to, sees no connection. The same call with the host enabled reaches it.
#[tokio::test]
async fn t_08_02_a_host_not_enabled_is_refused_logged_and_never_sent() {
    log_dir();
    let mock = Mock::start(|_| (200, "{}".into()));
    let client = mock.client();
    let only_fullcontact = OutboundPolicy::deny_all().enable(Integration::FullContact);
    let url = format!("https://places.googleapis.com/v1/places:autocomplete?key={GOOGLE_KEY}");
    let r = client.send(&only_fullcontact, HttpRequest::get(&url)).await;
    assert!(
        matches!(r, Err(HttpError::Refused(Refused::HostNotEnabled(ref h))) if h == "places.googleapis.com"),
        "{r:?}"
    );
    let r = client.send(&OutboundPolicy::deny_all(), HttpRequest::get("https://api.fullcontact.com/v3/x")).await;
    assert!(matches!(r, Err(HttpError::Refused(_))), "a fresh policy refuses everything");
    assert_eq!(mock.connections(), 0, "nothing was sent");
    let log = log_text();
    assert!(log.contains("outbound call refused: places.googleapis.com is not enabled"), "{log}");
    assert_no_key_in(&log);

    // Enabled: the same client reaches the mock, so the 0 above is the policy, not a dead mock.
    let r = client.send(&OutboundPolicy::deny_all().enable(Integration::GooglePlaces), HttpRequest::get(&url)).await;
    assert_eq!(r.unwrap().status, 200);
    assert_eq!(mock.connections(), 1);
    assert_eq!(mock.hits()[0].host, "places.googleapis.com");
}

/// T-08-12: an address suggestion with the integration off makes no network call and reads no key, even with every
/// key saved. The same holds for enrichment with FullContact off (AP-12).
#[tokio::test]
async fn t_08_12_address_suggestion_with_the_integration_off_makes_no_call() {
    let mock = Mock::start(|_| (200, r#"{"suggestions":[]}"#.into()));
    let app = App::new(mock.client(), None, false, &ALL_KEYS);
    let before = app.snapshot("p-hp-eh");
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore+Street").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Address suggestions are turned off. Turn them on in Settings."), "{html}");
    let (status, _, _) = app.get("/address-suggestions/pick?place_id=ChIJabc").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, html) = app.send("POST", "/parties/p-hp-eh/enrich", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("FullContact is turned off."), "{html}");
    assert_eq!(mock.connections(), 0, "no network call");
    assert_eq!(app.reads(), 0, "no key was read");
    assert_eq!(app.snapshot("p-hp-eh"), before, "nothing changed");
}

/// BR-INTEG-02: on, but without its key: no call ("no key -> no call").
#[tokio::test]
async fn an_integration_without_its_key_makes_no_call() {
    let mock = Mock::start(|_| (200, r#"{"suggestions":[]}"#.into()));
    let app = App::new(mock.client(), Some("SmartyStreets"), true, &[(SecretKey::SmartyStreetsApiKey, SMARTY_TOKEN)]);
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("No SmartyStreets auth ID is saved. Add it in Settings."), "{html}");
    let (status, _, html) = app.send("POST", "/parties/p-hp-eh/enrich", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("No FullContact API key is saved."), "{html}");
    assert_eq!(mock.connections(), 0);
}

/// BR-INTEG-02: TLS is verified. A server whose certificate the client does not trust gets a connection but no
/// request, and the user sees that the service could not be reached.
#[tokio::test]
async fn tls_is_verified_an_untrusted_certificate_gets_no_request() {
    let mock = Mock::start(|_| (200, r#"{"suggestions":[]}"#.into()));
    let resolve: Vec<(&str, SocketAddr)> = HOSTS.iter().map(|h| (*h, mock.addr)).collect();
    let untrusting = HttpsClient::for_contract_tests(b"", &resolve);
    let app = App::new(untrusting, Some("Google Places"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("could not be reached"), "{html}");
    assert!(mock.hits().is_empty(), "the request never left the TLS handshake");
    let log = log_text();
    assert!(log.contains("call to places.googleapis.com failed"), "{log}");
    assert_no_key_in(&log);
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-02 address_suggest: Google Places
// ---------------------------------------------------------------------------------------------------------------

const GOOGLE_AUTOCOMPLETE: &str = r#"{"suggestions":[
  {"placePrediction":{"placeId":"ChIJ-fore-12","text":{"text":"12 Fore Street, Portland, ME, USA"}}},
  {"placePrediction":{"placeId":"ChIJ_fore_120","text":{"text":"120 Fore Street, Portland, ME, USA"}}},
  {"queryPrediction":{"text":{"text":"fore street restaurants"}}}
]}"#;

const GOOGLE_DETAILS: &str = r#"{"addressComponents":[
  {"longText":"12","shortText":"12","types":["street_number"]},
  {"longText":"Fore Street","shortText":"Fore St","types":["route"]},
  {"longText":"Suite 3","shortText":"Suite 3","types":["subpremise"]},
  {"longText":"Portland","shortText":"Portland","types":["locality","political"]},
  {"longText":"Cumberland County","shortText":"Cumberland County","types":["administrative_area_level_2","political"]},
  {"longText":"Maine","shortText":"ME","types":["administrative_area_level_1","political"]},
  {"longText":"United States","shortText":"US","types":["country","political"]},
  {"longText":"04101","shortText":"04101","types":["postal_code"]},
  {"longText":"4423","shortText":"4423","types":["postal_code_suffix"]}
]}"#;

fn google(hit: &Hit) -> (u16, String) {
    if hit.method == "POST" && hit.target == "/v1/places:autocomplete" {
        (200, GOOGLE_AUTOCOMPLETE.into())
    } else if hit.method == "GET" && hit.target == "/v1/places/ChIJ-fore-12" {
        (200, GOOGLE_DETAILS.into())
    } else {
        (404, r#"{"error":{"code":404,"message":"Not found","status":"NOT_FOUND"}}"#.into())
    }
}

/// Google Places: the autocomplete call (Picker 198) and the details call of the pick (Picker 200), both to
/// places.googleapis.com with the key in a header; the pick fills the address fields and empties the list.
#[tokio::test]
async fn google_places_suggests_and_a_pick_fills_the_address() {
    let mock = Mock::start(google);
    let app = App::new(mock.client(), Some("Google Places"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=12%20Fore").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains(">12 Fore Street, Portland, ME, USA</button>"), "{html}");
    assert!(html.contains("/address-suggestions/pick?place_id=ChIJ-fore-12"), "{html}");
    assert!(html.contains("/address-suggestions/pick?place_id=ChIJ_fore_120"));
    assert!(!html.contains("restaurants"), "a query prediction is not an address");
    assert_no_key_in(&html);

    let hit = &mock.hits()[0];
    assert_eq!(
        (hit.method.as_str(), hit.target.as_str(), hit.host.as_str()),
        ("POST", "/v1/places:autocomplete", "places.googleapis.com")
    );
    assert_eq!(hit.header("x-goog-api-key"), Some(GOOGLE_KEY));
    assert_eq!(hit.json(), serde_json::json!({ "input": "12 Fore" }));

    let (status, _, html) = app.get("/address-suggestions/pick?place_id=ChIJ-fore-12").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    for (name, value) in [
        ("line1", "12 Fore Street"),
        ("line2", "Suite 3"),
        ("city", "Portland"),
        ("state", "ME"),
        ("postal_code", "04101-4423"),
        ("country", "US"),
    ] {
        assert!(html.contains(&format!(r#"name="{name}" value="{value}""#)), "{name}: {html}");
    }
    assert!(html.contains(r#"id="address-suggestions" hx-swap-oob="innerHTML""#), "the list is emptied");
    let hit = &mock.hits()[1];
    assert_eq!((hit.method.as_str(), hit.target.as_str()), ("GET", "/v1/places/ChIJ-fore-12"));
    assert_eq!(hit.header("x-goog-api-key"), Some(GOOGLE_KEY));
    assert_eq!(hit.header("x-goog-fieldmask"), Some("addressComponents"));
    assert!(mock.hits().iter().all(|h| h.host == "places.googleapis.com"), "Google calls its own host only");
}

/// An empty search makes no call (Picker 117 s50); a place id that could change the URL path is refused before a call.
#[tokio::test]
async fn an_empty_search_or_a_bad_place_id_makes_no_call() {
    let mock = Mock::start(google);
    let app = App::new(mock.client(), Some("Google Places"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=%20%20").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("<li>"));
    let (status, _, html) = app.get("/address-suggestions/pick?place_id=..%2Fplaces%3Fx%3D1").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Pick a suggestion from the list."), "{html}");
    let long = "x".repeat(129);
    let (status, _, html) = app.get(&format!("/address-suggestions?q={long}")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Type at most 128 characters."));
    assert_eq!(mock.connections(), 0);
}

/// A failed pick keeps the typed fields: the reason goes to the suggestion list (HX-Retarget), not over the fields.
#[tokio::test]
async fn a_failed_pick_leaves_the_fields_and_says_why() {
    let mock =
        Mock::start(|_| (500, r#"{"error":{"code":500,"message":"Internal error","status":"INTERNAL"}}"#.into()));
    let app = App::new(mock.client(), Some("Google Places"), false, &ALL_KEYS);
    let (status, headers, html) = app.get("/address-suggestions/pick?place_id=ChIJ-fore-12").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(headers["HX-Retarget"], "#address-suggestions");
    assert!(html.contains("Google Places refused the call: Internal error (HTTP 500)."), "{html}");
    assert!(!html.contains("address-fields"));
}

/// Google answers 403 for a bad key: the user is told to check the key; the log has the host and status only.
#[tokio::test]
async fn a_rejected_key_is_shown_and_logged_without_the_key() {
    let mock = Mock::start(|_| {
        (403, r#"{"error":{"code":403,"message":"API key not valid. Please pass a valid API key.","status":"PERMISSION_DENIED"}}"#.into())
    });
    let app = App::new(mock.client(), Some("Google Places"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        html.contains("Google Places refused the call: it did not accept the key. Check the key in Settings."),
        "{html}"
    );
    let log = log_text();
    assert!(log.contains("places.googleapis.com answered HTTP 403"), "{log}");
    assert_no_key_in(&log);
}

/// No answer (nothing listens) and an answer that is not JSON: each says so; no retry.
#[tokio::test]
async fn no_answer_or_a_bad_answer_is_an_error_without_retry() {
    let dead = closed_addr();
    let resolve: Vec<(&str, SocketAddr)> = HOSTS.iter().map(|h| (*h, dead)).collect();
    let app = App::new(HttpsClient::for_contract_tests(CA, &resolve), Some("Google Places"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("The service could not be reached. Check the connection and try again."), "{html}");

    let mock = Mock::start(|_| (200, "<html>not json</html>".into()));
    let app = App::new(mock.client(), Some("Google Places"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Google Places sent an answer KarbonFM cannot read."), "{html}");
    assert_eq!(mock.connections(), 1, "no retry");
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-02 address_suggest: SmartyStreets
// ---------------------------------------------------------------------------------------------------------------

const SMARTY: &str = r#"{"suggestions":[
  {"street_line":"12 Fore St","secondary":"Ste 3","city":"Portland","state":"ME","zipcode":"04101","entries":0},
  {"street_line":"120 Fore St","secondary":"","city":"Portland","state":"ME","zipcode":"04101","entries":0}
]}"#;

/// SmartyStreets (Picker 117): one GET to its host with the auth ID and token in the query; a suggestion carries
/// its address, so the pick makes no second call.
#[tokio::test]
async fn smartystreets_suggests_and_a_pick_needs_no_second_call() {
    let mock =
        Mock::start(|hit| if hit.target.starts_with("/lookup?") { (200, SMARTY.into()) } else { (404, "{}".into()) });
    let app = App::new(mock.client(), Some("SmartyStreets"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=12%20Fore%20%26%20Co").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains(">12 Fore St Ste 3, Portland ME 04101</button>"), "{html}");
    assert!(html.contains(">120 Fore St, Portland ME 04101</button>"));
    assert_no_key_in(&html);
    let hit = &mock.hits()[0];
    assert_eq!(hit.host, "us-autocomplete-pro.api.smarty.com");
    assert_eq!(
        hit.target,
        format!("/lookup?search=12%20Fore%20%26%20Co&auth-id={SMARTY_ID}&auth-token={SMARTY_TOKEN}")
    );

    let pick = "line1=12%20Fore%20St&line2=Ste%203&city=Portland&state=ME&postal_code=04101&country=US";
    assert!(html.contains(&format!("/address-suggestions/pick?{}", pick.replace('&', "&#38;"))), "{html}");
    let (status, _, html) = app.get(&format!("/address-suggestions/pick?{pick}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains(r#"name="line1" value="12 Fore St""#) && html.contains(r#"name="postal_code" value="04101""#)
    );
    assert!(html.contains("Filled in from SmartyStreets."));
    assert_eq!(mock.connections(), 1, "the pick made no call");
    // The log never has the auth ID or token, which travel in the query.
    assert_no_key_in(&log_text());
}

/// SmartyStreets answers 401 for bad credentials.
#[tokio::test]
async fn smartystreets_bad_credentials() {
    let mock = Mock::start(|_| (401, r#"{"errors":[{"id":1,"message":"Unauthorized"}]}"#.into()));
    let app = App::new(mock.client(), Some("SmartyStreets"), false, &ALL_KEYS);
    let (status, _, html) = app.get("/address-suggestions?q=12+Fore").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("SmartyStreets refused the call: it did not accept the key."), "{html}");
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-03 party_enrich: FullContact
// ---------------------------------------------------------------------------------------------------------------

const ELENA: &str = r#"{
  "fullName": "Elena Halden",
  "gender": "Female",
  "title": "Principal Architect",
  "bio": "Architect in Portland, Maine.",
  "linkedin": "https://www.linkedin.com/in/elena-halden-example",
  "twitter": "https://twitter.com/elenahalden_example",
  "facebook": "",
  "avatar": "https://img.fullcontact.com/static/example"
}"#;

/// FullContact (kontroller 746, kontacts 865): a POST to person.enrich with a Bearer key and the person's names,
/// emails and phones (E.164). The answer is a preview that changes nothing; Keep writes the checked values in one
/// transaction, with the stamps, and the links as web addresses. A second Keep adds no duplicate.
#[tokio::test]
async fn fullcontact_previews_then_keeps_only_what_was_checked() {
    let mock = Mock::start(|hit| {
        if hit.method == "POST" && hit.target == "/v3/person.enrich" {
            (200, ELENA.into())
        } else {
            (404, "{}".into())
        }
    });
    let app = App::new(mock.client(), None, true, &ALL_KEYS);
    let before = app.snapshot("p-hp-eh");
    let (status, _, html) = app.send("POST", "/parties/p-hp-eh/enrich", "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("FullContact: Elena Halden"));
    assert!(html.contains("Principal Architect") && html.contains("Now: Principal"), "{html}");
    assert!(
        html.contains(r#"name="keep_job_title" class="checkbox checkbox-sm mt-0.5" >"#),
        "a value the party has is unchecked: {html}"
    );
    assert!(html.contains(r#"name="keep_gender" class="checkbox checkbox-sm mt-0.5" checked"#), "{html}");
    assert!(html.contains("https://www.linkedin.com/in/elena-halden-example"));
    assert!(!html.contains("keep_facebook"), "an empty value is not offered");
    assert!(!html.contains("img.fullcontact.com"), "the avatar is on another host and is not fetched");
    assert_eq!(app.snapshot("p-hp-eh"), before, "the call changed nothing");

    let hit = &mock.hits()[0];
    assert_eq!(hit.host, "api.fullcontact.com");
    assert_eq!(hit.header("authorization"), Some(format!("Bearer {FULLCONTACT_KEY}").as_str()));
    assert_eq!(
        hit.json(),
        serde_json::json!({
            "emails": ["elena@haldenpryce.example"],
            "phones": ["+12075550148"],
            "name": { "given": "Elena", "family": "Halden", "full": "Elena Halden" }
        })
    );
    assert_eq!(mock.connections(), 1);

    let form = "keep_job_title=&job_title=Principal+Architect&keep_gender=on&gender=Female&keep_bio=on&bio=Architect+in+Portland%2C+Maine.\
                &keep_linkedin=on&linkedin=https%3A%2F%2Fwww.linkedin.com%2Fin%2Felena-halden-example\
                &keep_twitter=&twitter=https%3A%2F%2Ftwitter.com%2Felenahalden_example";
    let (status, headers, html) = app.send("POST", "/parties/p-hp-eh/enrich/apply", form).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("Kept 3 values from FullContact"));
    assert!(html.contains(r#"id="party-header""#) && html.contains("hx-swap-oob"));
    assert_eq!(mock.connections(), 1, "Keep makes no call");
    let db = app.db();
    let (job, gender, bio, stamp): (String, String, String, String) = db
        .query_row("SELECT job_title, gender, bio, modification_timestamp FROM person WHERE id = 'p-hp-eh'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert_eq!((job.as_str(), gender.as_str(), bio.as_str()), ("Principal", "Female", "Architect in Portland, Maine."));
    assert!(karbon_server::audit::is_timestamp(&stamp), "BR-PARTY-07 stamp: {stamp}");
    let links: Vec<(String, String, i64)> = db
        .prepare(
            "SELECT n.address, cm.label, COALESCE(n.primary_ind, 0) FROM contact_method cm JOIN network_address n ON n.id = cm.id
             WHERE cm.party_id = 'p-hp-eh' AND COALESCE(n.email_address_ind, 0) = 0",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    // Elena had no web address, so this first one is primary (BR-CONTACT-01: the first of a kind is primary).
    assert_eq!(links, [("https://www.linkedin.com/in/elena-halden-example".to_owned(), "LinkedIn".to_owned(), 1)]);

    // Again: the LinkedIn link is there already, so nothing is added; a new preview no longer offers it.
    let (status, headers, _) = app
        .send(
            "POST",
            "/parties/p-hp-eh/enrich/apply",
            "keep_linkedin=on&linkedin=https%3A%2F%2Flinkedin.com%2Fin%2Felena-halden-example%2F",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["HX-Trigger"].to_str().unwrap().contains("Nothing changed"));
    let (_, _, html) = app.send("POST", "/parties/p-hp-eh/enrich", "").await;
    assert!(!html.contains("keep_linkedin") && html.contains("keep_twitter"), "{html}");
}

/// No match (404) and no answer yet (202): "no match", nothing changed.
#[tokio::test]
async fn fullcontact_no_match_changes_nothing() {
    for status in [404, 202] {
        let mock = Mock::start(move |_| (status, r#"{"status":404,"message":"Profile not found"}"#.into()));
        let app = App::new(mock.client(), None, true, &ALL_KEYS);
        let before = app.snapshot("p-hp-eh");
        let (code, _, html) = app.send("POST", "/parties/p-hp-eh/enrich", "").await;
        assert_eq!(code, StatusCode::OK);
        assert!(html.contains("FullContact has no match for Elena Halden. Nothing was changed."), "{status}: {html}");
        assert_eq!(app.snapshot("p-hp-eh"), before);
    }
}

/// A rejected key (401) and a quota error (429) are shown; nothing changed.
#[tokio::test]
async fn fullcontact_errors_are_shown_and_change_nothing() {
    for (status, text) in [
        (401, "FullContact refused the call: it did not accept the key."),
        (429, "FullContact refused the call: too many calls."),
    ] {
        let mock = Mock::start(move |_| (status, r#"{"status":401,"message":"Invalid key"}"#.into()));
        let app = App::new(mock.client(), None, true, &ALL_KEYS);
        let before = app.snapshot("p-hp-eh");
        let (code, _, html) = app.send("POST", "/parties/p-hp-eh/enrich", "").await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(html.contains(text) && html.contains("Nothing was changed."), "{status}: {html}");
        assert_eq!(app.snapshot("p-hp-eh"), before);
        assert_no_key_in(&html);
    }
    assert_no_key_in(&log_text());
}

/// kontacts 865 s3-7: an organization is not looked up, nor a person without an email or phone; no call.
#[tokio::test]
async fn fullcontact_needs_a_person_with_an_email_or_phone() {
    let mock = Mock::start(|_| (200, ELENA.into()));
    let app = App::new(mock.client(), None, true, &ALL_KEYS);
    let (status, _, html) = app.send("POST", "/parties/p-hp/enrich", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("Enrichment looks up people; this party is an organization."), "{html}");
    app.db()
        .execute_batch(
            "INSERT INTO party (id, name_cached, organization_ind, person_ind, deleted_ind) VALUES ('p-test-nora', 'Nora Blank', 0, 1, 0);
             INSERT INTO person (id, first_name, last_name) VALUES ('p-test-nora', 'Nora', 'Blank');",
        )
        .unwrap();
    let (status, _, html) = app.send("POST", "/parties/p-test-nora/enrich", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(html.contains("FullContact needs the person&#39;s name and an email or a phone."), "{html}");
    let (status, _, _) = app.send("POST", "/parties/p-nobody/enrich", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(mock.connections(), 0);
}
