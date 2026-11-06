//! B11: the demo data's dates follow today, and every figure of the small seed stays true on any date.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::db::{self, Seed};
use karbon_server::demo_dates::{self, parse_date, KEPT_COLUMNS, KEPT_TABLES};
use karbon_server::{router, AppState};
use rusqlite::Connection;
use tower::ServiceExt;

fn seeded_as_of(today: &str) -> Connection {
    db::open_in_memory(Seed::DemoAsOf(parse_date(today).unwrap())).unwrap().0
}

async fn get(state: &AppState, uri: &str) -> String {
    let res = router(state.clone()).oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "{uri}");
    String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes()).into_owned()
}

/// The figure printed after a label: the next `$…` of the page.
fn usd_after<'a>(html: &'a str, label: &str) -> &'a str {
    let rest = &html[html.find(label).unwrap_or_else(|| panic!("no {label}"))..];
    let start = rest.find('$').unwrap();
    let end = rest[start..].find('<').unwrap();
    &rest[start..start + end]
}

#[tokio::test]
async fn b11_small_seed_as_of() {
    for (today, revenue) in [
        ("2026-10-28", "$25,654.50"), // no shift
        ("2026-11-06", "$20,595.50"), // INV-1050 on Nov 1, INV-1051 on Nov 6
        ("2026-12-01", "$12,980.00"), // only INV-1051
        ("2027-01-01", "$12,980.00"), // only INV-1051, across a year
        ("2028-02-29", "$28,454.50"), // 28 days of October's sales, in a leap month
        ("2028-03-01", "$12,980.00"), // only INV-1051, the day after
    ] {
        let conn = seeded_as_of(today);
        assert!(db::foreign_key_check(&conn).unwrap().is_empty(), "{today}");
        let off: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sales_transaction WHERE period <> CAST(strftime('%Y%m', date) AS INTEGER)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(off, 0, "{today}: period follows date");
        let state = AppState::new(conn).with_today(today);

        let home = get(&state, "/").await;
        assert_eq!(usd_after(&home, "Revenue this month"), revenue, "{today}");
        assert_eq!(usd_after(&home, "Balance due"), "$29,619.05", "{today}");
        assert!(home.contains("9 open invoices"), "{today}");
        assert_eq!(usd_after(&home, "Open pipeline"), "$113,450.00", "{today}");
        // b11_dashboard_overdue_matches_customers: the tile and /customers read one query.
        assert_eq!(usd_after(&home, ">Overdue<"), "$4,457.40", "{today}");
        assert!(home.contains("3 customers"), "{today}");

        let customers = get(&state, "/customers").await;
        assert!(customers.contains("$29,619.05") && customers.contains("$4,457.40"), "{today}");
        assert!(customers.contains("41"), "{today}: oldest 41 days");
    }
}

/// The fixed seed drifts with a pinned today the way the app did: a later date makes more invoices overdue.
#[tokio::test]
async fn pinned_today_drives_the_overdue_figures() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let state = AppState::new(conn).with_today("2026-11-06");
    let home = get(&state, "/").await;
    assert_eq!(usd_after(&home, ">Overdue<"), "$7,514.55");
    assert_eq!(usd_after(&home, "Revenue this month"), "$0.00", "the fixed seed: why B11 was reported");
}

#[test]
fn b11_zero_shift_changes_nothing() {
    let fixed = db::open_in_memory(Seed::Demo).unwrap().0;
    let shifted = seeded_as_of(demo_dates::SMALL_SEED_ANCHOR);
    // The tables the seed fills. The reference rows are stamped when they are written, so they are left out.
    let tables = [
        "sales_transaction",
        "payment",
        "activity",
        "activity_comment",
        "task",
        "event",
        "notification",
        "opportunity",
    ];
    for (table, column) in demo_dates::shifted_columns(&fixed).unwrap() {
        if !tables.contains(&table.as_str()) {
            continue;
        }
        let sql = format!(r#"SELECT group_concat(COALESCE("{column}", '-')) FROM "{table}""#);
        let dump = |c: &Connection| c.query_row(&sql, [], |r| r.get::<_, Option<String>>(0)).unwrap();
        assert_eq!(dump(&fixed), dump(&shifted), "{table}.{column}");
    }
}

#[test]
fn b11_seed_demo_is_never_shifted() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let date: String =
        conn.query_row("SELECT date FROM sales_transaction WHERE doc_number = 'INV-1048'", [], |r| r.get(0)).unwrap();
    assert_eq!(date, "2026-10-14");
}

#[test]
fn a_shift_moves_every_kind_of_date_and_keeps_a_birthday() {
    let conn = seeded_as_of("2026-11-06"); // 9 days
    let one = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, Option<String>>(0)).unwrap();
    assert_eq!(one("SELECT date FROM sales_transaction WHERE doc_number = 'INV-1048'").unwrap(), "2026-10-23");
    assert_eq!(one("SELECT due_date FROM sales_transaction WHERE doc_number = 'INV-1048'").unwrap(), "2026-11-07");
    assert_eq!(
        one("SELECT smtp_timestamp FROM email_message WHERE id = 'a-mail-1048'").unwrap(),
        "2026-10-23 16:40:00"
    );
    assert_eq!(one("SELECT date FROM payment WHERE id = 'pay-004'").unwrap(), "2026-10-29");
    assert_eq!(one("SELECT due_date FROM task WHERE id = 'a-task-quote'").unwrap(), "2026-11-06", "still due today");
    conn.execute("UPDATE person SET date_of_birth = '1980-05-01' WHERE id = 'p-mara'", []).unwrap();
    demo_dates::shift(&conn, parse_date("2026-11-06").unwrap(), parse_date("2026-11-07").unwrap()).unwrap();
    assert_eq!(one("SELECT date_of_birth FROM person WHERE id = 'p-mara'").unwrap(), "1980-05-01");
}

#[test]
fn b11_every_date_column_is_listed() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let shifted = demo_dates::shifted_columns(&conn).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p
             WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'",
        )
        .unwrap();
    let all: Vec<(String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    for (t, c) in all {
        let sql = format!(
            r#"SELECT EXISTS (SELECT 1 FROM "{t}" WHERE "{c}" GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]*')"#
        );
        let has_dates: bool = conn.query_row(&sql, [], |r| r.get(0)).unwrap();
        let kept = KEPT_TABLES.contains(&t.as_str()) || KEPT_COLUMNS.iter().any(|(kt, kc)| *kt == t && *kc == c);
        assert!(!has_dates || kept || shifted.contains(&(t.clone(), c.clone())), "{t}.{c} is in neither list");
    }
}

#[test]
fn view_excludes_retainer_lines() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let mut stmt =
        conn.prepare("SELECT period, revenue, transactions FROM sales_summary_by_month ORDER BY period").unwrap();
    let rows: Vec<(i64, i64, i64)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<Result<_, _>>().unwrap();
    // The retainer deposits and credits are gone: RET-0042, RET-0044, RET-0045, RET-0046 have no revenue line.
    assert_eq!(rows, [(202607, 418_000, 1), (202608, 544_500, 2), (202609, 765_240, 4), (202610, 2_565_450, 5)]);
}
