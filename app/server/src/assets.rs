//! Static files, embedded in the binary at build time (the app works offline; no CDN).

use crate::AppState;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

/// (path under /assets/, content type, bytes). Vendored files carry their version in the name.
const FILES: &[(&str, &str, &[u8])] = &[
    ("app.css", "text/css; charset=utf-8", include_bytes!("../assets/app.css")),
    ("invoice.css", "text/css; charset=utf-8", include_bytes!("../assets/invoice.css")),
    ("app.js", "text/javascript; charset=utf-8", include_bytes!("../assets/app.js")),
    ("theme-init.js", "text/javascript; charset=utf-8", include_bytes!("../assets/theme-init.js")),
    (
        "vendor/htmx-2.0.4.min.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../assets/vendor/htmx-2.0.4.min.js"),
    ),
    (
        "vendor/alpine-3.14.8.min.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../assets/vendor/alpine-3.14.8.min.js"),
    ),
    (
        "vendor/chart-4.4.7.umd.min.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../assets/vendor/chart-4.4.7.umd.min.js"),
    ),
    ("vendor/fonts/Geist-Variable.woff2", "font/woff2", include_bytes!("../assets/vendor/fonts/Geist-Variable.woff2")),
    (
        "vendor/fonts/GeistMono-Variable.woff2",
        "font/woff2",
        include_bytes!("../assets/vendor/fonts/GeistMono-Variable.woff2"),
    ),
];

pub fn router() -> Router<AppState> {
    Router::new().route("/assets/{*path}", get(asset))
}

async fn asset(Path(path): Path<String>) -> Response {
    match FILES.iter().find(|(p, _, _)| *p == path) {
        Some((_, ctype, body)) => {
            ([(header::CONTENT_TYPE, *ctype), (header::CACHE_CONTROL, "no-cache")], *body).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
