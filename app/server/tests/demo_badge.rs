//! The sidebar's DEMO badge: shown only when the database holds the demo data (`app_meta.demo_data`), so a
//! `--no-default-features` build or a fresh install without the demo seed shows no badge.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::admin::{self, SetupInput};
use karbon_server::db::{self, Seed};
use karbon_server::{router, AppState};
use tower::ServiceExt;

const BADGE: &str = r#"<span class="brand-env">DEMO</span>"#;

/// The page's HTML, after checking it rendered.
async fn page(state: &AppState, uri: &str) -> String {
    let res = router(state.clone()).oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "{uri}");
    let html = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(html.contains(r#"<aside class="sidebar""#), "{uri} renders the sidebar");
    html
}

#[tokio::test]
async fn demo_database_shows_the_badge() {
    let (conn, report) = db::open_in_memory(Seed::Demo).unwrap();
    assert!(report.demo_seed_loaded);
    let state = AppState::new(conn);
    for uri in ["/", "/customers", "/sales/INV-1048", "/settings"] {
        assert!(page(&state, uri).await.contains(BADGE), "{uri} shows DEMO");
    }
}

#[tokio::test]
async fn database_without_the_demo_seed_shows_no_badge() {
    // What a --no-default-features build opens: the reference rows only, then the user's own setup.
    let (mut conn, report) = db::open_in_memory(Seed::Reference).unwrap();
    assert!(!report.demo_seed_loaded);
    let input = SetupInput {
        organization_name: "Harbor & Pine Design".into(),
        first_name: "Rosa".into(),
        last_name: "Lindqvist".into(),
        tax_rate_bp: 550,
        country_code: "1".into(),
        account_name: "rosa".into(),
        ..Default::default()
    };
    admin::setup_welcome(&mut conn, &input).unwrap();
    let state = AppState::new(conn);
    // Not /customers: it answers 500 on a database without customers (SUM(current) is NULL), a separate bug.
    for uri in ["/", "/settings"] {
        let html = page(&state, uri).await;
        assert!(!html.contains(BADGE) && !html.contains(">DEMO<"), "{uri} shows no DEMO");
    }
}
