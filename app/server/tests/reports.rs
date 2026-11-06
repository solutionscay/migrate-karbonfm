//! S7 The monthly sales summary (UI-20, OP-SALES-11, BR-SALES-17). Small seed, pinned to 2026-10-28.

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::{db, router, AppState};
use rusqlite::Connection;
use tower::ServiceExt;

fn demo() -> (axum::Router, Connection, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let path = db::database_path(tmp.path());
    let (conn, _) = db::open(&path, db::Seed::Demo).unwrap();
    let app = router(AppState::new(conn).with_today("2026-10-28"));
    let check = Connection::open(&path).unwrap();
    (app, check, tmp)
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let (s, _, b) = send(app, Request::get(uri).body(Body::empty()).unwrap()).await;
    (s, b)
}

/// (period label, revenue, transactions) of each table row, and the total row last.
fn table_rows(html: &str) -> Vec<(String, String, String)> {
    let cell = |td: &str| td.split('>').nth(1).unwrap_or("").split('<').next().unwrap_or("").to_owned();
    html.split("<tr")
        .skip(2)
        .map(|row| {
            let tds: Vec<&str> = row.split("<td").skip(1).collect();
            (cell(tds[0]), cell(tds[1]), cell(tds[2]))
        })
        .collect()
}

fn row(html: &str, period: &str) -> (String, String) {
    let rows = table_rows(html);
    let r = rows.iter().find(|r| r.0 == period).unwrap_or_else(|| panic!("{period} in {rows:?}"));
    (r.1.clone(), r.2.clone())
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_owned(), b.to_owned())
}

#[tokio::test]
async fn sum_per_period_equals_line_sums() {
    let (app, check, _tmp) = demo();
    let (status, html) = get(&app, "/reports/sales-by-month").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<!doctype html>") && html.contains("chart-4.4.7"));
    assert!(html.contains(r#"href="/reports/sales-by-month" aria-current="page""#), "the sidebar link");
    assert_eq!(table_rows(&html).len(), 13, "the last 12 months and the total");
    assert_eq!(table_rows(&html)[0].0, "Nov 2025");
    assert_eq!(row(&html, "Jul 2026"), pair("$4,180.00", "1"));
    assert_eq!(row(&html, "Aug 2026"), pair("$5,445.00", "2"));
    assert_eq!(row(&html, "Sep 2026"), pair("$7,652.40", "4"));
    assert_eq!(row(&html, "Oct 2026"), pair("$25,654.50", "5"));
    assert_eq!(row(&html, "Total"), pair("$42,931.90", "12"));

    // Independently, from the lines.
    let by_lines: Vec<(i64, i64)> = check
        .prepare(
            "SELECT st.period, SUM(sl.amount) FROM sales_line sl
             JOIN sales_transaction st ON st.id = sl.sales_transaction_id LEFT JOIN item it ON it.id = sl.item_id
             WHERE st.locked_ind = 1 AND COALESCE(st.deleted, 0) = 0 AND (it.name IS NULL OR it.name <> 'Retainer')
             GROUP BY st.period ORDER BY st.period",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(by_lines, vec![(202607, 418_000), (202608, 544_500), (202609, 765_240), (202610, 2_565_450)]);
}

#[tokio::test]
async fn a_month_with_only_a_retainer_shows_no_revenue() {
    let (app, check, _tmp) = demo();
    check
        .execute_batch(
            "INSERT INTO sales_transaction (id, doc_number, date, id_customer, locked_ind, period, sales_receipt_ind, invoice_ind)
               VALUES ('t-ret-x', 'RET-X', '2025-02-10', 'r-hp', 1, 202502, 1, 0);
             INSERT INTO sales_line (id, sales_transaction_id, item_id, description, qty, unit_price, amount, taxable)
               VALUES ('l-ret-x', 't-ret-x', 'i-retainer', 'Retainer', 1, 100000, 100000, 0);",
        )
        .unwrap();
    let (status, html) = get(&app, "/reports/sales-by-month?from=202502&to=202502").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(row(&html, "Feb 2025"), pair("$0.00", "0"), "BR-SALES-17");
}

#[tokio::test]
async fn post_and_unpost_move_the_number() {
    let (app, check, _tmp) = demo();
    let id: String =
        check.query_row("SELECT id FROM sales_transaction WHERE doc_number = 'INV-1048'", [], |r| r.get(0)).unwrap();
    let lock = |value: &'static str| {
        Request::post(format!("/sales-transactions/{id}/lock"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("HX-Request", "true")
            .body(Body::from(format!("lock={value}")))
            .unwrap()
    };
    assert_eq!(send(&app, lock("false")).await.0, StatusCode::OK);
    let (_, html) = get(&app, "/reports/sales-by-month").await;
    // INV-1048's lines less its retainer line: $3,550.00.
    assert_eq!(row(&html, "Oct 2026"), pair("$22,104.50", "4"));
    assert_eq!(send(&app, lock("true")).await.0, StatusCode::OK);
    let (_, html) = get(&app, "/reports/sales-by-month").await;
    assert_eq!(row(&html, "Oct 2026"), pair("$25,654.50", "5"));
}

#[tokio::test]
async fn months_with_no_sales_show_0_and_a_range_is_its_months() {
    let (app, _check, _tmp) = demo();
    let (_, html) = get(&app, "/reports/sales-by-month?from=202512&to=202602").await;
    let rows = table_rows(&html);
    assert_eq!(
        rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["Dec 2025", "Jan 2026", "Feb 2026", "Total"],
        "over a year end"
    );
    assert!(rows.iter().all(|r| r.1 == "$0.00" && r.2 == "0"), "{rows:?}");

    let (_, html) = get(&app, "/reports/sales-by-month?from=202608&to=202609").await;
    assert_eq!(table_rows(&html).len(), 3);
    assert_eq!(row(&html, "Total"), pair("$13,097.40", "6"));
    assert!(html.contains(r#"name="from" value="202608""#) && html.contains(r#"name="to" value="202609""#));
}

#[tokio::test]
async fn a_bad_period_is_refused_with_the_form() {
    let (app, _check, _tmp) = demo();
    for (query, field) in [
        ("from=2026", "from"),
        ("from=202613", "from"),
        ("to=abc", "to"),
        ("from=202610&to=202609", "from"),
        ("from=200001&to=202610", "from"),
    ] {
        let (status, html) = get(&app, &format!("/reports/sales-by-month?{query}")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
        assert!(html.contains(&format!(r#"id="err-{field}""#)), "{query}");
        assert!(html.contains("<!doctype html>"), "the page, with the form");
        assert!(!html.contains("data-revenue-chart"), "{query}: no chart");
    }
    let req = Request::get("/reports/sales-by-month?from=x").header("HX-Request", "true").body(Body::empty()).unwrap();
    let (status, _, html) = send(&app, req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!html.contains("<html") && html.contains(r#"value="x""#), "a fragment keeps what was typed");
    let (_, html) = get(&app, "/reports/sales-by-month?from=%3Cscript%3E").await;
    assert!(!html.contains("<script>alert") && !html.contains("value=\"<script>"), "escaped");
}

#[tokio::test]
async fn the_chart_equals_the_table() {
    let (app, _check, _tmp) = demo();
    let (_, html) = get(&app, "/reports/sales-by-month?from=202607&to=202610").await;
    let raw = html.split("data-revenue-chart=\"").nth(1).unwrap().split('"').next().unwrap();
    let raw = raw.replace("&quot;", "\"").replace("&#34;", "\"");
    let chart: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(chart["labels"], serde_json::json!(["Jul 2026", "Aug 2026", "Sep 2026", "Oct 2026"]));
    assert_eq!(chart["revenue"], serde_json::json!([4180.0, 5445.0, 7652.4, 25654.5]));
    let rows = table_rows(&html);
    for (i, label) in chart["labels"].as_array().unwrap().iter().enumerate() {
        assert_eq!(rows[i].0, label.as_str().unwrap());
    }
}

#[tokio::test]
async fn the_dashboard_links_to_it() {
    let (app, _check, _tmp) = demo();
    let (_, html) = get(&app, "/").await;
    assert!(html.contains(r#"class="text-[12px] link link-hover" href="/reports/sales-by-month""#));
}
