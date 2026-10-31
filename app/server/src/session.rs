//! Per-launch session of AP-13: only the app window may use the local server.
//!
//! At start the shell creates a [`Session`] and opens the window on its one-time launch path,
//! `/session/{token}`. That route sets an HttpOnly, SameSite=Strict cookie and redirects to `/`; the token then stops
//! working. Every other route except the static files needs the cookie ([`crate::web::origin_guard`]), so another
//! local process or a page in a browser, which never saw the token, gets 401.

use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::sync::Mutex;

/// The launch token and the session cookie of one run of the server. Both are 256-bit random values from the OS.
pub struct Session {
    cookie_name: String,
    cookie_value: String,
    launch_token: Mutex<Option<String>>,
}

impl Session {
    /// A new session for the server on `port`. The cookie name carries the port: cookies are not separated by port,
    /// so two servers on 127.0.0.1 (the app and a dev server in one browser) would otherwise overwrite each other.
    pub fn new(port: u16) -> Self {
        Self {
            cookie_name: format!("karbon_session_{port}"),
            cookie_value: random_hex(),
            launch_token: Mutex::new(Some(random_hex())),
        }
    }

    /// `/session/{token}` while the token is unused, for the window (or the dev server's console) to open.
    pub fn launch_path(&self) -> Option<String> {
        self.launch_token.lock().unwrap().as_ref().map(|t| format!("/session/{t}"))
    }

    /// True if the request carries this session's cookie.
    pub fn is_valid(&self, headers: &HeaderMap) -> bool {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .any(|(name, value)| name == self.cookie_name && constant_time_eq(value, &self.cookie_value))
    }

    /// Spends the launch token. True only the first time the right token is given.
    fn redeem(&self, token: &str) -> bool {
        let mut slot = self.launch_token.lock().unwrap();
        match slot.as_deref() {
            Some(t) if constant_time_eq(t, token) => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    fn set_cookie(&self) -> HeaderValue {
        // No Max-Age: the cookie ends with the window; the next launch has a new value anyway.
        let cookie = format!("{}={}; Path=/; HttpOnly; SameSite=Strict", self.cookie_name, self.cookie_value);
        HeaderValue::from_str(&cookie).expect("hex cookie is a valid header value")
    }
}

/// The only route that works without the cookie (with the static files).
pub fn router() -> Router<AppState> {
    Router::new().route("/session/{token}", get(launch))
}

/// Sets the session cookie and redirects to the dashboard. A spent or wrong token gets 403, unless the request already
/// has the cookie (a reload of the launch URL in the dev browser), which just redirects.
async fn launch(State(state): State<AppState>, Path(token): Path<String>, headers: HeaderMap) -> Response {
    let Some(session) = &state.session else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let redirect = [(header::LOCATION, HeaderValue::from_static("/"))];
    if session.redeem(&token) {
        let mut res = (StatusCode::SEE_OTHER, redirect).into_response();
        res.headers_mut().insert(header::SET_COOKIE, session.set_cookie());
        res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        res
    } else if session.is_valid(&headers) {
        (StatusCode::SEE_OTHER, redirect).into_response()
    } else {
        (StatusCode::FORBIDDEN, "This link has been used. Restart the app or the dev server.").into_response()
    }
}

fn random_hex() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source is available");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares without stopping at the first differing byte, so the time taken tells nothing about the secret.
fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(s: &Session, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(&format!("other=1; {}={value}", s.cookie_name)).unwrap());
        h
    }

    #[test]
    fn token_works_once() {
        let s = Session::new(4321);
        let path = s.launch_path().unwrap();
        let token = path.strip_prefix("/session/").unwrap().to_owned();
        assert_eq!(token.len(), 64);
        assert!(!s.redeem("0".repeat(64).as_str()));
        assert!(s.redeem(&token));
        assert!(!s.redeem(&token), "spent");
        assert_eq!(s.launch_path(), None);
    }

    #[test]
    fn cookie_must_match_name_and_value() {
        let s = Session::new(4321);
        assert!(s.is_valid(&cookie(&s, &s.cookie_value.clone())));
        assert!(!s.is_valid(&cookie(&s, "f00")));
        assert!(!s.is_valid(&HeaderMap::new()));
        let other_port = Session::new(4322);
        assert!(!other_port.is_valid(&cookie(&s, &s.cookie_value.clone())));
        let set = s.set_cookie();
        let set = set.to_str().unwrap();
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Strict") && set.starts_with("karbon_session_4321="));
    }

    #[test]
    fn sessions_differ_per_launch() {
        let (a, b) = (Session::new(1), Session::new(1));
        assert_ne!(a.cookie_value, b.cookie_value);
        assert_ne!(a.launch_path(), b.launch_path());
    }
}
