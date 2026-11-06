//! The big demo database the desktop app opens on (owner decision 2026-11-01, 03 "Demo seed data"): a new data
//! folder gets it and opens straight to the app; an existing database is never touched.
#![cfg(feature = "big-demo")]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use karbon_server::big_demo::{self, Outcome};
use karbon_server::db::{self, Seed};
use karbon_server::{admin, router, AppState};
use std::path::Path;
use tower::ServiceExt;

const BADGE: &str = r#"<span class="brand-env">DEMO</span>"#;

async fn get(state: &AppState, uri: &str) -> (StatusCode, String) {
    let res = router(state.clone()).oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// A second connection, for reads a test needs next to the state's.
fn state_db(dir: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(db::database_path(dir)).unwrap()
}

/// Every file under `dir` with its bytes, for "nothing changed" checks.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.strip_prefix(dir).unwrap().display().to_string(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn new_data_folder_gets_the_big_demo_and_opens_without_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    std::fs::create_dir_all(dir.join("logs")).unwrap(); // the desktop app starts its log first

    let Outcome::Created { rows, .. } = big_demo::create(&dir).unwrap() else { panic!("not created") };
    assert!(rows > 40_000, "{rows} rows");
    assert!(!dir.join(big_demo::STAGING_DIR).exists(), "the staging folder is removed");
    assert!(dir.join("files").is_dir(), "the containers are in the file store");
    for suffix in ["-wal", "-shm"] {
        assert!(!dir.join(format!("karbonfm.db{suffix}")).exists(), "one self-contained file");
    }

    // What the desktop app does next: open it as a real install would (reference rows only).
    let (conn, report) = db::open(&db::database_path(&dir), Seed::Reference).unwrap();
    assert!(report.migrations_applied.is_empty(), "the import migrated it already");
    assert!(admin::setup_done(&conn).unwrap());
    assert_eq!(admin::current_user(&conn).unwrap().and_then(|a| a.user_id).as_deref(), Some("u-mara"));
    assert!(db::has_demo_data(&conn).unwrap());
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert!(count("SELECT COUNT(*) FROM party") > 400);
    assert!(count("SELECT COUNT(*) FROM sales_transaction") > 3_000);
    let (tax, phones): (i64, i64) = conn
        .query_row(
            "SELECT tax_rate_bp, automatically_format_phone_numbers_ind FROM system_setting WHERE singleton = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((tax, phones), (550, 1));
    // B2: a client contact's EmployerName is the organization it was made with (kontroller 229 s229).
    let employer: Option<String> =
        conn.query_row("SELECT employer_name FROM person WHERE id = 'p-hp-eh'", [], |r| r.get(0)).unwrap();
    assert_eq!(employer.as_deref(), Some("Halden & Pryce Architects"));
    assert!(count("SELECT COUNT(*) FROM person WHERE employer_name IS NOT NULL") > 100);
    drop(conn);

    // No secret of the export reached the database (D-09, OD-05-02).
    let bytes = std::fs::read(db::database_path(&dir)).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    for secret in ["synthetic-secret-not-imported", "synthetic-token-not-imported", "synthetic-twilio-not-imported"] {
        assert!(!text.contains(secret), "{secret} is in the database");
    }
    assert!(!text.contains("900-00-000"), "an SSN is in the database");

    // Every page opens on it, with the DEMO badge, and none sends the window to /setup.
    let state = AppState::new(db::open(&db::database_path(&dir), Seed::Reference).unwrap().0).with_data_dir(&dir);
    let (status, html) = get(&state, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Good day, Mara"), "the greeting names the current user");
    assert!(html.contains(BADGE));
    for uri in [
        "/customers",
        "/customers?page=5",
        "/parties",
        "/parties/p-hp",
        "/parties/new",
        "/sales",
        "/payments",
        "/sales/INV-1048",
        "/sales/INV-12000",
        "/settings",
        "/activities",
        "/activities?kind=task&status=overdue",
        "/activities?page=3",
        "/items",
    ] {
        let (status, html) = get(&state, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains(BADGE), "{uri} shows DEMO");
    }
    assert_eq!(get(&state, "/setup").await.0, StatusCode::SEE_OTHER, "setup is done");

    // S2: the item list and the picker, under the same limit as the other long lists.
    for (uri, expect) in [("/items", "of "), ("/items?q=a", "of "), ("/pickers/item", "item-picker")] {
        let start = std::time::Instant::now();
        let (status, html) = get(&state, uri).await;
        let took = start.elapsed();
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains(expect), "{uri}");
        assert!(took.as_millis() < 600, "{uri} took {took:?}");
    }

    // 06 "Data-density": a page of a long list renders fast. Some 3,000 activities here; the debug build gets a
    // generous margin over the 200 ms of 06 (a release build is several times faster).
    for uri in ["/sales?page=5", "/sales?status=overdue", "/sales/INV-12000"] {
        let start = std::time::Instant::now();
        let (status, html) = get(&state, uri).await;
        let took = start.elapsed();
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(html.contains("INV-"), "{uri}");
        assert!(took.as_millis() < 600, "{uri} took {took:?}");
    }
    // The editor of a draft, one of the last week's (B11 leaves them unposted).
    let draft: String = {
        let conn = state_db(&dir);
        conn.query_row(
            "SELECT doc_number FROM sales_transaction WHERE COALESCE(locked_ind, 0) = 0 AND invoice_ind = 1 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let start = std::time::Instant::now();
    let (status, html) = get(&state, &format!("/sales/{draft}")).await;
    let took = start.elapsed();
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("txn-editor"), "{draft} opens in the editor");
    assert!(took.as_millis() < 600, "the editor took {took:?}");
    for uri in ["/activities?page=2", "/activities?kind=task&status=incomplete"] {
        let start = std::time::Instant::now();
        let (status, html) = get(&state, uri).await;
        let took = start.elapsed();
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("tl-item"), "{uri} lists activities");
        assert!(took.as_millis() < 600, "{uri} took {took:?}");
    }
}

#[test]
fn existing_database_is_never_touched() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // The user's own data: here the small seed, closed so its WAL is folded in.
    drop(db::open(&db::database_path(dir), Seed::Demo).unwrap());
    let before = snapshot(dir);
    assert_eq!(big_demo::create(dir).unwrap(), Outcome::Existing);
    assert_eq!(snapshot(dir), before, "no file added, changed or removed");
    let (conn, _) = db::open(&db::database_path(dir), Seed::Reference).unwrap();
    let parties: i64 = conn.query_row("SELECT COUNT(*) FROM party", [], |r| r.get(0)).unwrap();
    assert!(parties < 100, "still the small seed: {parties} parties");
}

#[test]
fn a_file_store_without_a_database_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("files/2024")).unwrap();
    std::fs::write(dir.join("files/2024/a.pdf"), b"user file").unwrap();
    let before = snapshot(dir);
    assert_eq!(big_demo::create(dir).unwrap(), Outcome::FilesWithoutDatabase);
    assert_eq!(snapshot(dir), before);
}

#[test]
fn a_half_built_staging_folder_is_rebuilt() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let staging = dir.join(big_demo::STAGING_DIR);
    std::fs::create_dir_all(staging.join("data")).unwrap();
    std::fs::write(staging.join("data/karbonfm.db"), b"not a database").unwrap();
    assert!(matches!(big_demo::create(dir).unwrap(), Outcome::Created { .. }));
    assert!(!staging.exists());
    let (conn, _) = db::open(&db::database_path(dir), Seed::Reference).unwrap();
    assert!(db::has_demo_data(&conn).unwrap());
}

/// (table, column) of every text column holding a date or a timestamp.
fn columns_with_dates(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p
             WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'",
        )
        .unwrap();
    let all: Vec<(String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    all.into_iter()
        .filter(|(t, c)| {
            let sql = format!(
                r#"SELECT EXISTS (SELECT 1 FROM "{t}" WHERE "{c}" GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]*')"#
            );
            conn.query_row(&sql, [], |r| r.get::<_, bool>(0)).unwrap()
        })
        .collect()
}

fn usd_after<'a>(html: &'a str, label: &str) -> &'a str {
    let at = html.find(label).unwrap_or_else(|| panic!("no {label}"));
    let rest = &html[at..];
    let start = rest.find('$').unwrap();
    let end = rest[start..].find('<').unwrap();
    &rest[start..start + end]
}

/// B11 on the big demo: created in the first week of a month, when the shift alone would leave no posted sale.
#[tokio::test]
async fn b11_big_demo_is_current_on_any_day() {
    let today = karbon_server::demo_dates::parse_date("2026-11-06").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(matches!(big_demo::create_as_of(&dir, today).unwrap(), Outcome::Created { .. }));
    let (conn, _) = db::open(&db::database_path(&dir), Seed::Reference).unwrap();
    let int = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();

    // The export's last day is now today, and a sale was posted on it.
    let newest: String = conn.query_row("SELECT MAX(date) FROM sales_transaction", [], |r| r.get(0)).unwrap();
    assert_eq!(newest, "2026-11-06");
    assert!(int("SELECT COUNT(*) FROM sales_transaction WHERE date = '2026-11-06' AND locked_ind = 1") > 0);
    assert_eq!(
        int("SELECT COUNT(*) FROM sales_transaction WHERE date IS NOT NULL
             AND period <> CAST(strftime('%Y%m', date) AS INTEGER)"),
        0,
        "period follows date"
    );
    assert!(db::foreign_key_check(&conn).unwrap().is_empty());

    // Every date column is one the shift knows, or one it keeps on purpose.
    let shifted = karbon_server::demo_dates::shifted_columns(&conn).unwrap();
    for (t, c) in columns_with_dates(&conn) {
        let kept = karbon_server::demo_dates::KEPT_TABLES.contains(&t.as_str())
            || karbon_server::demo_dates::KEPT_COLUMNS.iter().any(|(kt, kc)| *kt == t && *kc == c);
        assert!(kept || shifted.contains(&(t.clone(), c.clone())), "{t}.{c} holds dates and is in neither list");
    }

    let revenue = int(
        "SELECT COALESCE(SUM(sl.amount), 0) FROM sales_line sl JOIN sales_transaction st ON st.id = sl.sales_transaction_id
         LEFT JOIN item i ON i.id = sl.item_id
         WHERE st.locked_ind = 1 AND COALESCE(i.name, '') <> 'Retainer' AND st.date >= '2026-11-01'",
    );
    let balance =
        int("SELECT SUM(balance) FROM sales_transaction WHERE invoice_ind = 1 AND locked_ind = 1 AND balance > 0
         AND COALESCE(deleted, 0) = 0");
    assert!(revenue > 0, "b11: revenue this month is above $0.00");
    let state = AppState::new(conn).with_data_dir(&dir).with_today("2026-11-06");
    let (_, html) = get(&state, "/").await;
    assert_eq!(usd_after(&html, "Revenue this month"), karbon_server::money::usd(revenue));
    assert_eq!(usd_after(&html, "Balance due"), karbon_server::money::usd(balance));
    // b11_dashboard_overdue_matches_customers, on the big demo.
    let overdue = usd_after(&html, ">Overdue<").to_owned();
    let (_, customers) = get(&state, "/customers").await;
    assert!(customers.contains(&overdue), "/customers shows {overdue}");
    assert_ne!(overdue, "$0.00");
}

/// B11 by the wall clock: `create` uses the local date, with nothing pinned.
#[tokio::test]
async fn b11_big_demo_by_the_wall_clock_has_revenue() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    std::fs::create_dir_all(&dir).unwrap();
    big_demo::create(&dir).unwrap();
    let state = AppState::new(db::open(&db::database_path(&dir), Seed::Reference).unwrap().0).with_data_dir(&dir);
    let (_, html) = get(&state, "/").await;
    assert_ne!(usd_after(&html, "Revenue this month"), "$0.00");
}

/// S3: the commands' customer caches agree with the importer's recompute, for every customer; one save is fast on the
/// big demo; the model and the stored totals agree on what posting did.
#[test]
fn sales_commands_agree_with_import_recompute() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    std::fs::create_dir_all(&dir).unwrap();
    big_demo::create_as_of(&dir, karbon_server::demo_dates::parse_date("2026-11-06").unwrap()).unwrap();
    let (mut conn, _) = db::open(&db::database_path(&dir), Seed::Reference).unwrap();
    let actor = admin::current_user(&conn).unwrap().unwrap();
    let customers: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM customer").unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert!(customers.len() > 100);
    let dump = |c: &rusqlite::Connection| -> Vec<(String, i64, i64)> {
        let mut stmt = c
            .prepare("SELECT id, COALESCE(retainer_balance_cached, 0), COALESCE(balance, 0) FROM customer ORDER BY id")
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        rows.map(Result::unwrap).collect()
    };
    let imported = dump(&conn);
    for role in &customers {
        karbon_server::sales::recompute_customer(&conn, role).unwrap();
    }
    assert_eq!(dump(&conn), imported, "the same two formulas");

    // A stored transaction recomputes to what the import stored, with shipping, tax and retainer lines among them.
    let ids: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT id FROM sales_transaction WHERE COALESCE(ship_amt, 0) > 0 OR retainer <> 0 LIMIT 300")
            .unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    let totals = |c: &rusqlite::Connection, id: &str| -> (i64, i64, i64, i64, i64, i64) {
        c.query_row(
            "SELECT subtotal, taxable_subtotal, total_tax, total_amt, balance, retainer FROM sales_transaction WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap()
    };
    assert!(ids.len() >= 100);
    for id in &ids {
        let before = totals(&conn, id);
        karbon_server::sales::recompute_totals(&conn, id).unwrap();
        assert_eq!(totals(&conn, id), before, "{id}");
    }

    // One save on the big demo.
    let draft: String = conn
        .query_row(
            "SELECT id FROM sales_transaction WHERE COALESCE(locked_ind, 0) = 0 AND invoice_ind = 1 LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let input = karbon_server::sales::transaction_get(&conn, &draft).unwrap().input;
    let start = std::time::Instant::now();
    karbon_server::sales::transaction_save(
        &mut conn,
        karbon_server::sales::Kind::Invoice,
        &draft,
        &input,
        false,
        &actor,
    )
    .unwrap();
    let took = start.elapsed();
    assert!(took.as_millis() < 600, "a save took {took:?}");
}

/// "The PDF matches the totals" at full scale (S5): every invoice's model equals its stored totals, the printed rows on
/// a sample with shipping and with a retainer line, and a long invoice's PDF is fast.
#[tokio::test]
async fn pdf_matches_totals_big_demo() {
    use karbon_money::Cents;
    use karbon_server::invoice::Invoice;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(matches!(big_demo::create(&dir).unwrap(), Outcome::Created { .. }));
    let conn = db::open(&db::database_path(&dir), Seed::Reference).unwrap().0;

    type Row = (String, i64, i64, i64, i64, i64);
    let rows: Vec<Row> = conn
        .prepare(
            "SELECT doc_number, COALESCE(total_tax, 0), COALESCE(total_amt, 0), COALESCE(retainer, 0),
                    COALESCE(balance, 0), COALESCE(ship_amt, 0)
             FROM sales_transaction
             WHERE invoice_ind = 1 AND COALESCE(deleted, 0) = 0 AND doc_number IS NOT NULL",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(rows.len() > 2_000, "{} invoices", rows.len());
    let mut wrong = Vec::new();
    for (doc, tax, total, retainer, balance, ship) in &rows {
        let inv = Invoice::load(&conn, doc).unwrap();
        let t = inv.totals;
        if (t.total_tax.0, t.total_amt.0, t.retainer.0, t.balance.0, inv.ship_amt.0)
            != (*tax, *total, *retainer, *balance, *ship)
        {
            wrong.push(format!(
                "{doc}: model {t:?}, stored tax {tax} total {total} retainer {retainer} balance {balance}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{} of {}: {:#?}", wrong.len(), rows.len(), &wrong[..wrong.len().min(10)]);

    let printed_row =
        |texts: &[String], label: &str, value: String| texts.windows(2).any(|w| w[0] == label && w[1] == value);
    let with_ship = rows.iter().find(|r| r.5 != 0).expect("an invoice with shipping");
    let with_retainer = rows.iter().find(|r| r.3 != 0).expect("an invoice with a retainer line");
    for (doc, _, total, retainer, balance, ship) in [with_ship, with_retainer] {
        let inv = Invoice::load(&conn, doc).unwrap();
        let t: Vec<String> = karbon_server::pdf::page_texts(&inv).into_iter().flatten().collect();
        assert!(printed_row(&t, "Total", Cents(total - retainer).to_string()), "{doc}: {t:?}");
        assert!(printed_row(&t, "Balance due", Cents(*balance).to_string()), "{doc}: {t:?}");
        if *ship != 0 {
            assert!(printed_row(&t, "Shipping", Cents(*ship).to_string()), "{doc}: {t:?}");
        }
    }
    drop(conn);

    let state = AppState::new(db::open(&db::database_path(&dir), Seed::Reference).unwrap().0);
    let start = std::time::Instant::now();
    let (status, _) = get(&state, "/sales/INV-12000/invoice.pdf").await;
    let took = start.elapsed();
    assert_eq!(status, StatusCode::OK);
    assert!(took.as_millis() < 600, "the PDF took {took:?}");
}
