//! Response helpers shared by the routes: template rendering, htmx detection, errors, security headers.

use crate::AppState;
use askama::Template;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};

/// Pages load scripts, styles and fonts from this server only. Alpine.js (standard build) evaluates its
/// attributes with `new Function`, hence 'unsafe-eval'; DaisyUI, htmx and Alpine set inline styles.
const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; \
                   img-src 'self' data:; font-src 'self'; connect-src 'self'; form-action 'self'; \
                   base-uri 'none'; frame-ancestors 'none'; object-src 'none'";

pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    // A route may set its own policy (the invoice PDF, which the invoice page frames in its preview).
    h.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static(CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    res
}

/// The local server guard of AP-13, in this order:
/// 1. Host: every request must be addressed to this server by its 127.0.0.1 address (no DNS rebinding): 421.
/// 2. Session: every route except the launch route and the static files needs the per-launch session cookie
///    ([`crate::session`]), which only the app window has: 401.
/// 3. Origin: a request that is not GET or HEAD must come from a page of this server (no cross-site POST from a page
///    in another browser): 403.
///
/// Each check is on when its part of the state is set (`serve` sets both; route tests may leave them off).
pub async fn origin_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(origin) = &state.origin {
        let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok());
        if host != origin.strip_prefix("http://") {
            return (StatusCode::MISDIRECTED_REQUEST, "Wrong host").into_response();
        }
    }
    if let Some(session) = &state.session {
        let path = req.uri().path();
        let open = path.starts_with("/session/") || path.starts_with("/assets/");
        if !open && !session.is_valid(req.headers()) {
            return (StatusCode::UNAUTHORIZED, "Open KarbonFM from the app.").into_response();
        }
    }
    if let Some(origin) = &state.origin {
        let safe = req.method() == axum::http::Method::GET || req.method() == axum::http::Method::HEAD;
        let from = req.headers().get(header::ORIGIN).and_then(|h| h.to_str().ok());
        if !safe && from != Some(origin.as_str()) {
            return (StatusCode::FORBIDDEN, "Cross-origin request refused").into_response();
        }
    }
    next.run(req).await
}

/// The error behind a 5xx response, for [`log_errors`]. The page only gets a generic message (D-25).
#[derive(Clone, Debug)]
pub struct ErrorDetail {
    pub code: &'static str,
    pub message: String,
}

/// Writes a log line (OP-ADMIN-05) for every response that carries an [`ErrorDetail`], with the route as the
/// operation. The message is masked by the log (AP-03, D-30).
pub async fn log_errors(req: Request, next: Next) -> Response {
    let operation = format!("{} {}", req.method(), req.uri().path());
    let res = next.run(req).await;
    if let Some(e) = res.extensions().get::<ErrorDetail>() {
        crate::applog::log_write(&crate::applog::Entry::error(&operation, e.code, &e.message));
    }
    res
}

/// True for a request made by htmx, which wants a fragment instead of a full page.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.get("hx-request").is_some_and(|v| v == "true")
}

/// Percent-encodes a query-string value.
pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn render(t: &impl Template) -> Result<Html<String>, AppError> {
    Ok(Html(t.render()?))
}

/// An error rendered as a plain message (I1 renders typed error fragments, D-25).
#[derive(Debug)]
pub enum AppError {
    NotFound,
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
            AppError::Internal(msg) => {
                let mut res = (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into_response();
                res.extensions_mut().insert(ErrorDetail { code: "Internal", message: msg });
                res
            }
        }
    }
}

impl From<askama::Error> for AppError {
    fn from(e: askama::Error) -> Self {
        AppError::Internal(format!("template: {e}"))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound,
            e => AppError::Internal(format!("database: {e}")),
        }
    }
}

/// A url-encoded form as its (name, value) pairs, for forms with a repeated field (checkbox lists): axum's `Form`
/// keeps one value per name. The handler names the fields it accepts ([`FormPairs::only`]): anything else is refused
/// with 422, as `deny_unknown_fields` does for the typed forms (05).
#[derive(Debug, Default)]
pub struct FormPairs(pub Vec<(String, String)>);

impl FormPairs {
    pub fn parse(body: &str) -> Self {
        FormPairs(
            body.split('&')
                .filter(|p| !p.is_empty())
                .map(|p| {
                    let (k, v) = p.split_once('=').unwrap_or((p, ""));
                    (form_decode(k), form_decode(v))
                })
                .collect(),
        )
    }
    /// The first value of `name`, or "".
    pub fn one(&self, name: &str) -> String {
        self.0.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default()
    }
    /// Every value of `name`, in order.
    pub fn all(&self, name: &str) -> Vec<String> {
        self.0.iter().filter(|(k, _)| k == name).map(|(_, v)| v.clone()).collect()
    }
    /// 422 when the form has a field not in `names`.
    pub fn only(&self, names: &[&str]) -> Result<(), Box<axum::response::Response>> {
        match self.0.iter().find(|(k, _)| !names.contains(&k.as_str())) {
            None => Ok(()),
            Some((k, _)) => {
                Err(Box::new((StatusCode::UNPROCESSABLE_ENTITY, format!("unknown field `{k}`")).into_response()))
            }
        }
    }
}

impl<S: Send + Sync> axum::extract::FromRequest<S> for FormPairs {
    type Rejection = axum::response::Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let form = req
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
        if !form {
            return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "expected a form").into_response());
        }
        let body = String::from_request(req, state).await.map_err(IntoResponse::into_response)?;
        Ok(FormPairs::parse(&body))
    }
}

/// Decodes one url-encoded form component: `+` is a space, `%XX` a byte; invalid UTF-8 is replaced.
fn form_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod form_tests {
    use super::FormPairs;

    #[test]
    fn repeated_fields_and_decoding() {
        let f = FormPairs::parse("subscriber=p-1&subscriber=p%2D2&notes=Hello+world%21&empty=&active=on&x=%E2%9C%93");
        assert_eq!(f.all("subscriber"), ["p-1", "p-2"]);
        assert_eq!(f.one("notes"), "Hello world!");
        assert_eq!(f.one("empty"), "");
        assert_eq!(f.one("x"), "✓");
        assert_eq!(f.one("missing"), "");
        assert!(f.only(&["subscriber", "notes", "empty", "active", "x"]).is_ok());
        assert!(f.only(&["subscriber"]).is_err());
        assert_eq!(FormPairs::parse("a=%zz&b=%4").one("a"), "%zz");
        assert_eq!(FormPairs::parse("b=%4").one("b"), "%4");
    }
}
