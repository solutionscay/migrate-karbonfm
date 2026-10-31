//! Outbound network policy of AP-02 and AP-12: default deny.
//!
//! The app makes outbound calls only from Rust, and only to the SMTP host the user set and to the integration hosts
//! the user enabled in settings. Every client (the SMTP sender, the HTTPS client of I6) asks [`OutboundPolicy`] first;
//! a refused call is written to the app log (warn, host only) and never sent. This is the stub the later integrations plug into: there
//! is no HTTP client yet.

use crate::applog::{log_write, Entry, Level};
use axum::http::Uri;
use std::collections::BTreeSet;
use std::fmt;

/// The optional integrations of 05 (EXT-02, EXT-03) and the one host each may call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Integration {
    GooglePlaces,
    SmartyStreets,
    FullContact,
}

impl Integration {
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
    fn refusal_text_has_no_query() {
        let p = OutboundPolicy::deny_all();
        let e = p.check_https("https://api.example.com/x?key=SECRET123").unwrap_err();
        assert_eq!(e.to_string(), "outbound call refused: api.example.com is not enabled");
    }
}
