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
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
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

/// True for a request made by htmx, which wants a fragment instead of a full page.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.get("hx-request").is_some_and(|v| v == "true")
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
                eprintln!("internal error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into_response()
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
