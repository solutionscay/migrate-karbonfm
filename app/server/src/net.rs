//! Outbound network policy of AP-02 and AP-12: default deny.
//!
//! The app makes outbound calls only from Rust, and only to the SMTP host the user set and to the integration hosts
//! the user enabled in settings. Every client (the SMTP sender, the HTTPS client of I6) asks [`OutboundPolicy`] first;
//! a refused call is written to the app log (warn, host only) and never sent.
//!
//! [`HttpsClient`] is the one HTTP client of the app (EXT-02, EXT-03; BR-INTEG-02): `https` only, certificates verified
//! against the webpki roots (rustls with the ring provider, as lettre), no redirects (a redirect could leave the
//! allowed host), no proxy from the environment, short timeouts and a capped response body. [`HttpsClient::send`]
//! takes the policy of the call and checks the URL before anything leaves the computer.

use crate::applog::{log_write, Entry, Level};
use axum::http::Uri;
use std::collections::BTreeSet;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// The optional integrations of 05 (EXT-02, EXT-03) and the one host each may call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Integration {
    GooglePlaces,
    SmartyStreets,
    FullContact,
}

impl Integration {
    pub const ALL: [Integration; 3] = [Integration::GooglePlaces, Integration::SmartyStreets, Integration::FullContact];

    pub fn label(self) -> &'static str {
        match self {
            Integration::GooglePlaces => "Google Places",
            Integration::SmartyStreets => "SmartyStreets",
            Integration::FullContact => "FullContact",
        }
    }

    pub fn host(self) -> &'static str {
        match self {
            Integration::GooglePlaces => "places.googleapis.com",
            Integration::SmartyStreets => "us-autocomplete-pro.api.smarty.com",
            Integration::FullContact => "api.fullcontact.com",
        }
    }
}

/// A refused call. Its text has the host only: a URL may carry a key in its query.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not an `https://host/...` URL with a host name (no user info, no plain http).
    BadUrl,
    HostNotEnabled(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::BadUrl => f.write_str("outbound call refused: not an https URL"),
            Refused::HostNotEnabled(h) => write!(f, "outbound call refused: {h} is not enabled"),
        }
    }
}

impl std::error::Error for Refused {}

/// The hosts the app may call. Empty by default: a fresh install calls nothing.
#[derive(Clone, Debug, Default)]
pub struct OutboundPolicy {
    https: BTreeSet<String>,
    smtp: Option<String>,
}

impl OutboundPolicy {
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Allows the host of an integration the user enabled (AP-12).
    pub fn enable(mut self, integration: Integration) -> Self {
        self.https.insert(integration.host().to_owned());
        self
    }

    /// Allows the SMTP host from the user's settings.
    pub fn smtp_host(mut self, host: &str) -> Self {
        self.smtp = Some(host.trim().to_ascii_lowercase());
        self
    }

    /// Checks an HTTPS URL before the client sends it. Exact host match: no subdomains, no IP literals unless listed.
    pub fn check_https(&self, url: &str) -> Result<(), Refused> {
        let result = (|| {
            let uri: Uri = url.parse().map_err(|_| Refused::BadUrl)?;
            let authority = uri.authority().ok_or(Refused::BadUrl)?;
            if uri.scheme_str() != Some("https") || authority.as_str().contains('@') {
                return Err(Refused::BadUrl);
            }
            let host = authority.host().to_ascii_lowercase();
            if self.https.contains(&host) {
                Ok(())
            } else {
                Err(Refused::HostNotEnabled(host))
            }
        })();
        log_refusal("outbound https", &result);
        result
    }

    /// Checks the SMTP host before the sender connects.
    pub fn check_smtp(&self, host: &str) -> Result<(), Refused> {
        let host = host.trim().to_ascii_lowercase();
        let result =
            if self.smtp.as_deref() == Some(host.as_str()) { Ok(()) } else { Err(Refused::HostNotEnabled(host)) };
        log_refusal("outbound smtp", &result);
        result
    }
}

/// The host of a URL, for a log line or an error: never the path or the query (a query may carry a key, AP-03).
pub fn host_of(url: &str) -> String {
    url.parse::<Uri>().ok().and_then(|u| u.host().map(str::to_ascii_lowercase)).unwrap_or_else(|| "?".into())
}

/// The longest response body the client reads (address suggestions and an enrichment answer are a few KB).
pub const MAX_BODY: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// An outbound HTTPS request. Headers may carry a key (`Authorization`, `X-Goog-Api-Key`); they are never logged.
#[derive(Debug)]
pub struct HttpRequest {
    pub post: bool,
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub json: Option<serde_json::Value>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self { post: false, url: url.into(), headers: Vec::new(), json: None }
    }
    pub fn post_json(url: impl Into<String>, body: serde_json::Value) -> Self {
        Self { post: true, url: url.into(), headers: Vec::new(), json: Some(body) }
    }
    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

/// The answer of a call that was sent: any HTTP status (the caller decides what a 404 means) and the body.
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Why a call has no answer. The text has the host at most, never the URL: a query may carry a key.
#[derive(Debug)]
pub enum HttpError {
    /// The policy refused it: nothing was sent (AP-02, T-08-02).
    Refused(Refused),
    /// Sent, or tried, and failed: no connection, TLS refused, timeout, a body over [`MAX_BODY`].
    Failed(String),
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::Refused(r) => r.fmt(f),
            HttpError::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for HttpError {}

/// The app's HTTPS client. Clone it freely (it shares one connection pool).
#[derive(Clone)]
pub struct HttpsClient {
    client: reqwest::Client,
}

impl std::fmt::Debug for HttpsClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HttpsClient")
    }
}

impl Default for HttpsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpsClient {
    /// The client of the app: the webpki roots, nothing else trusted.
    pub fn new() -> Self {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self::build(roots, &[])
    }

    /// For the contract tests only (`tests/integrations_contract.rs`): trust only `ca_pem` (the test CA, no public
    /// root) and resolve each `(host, addr)` to a mock server on 127.0.0.1. URLs, the policy, TLS verification and
    /// every other setting are those of [`HttpsClient::new`]; nothing in the app calls this.
    pub fn for_contract_tests(ca_pem: &[u8], resolve: &[(&str, SocketAddr)]) -> Self {
        use rustls::pki_types::{pem::PemObject, CertificateDer};
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(ca_pem) {
            roots.add(cert.expect("test CA PEM")).expect("test CA certificate");
        }
        Self::build(roots, resolve)
    }

    fn build(roots: rustls::RootCertStore, resolve: &[(&str, SocketAddr)]) -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("ring supports TLS 1.2 and 1.3")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let mut builder = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("KarbonFM/", env!("CARGO_PKG_VERSION")));
        for (host, addr) in resolve {
            builder = builder.resolve(host, *addr);
        }
        Self { client: builder.build().expect("the HTTPS client builds with a rustls config") }
    }

    /// Sends `req` if `policy` allows its URL (exact host, https). A refusal is logged by the policy and nothing is
    /// sent; a failure is logged at warn with the host only. No retry (05 EXT-02, EXT-03).
    pub async fn send(&self, policy: &OutboundPolicy, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        policy.check_https(&req.url).map_err(HttpError::Refused)?;
        let host = host_of(&req.url);
        let result = self.send_checked(req).await;
        if let Err(HttpError::Failed(message)) = &result {
            log_write(&Entry {
                level: Level::Warn,
                operation: "outbound https",
                code: Some("Integration"),
                message: &format!("call to {host} failed: {message}"),
                payload: None,
            });
        }
        result
    }

    async fn send_checked(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        let mut builder = if req.post { self.client.post(&req.url) } else { self.client.get(&req.url) };
        builder = builder.header("Accept", "application/json");
        for (name, value) in &req.headers {
            builder = builder.header(*name, value);
        }
        if let Some(json) = &req.json {
            builder = builder.json(json);
        }
        let failed = |e: reqwest::Error| {
            let kind = if e.is_timeout() {
                "timed out"
            } else if e.is_connect() {
                "could not connect"
            } else {
                "request failed"
            };
            // without_url: reqwest's text would carry the URL and its query; the causes (connect, TLS, IO) have none.
            let e = e.without_url();
            let mut text = e.to_string();
            let mut source = std::error::Error::source(&e);
            while let Some(cause) = source {
                text.push_str(": ");
                text.push_str(&cause.to_string());
                source = cause.source();
            }
            let text: String = text.chars().take(240).collect();
            HttpError::Failed(format!("{kind} ({text})"))
        };
        let mut res = builder.send().await.map_err(failed)?;
        let status = res.status().as_u16();
        let mut body = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(failed)? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(HttpError::Failed("the answer is too large".into()));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse { status, body })
    }
}

/// Writes a refusal to the app log (OP-ADMIN-05, BR-ADMIN-03) at warn level with code Integration (05, D-25). The
/// message names the host only, never the URL path or query, and `log_write` masks it again (AP-03).
fn log_refusal(operation: &str, result: &Result<(), Refused>) {
    if let Err(e) = result {
        let message = e.to_string();
        log_write(&Entry {
            level: Level::Warn,
            operation,
            code: Some("Integration"),
            message: &message,
            payload: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_denies_everything() {
        let p = OutboundPolicy::deny_all();
        assert_eq!(
            p.check_https("https://places.googleapis.com/v1/places:autocomplete"),
            Err(Refused::HostNotEnabled("places.googleapis.com".into()))
        );
        assert!(p.check_smtp("smtp.example.com").is_err());
    }

    #[test]
    fn enabled_hosts_only_over_https() {
        let p = OutboundPolicy::deny_all().enable(Integration::FullContact).smtp_host("SMTP.Example.com ");
        assert_eq!(p.check_https("https://api.fullcontact.com/v3/person.enrich"), Ok(()));
        assert_eq!(p.check_https("https://API.FullContact.com/v3/person.enrich"), Ok(()));
        assert_eq!(p.check_https("http://api.fullcontact.com/v3/person.enrich"), Err(Refused::BadUrl));
        assert_eq!(p.check_https("https://user@api.fullcontact.com/"), Err(Refused::BadUrl));
        assert_eq!(p.check_https("not a url"), Err(Refused::BadUrl));
        assert_eq!(
            p.check_https("https://evil.api.fullcontact.com.example/"),
            Err(Refused::HostNotEnabled("evil.api.fullcontact.com.example".into()))
        );
        assert!(p.check_https("https://places.googleapis.com/").is_err(), "Google Places not enabled");
        assert_eq!(p.check_smtp("smtp.example.com"), Ok(()));
        assert!(p.check_smtp("smtp.other.com").is_err());
    }

    #[test]
    fn host_of_drops_path_and_query() {
        assert_eq!(host_of("https://API.fullcontact.com/v3/person.enrich?key=SECRET"), "api.fullcontact.com");
        assert_eq!(host_of("not a url"), "?");
    }

    /// T-08-02 at the client: a host the policy does not allow is refused before reqwest sees the request (the URL
    /// points at a port nothing listens on, so a send would fail with Failed, not Refused).
    #[tokio::test]
    async fn the_client_refuses_before_sending() {
        let client = HttpsClient::new();
        let policy = OutboundPolicy::deny_all().enable(Integration::FullContact);
        let r = client.send(&policy, HttpRequest::get("https://places.googleapis.com/v1/places:autocomplete")).await;
        assert!(matches!(r, Err(HttpError::Refused(Refused::HostNotEnabled(ref h))) if h == "places.googleapis.com"));
        let r = client.send(&policy, HttpRequest::get("http://api.fullcontact.com/v3/person.enrich")).await;
        assert!(matches!(r, Err(HttpError::Refused(Refused::BadUrl))), "{r:?}");
    }

    #[test]
    fn refusal_text_has_no_query() {
        let p = OutboundPolicy::deny_all();
        let e = p.check_https("https://api.example.com/x?key=SECRET123").unwrap_err();
        assert_eq!(e.to_string(), "outbound call refused: api.example.com is not enabled");
    }
}
