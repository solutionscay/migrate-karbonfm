//! I3 import (OP-IMPORT-01/02, BR-IMPORT-01/02, T-08-09, T-08-13) on the synthetic FileMaker export of karbon_synth:
//! golden counts, every dirty row's outcome, rerun equality, money rounding (BR-SALES-01), the party caches
//! (BR-PARTY-06), the totals (BR-SALES-02), the refusals, and the time of the full import.

use karbon_money::sales::{self, Line};
use karbon_money::{Cents, Qty};
use karbon_server::import::{import_export, ImportError, Options, Report};
use karbon_server::{db, router, AppState};
use karbon_synth::dirty::{self, Outcome};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

struct Run {
    _dir: tempfile::TempDir,
    data: std::path::PathBuf,
    export: std::path::PathBuf,
    report: Report,
    conn: Connection,
}

fn fixed() -> Options {
    Options { batch_id: Some("batch-test".into()), now: Some("2026-11-01 09:00:00".into()), me_user_id: None }
}

fn run(scale: f64, dirty: bool) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("export");
    let data = dir.path().join("data");
    karbon_synth::generate(karbon_synth::Config { scale, dirty, ..Default::default() }, &export).unwrap();
    let report = import_export(&export, &data, &fixed()).unwrap();
    let conn = Connection::open(db::database_path(&data)).unwrap();
    Run { _dir: dir, data, export, report, conn }
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn golden_counts_of_the_full_export_and_the_import_time() {
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("export");
    let t = Instant::now();
    let summary = karbon_synth::generate(karbon_synth::Config::default(), &export).unwrap();
    let generate_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let report = import_export(&export, &dir.path().join("data"), &fixed()).unwrap();
    let import_ms = t.elapsed().as_millis();
    eprintln!(
        "generated {} rows in {generate_ms} ms; imported {} rows in {import_ms} ms",
        summary.total_rows(),
        report.loaded()
    );
    // T-08-13: the full import of a small business finishes in seconds, even in a debug build.
    assert!(import_ms < 20_000, "import took {import_ms} ms");

    let conn = Connection::open(db::database_path(&dir.path().join("data"))).unwrap();
    // Golden counts: the generator is deterministic (fixed seed), so these change only when it does.
    let golden: &[(&str, i64)] = &[
        ("party", 473),
        ("organization", 203),
        ("person", 270),
        ("party_role", 249),
        ("customer", 192),
        ("vendor", 21),
        ("employee", 6),
        ("lead", 30),
        ("contact_method", 1109),
        ("sales_transaction", 3723),
        ("invoice", 3358),
        ("sales_receipt", 365),
        ("sales_line", 12446),
        ("payment", 3526),
        ("payment_line", 3525),
        ("item", 25),
        ("opportunity", 320),
        ("activity", 3000),
        ("activity_party", 7061),
        ("file", 352),
        ("file_attachment", 350),
        ("template", 6),
        ("user", 3),
    ];
    let actual: Vec<(&str, i64)> =
        golden.iter().map(|(t, _)| (*t, count(&conn, &format!("SELECT COUNT(*) FROM \"{t}\"")))).collect();
    assert_eq!(actual, golden, "golden counts");
    // The export is the size the brief asks for: 300-500 parties, 3,000-5,000 sales transactions, 3-4 years.
    let parties = count(&conn, "SELECT COUNT(*) FROM party");
    assert!((300..=500).contains(&parties));
    let (first, last): (String, String) = conn
        .query_row("SELECT MIN(date), MAX(date) FROM sales_transaction", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(first.as_str() < "2023-02-01" && last.as_str() > "2026-10-01", "{first} .. {last}");
    // Every file: read = loaded + rejected + skipped.
    for f in &report.files {
        assert_eq!(f.rows_read, f.rows_loaded + f.rows_rejected + f.rows_skipped, "{f:?}");
    }
    assert_eq!(report.rejected.len(), 6);
    assert!(db::foreign_key_check(&conn).unwrap().is_empty());
}

#[test]
fn every_dirty_row_has_its_expected_outcome() {
    let r = run(0.1, true);
    for (table, key, outcome) in dirty::ROWS {
        match outcome {
            Outcome::Rejected(why) => {
                let hit = r
                    .report
                    .rejected
                    .iter()
                    .find(|x| x.source_table == *table && x.source_key.as_deref() == Some(*key))
                    .unwrap_or_else(|| panic!("{table} {key} not rejected: {:#?}", r.report.rejected));
                assert!(hit.reason.contains(why), "{table} {key}: {}", hit.reason);
                let n: i64 = r
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM import_rejected_row WHERE source_table = ?1 AND source_key = ?2 AND raw_json LIKE '{%'",
                        [table, key],
                        |x| x.get(0),
                    )
                    .unwrap();
                assert_eq!(n, 1, "{table} {key} in import_rejected_row");
            }
            Outcome::Changed(field) => {
                let n: i64 = r
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM import_changed_value WHERE source_table = ?1 AND source_key = ?2 AND source_field = ?3",
                        [table, key, field],
                        |x| x.get(0),
                    )
                    .unwrap();
                assert!(
                    n >= 1,
                    "{table} {key} {field} not listed: {:#?}",
                    r.report.changed.iter().filter(|c| c.source_key.as_deref() == Some(*key)).collect::<Vec<_>>()
                );
            }
            Outcome::Skipped => {
                assert!(r.report.file(table).unwrap().rows_skipped >= 1, "{table} {key}");
            }
        }
    }
    let c = &r.conn;
    // The duplicate Halden & Pryce row lost; the first row stays.
    assert_eq!(
        count(c, "SELECT COUNT(*) FROM party WHERE id = 'p-hp' AND name_cached = 'Halden & Pryce Architects'"),
        1
    );
    // Exact values of the changed rows.
    assert_eq!(count(c, "SELECT credit_limit IS NULL FROM vendor WHERE id = 'dirty-vendor-role'"), 1);
    assert_eq!(count(c, "SELECT party_role_type_id IS NULL FROM party_role WHERE id = 'dirty-cust-role'"), 1);
    assert_eq!(count(c, "SELECT amount FROM payment WHERE id = 'dirty-pay-round'"), 10001);
    let (qty, amount): (String, i64) = c
        .query_row("SELECT CAST(qty AS TEXT), amount FROM sales_line WHERE id = 'dirty-line-qty'", [], |x| {
            Ok((x.get(0)?, x.get(1)?))
        })
        .unwrap();
    assert_eq!((qty.as_str(), amount), ("1.3333", 4000), "1.3333 x 30.00 = 39.999 -> 40.00");
    assert_eq!(count(c, "SELECT container_path IS NULL FROM file WHERE id = 'dirty-file-missing'"), 1);
    assert_eq!(count(c, "SELECT COUNT(*) FROM file_attachment WHERE id = 'dirty-fa-chat'"), 0);
    assert_eq!(count(c, "SELECT COUNT(*) FROM file WHERE id = 'dirty-file-chat'"), 1, "the file row itself loads");
    let name: String = c.query_row("SELECT name_cached FROM party WHERE id = 'p-ellison'", [], |x| x.get(0)).unwrap();
    assert_eq!(name, "Ellison Studio");
    assert_eq!(count(c, "SELECT total_amt FROM sales_transaction WHERE id = 't-inv-1027'"), 418000);
    // CSV quoting: a name with quotes and a comma arrives whole.
    let org: String = c.query_row("SELECT name_cached FROM party WHERE id = 'dirty-org'", [], |x| x.get(0)).unwrap();
    assert_eq!(org, "Dirtywater \"Test\" Co, Inc.");
    // Chat (OD-05-02): the tables are skipped, SMS activities load without their messages.
    for t in ["Chat", "ChatMessage", "ChatConsent"] {
        let f = r.report.file(t).unwrap();
        assert!(f.rows_read > 0 && f.rows_skipped == f.rows_read && f.rows_loaded == 0, "{f:?}");
    }
    let sms = count(c, "SELECT COUNT(*) FROM activity WHERE activity_type_id = 9");
    assert_eq!(sms as usize, r.report.file("ChatMessage").unwrap().rows_read);
    // CHECK-01: deprecated values are counted in the report, not imported.
    let deprecated = r.report.not_imported.get(&("Activity".to_string(), "OpportunityId_DEPRECATED".to_string()));
    assert_eq!(deprecated.map(|d| d.0), Some(dirty::DEPRECATED_VALUES));
}

/// Appends a copy of the first data row of `<table>.csv`: a duplicate key (or a second System row), so it is rejected.
fn duplicate_first_row(export: &Path, table: &str) {
    let path = export.join(format!("{table}.csv"));
    let text = std::fs::read_to_string(&path).unwrap();
    let first = text.split("\r\n").nth(1).unwrap().to_string();
    std::fs::write(&path, format!("{text}{first}\r\n")).unwrap();
}

#[test]
fn secrets_and_pii_of_d09_are_not_imported() {
    // BR-IMPORT-02: the export carries an SSN, an SMTP password and an API token; none reaches the database, not even
    // through a rejected row's raw_json (an Employee with a duplicate key, a second System row).
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("export");
    karbon_synth::generate(karbon_synth::Config { scale: 0.05, dirty: false, ..Default::default() }, &export).unwrap();
    duplicate_first_row(&export, "Employee");
    duplicate_first_row(&export, "System");
    let data = dir.path().join("data");
    let report = import_export(&export, &data, &fixed()).unwrap();
    let r = Run { data, export, report, conn: Connection::open_in_memory().unwrap(), _dir: dir };
    let rejected: Vec<(&str, &str)> =
        r.report.rejected.iter().map(|x| (x.source_table.as_str(), x.reason.as_str())).collect();
    assert_eq!(rejected, [("System", "more than one System row"), ("Employee", "duplicate key r-mara-emp")]);
    for x in &r.report.rejected {
        assert!(x.raw_json.starts_with('{') && x.raw_json.len() > 2, "{}", x.raw_json);
        for dropped in ["SocialSecurityNumber", "SystemEmailAccountPassword", "TwilioAuthToken", "qbo_"] {
            assert!(!x.raw_json.contains(dropped), "{dropped} in {}", x.raw_json);
        }
    }
    let bytes = std::fs::read(db::database_path(&r.data)).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    for secret in [
        "synthetic-secret-not-imported",
        "synthetic-token-not-imported",
        "synthetic-twilio-not-imported",
        "900-00-0001",
    ] {
        assert!(!text.contains(secret), "{secret} is in the database");
    }
    for field in ["SystemEmailAccountPassword", "SmartyStreetsAuthToken"] {
        assert!(r.report.not_imported.contains_key(&("System".to_string(), field.to_string())), "{field}");
    }
    assert!(r.report.not_imported.contains_key(&("Employee".to_string(), "SocialSecurityNumber".to_string())));
}

/// Every row of every table except the volatile ones, in a stable order.
fn dump(conn: &Connection) -> Vec<String> {
    let tables: Vec<String> = {
        let mut s = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                 AND name NOT IN ('app_meta', 'refinery_schema_history') ORDER BY name",
            )
            .unwrap();
        let rows = s.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        rows
    };
    let mut out = Vec::new();
    for t in tables {
        let mut s = conn.prepare(&format!("SELECT * FROM \"{t}\"")).unwrap();
        let n = s.column_count();
        let mut rows: Vec<String> = s
            .query_map([], |r| {
                Ok((0..n)
                    .map(|i| format!("{:?}", r.get::<_, rusqlite::types::Value>(i).unwrap()))
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap()
            .map(|x| format!("{t}: {}", x.unwrap()))
            .collect();
        rows.sort();
        out.extend(rows);
    }
    out
}

#[test]
fn rerunning_into_a_fresh_database_gives_identical_results() {
    // BR-IMPORT-01: "rerun into a fresh database gives the same result".
    let a = run(0.15, true);
    let other = tempfile::tempdir().unwrap();
    let report = import_export(&a.export, other.path(), &fixed()).unwrap();
    let b = Connection::open(db::database_path(other.path())).unwrap();
    let (da, db_) = (dump(&a.conn), dump(&b));
    assert_eq!(da.len(), db_.len());
    let first_diff = da.iter().zip(&db_).find(|(x, y)| x != y);
    assert!(first_diff.is_none(), "first difference: {first_diff:?}");
    assert_eq!(a.report.files, report.files);
    assert_eq!(a.report.changed, report.changed);
    assert_eq!(a.report.rejected, report.rejected);
    // The copied containers are the same files.
    let files = |d: &Path| {
        let mut v: Vec<String> =
            walk(&d.join("files")).into_iter().map(|p| p.strip_prefix(d).unwrap().display().to_string()).collect();
        v.sort();
        v
    };
    assert_eq!(files(&a.data), files(other.path()));
    assert!(!files(&a.data).is_empty());
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// A hand-written export with only the tables a sales line needs, for the exact BR-SALES-01 examples.
fn write_small_export(dir: &Path, lines: &[(&str, &str, &str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    let w = |name: &str, text: String| std::fs::write(dir.join(name), text).unwrap();
    w("Party.csv", "\"Id\",\"NameCached\",\"OrganizationInd\"\r\n\"p1\",\"Acme\",\"1\"\r\n".into());
    w("Organization.csv", "\"Id\",\"OrganizationName\"\r\n\"p1\",\"Acme\"\r\n".into());
    w(
        "PartyRole.csv",
        "\"Id\",\"PartyId\",\"PartyRoleTypeId\",\"CustomerInd\"\r\n\"r1\",\"p1\",\"2\",\"1\"\r\n".into(),
    );
    w("Customer.csv", "\"Id\",\"Taxable\"\r\n\"r1\",\"1\"\r\n".into());
    w(
        "SalesTransaction.csv",
        "\"id\",\"docNumber\",\"date\",\"id_Customer\",\"InvoiceInd\",\"LockedInd\"\r\n\"t1\",\"T-1\",\"3/7/2026\",\"r1\",\"1\",\"1\"\r\n".into(),
    );
    let mut s = String::from("\"id\",\"SalesTransactionId\",\"qty\",\"unitPrice\",\"amount\",\"taxable\"\r\n");
    for (id, qty, price, amount) in lines {
        s.push_str(&format!("\"{id}\",\"t1\",\"{qty}\",\"{price}\",\"{amount}\",\"0\"\r\n"));
    }
    w("SalesLine.csv", s);
}

#[test]
fn money_rounding_examples_of_br_sales_01() {
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("export");
    write_small_export(
        &export,
        &[
            ("a", "2", "1200", "2400"),      // 2 x 1200.00 = 2400.00
            ("b", "6", "95", "570"),         // 6 x 95.00 = 570.00
            ("c", "1.5", "33.33", "49.995"), // 1.5 x 33.33 = 49.995 -> 50.00 (half away from zero)
            ("d", "0.5", "0.05", "0.025"),   // 0.5 x 0.05 = 0.025 -> 0.03
            ("e", "0.5", "-0.05", "-0.025"), // -> -0.03
            ("f", "", "12.5", "12.5"),       // empty quantity counts as 1
            ("g", "3", "33.3333333", "100"), // import of 33.3333333 -> 33.33; 3 x 33.33 = 99.99
        ],
    );
    let r = import_export(&export, &dir.path().join("data"), &fixed()).unwrap();
    let conn = Connection::open(db::database_path(&dir.path().join("data"))).unwrap();
    let line = |id: &str| -> (i64, i64) {
        conn.query_row("SELECT unit_price, amount FROM sales_line WHERE id = ?1", [id], |x| Ok((x.get(0)?, x.get(1)?)))
            .unwrap()
    };
    assert_eq!(line("a"), (120000, 240000));
    assert_eq!(line("b"), (9500, 57000));
    assert_eq!(line("c"), (3333, 5000));
    assert_eq!(line("d"), (5, 3));
    assert_eq!(line("e"), (-5, -3));
    assert_eq!(line("f"), (1250, 1250));
    assert_eq!(line("g"), (3333, 9999));
    // Listed: the amounts exported with 3 decimals (rounded on load), and g, whose unit price was rounded and whose
    // amount was recomputed. a, b and f are exact.
    let changed: Vec<(String, String, Option<String>, Option<String>)> = r
        .changed
        .iter()
        .filter(|c| c.source_table == "SalesLine")
        .map(|c| {
            (c.source_key.clone().unwrap(), c.source_field.clone(), c.source_value.clone(), c.stored_value.clone())
        })
        .collect();
    assert_eq!(
        changed,
        [
            ("c".into(), "amount".into(), Some("49.995".into()), Some("50.00".into())),
            ("d".into(), "amount".into(), Some("0.025".into()), Some("0.03".into())),
            ("e".into(), "amount".into(), Some("-0.025".into()), Some("-0.03".into())),
            ("g".into(), "unitPrice".into(), Some("33.3333333".into()), Some("33.33".into())),
            ("g".into(), "amount".into(), Some("100.00".into()), Some("99.99".into())),
        ]
    );
    // The transaction total follows the lines (BR-SALES-02), and the invoice with no payment is open.
    let (sub, total, balance): (i64, i64, i64) = conn
        .query_row("SELECT subtotal, total_amt, balance FROM sales_transaction WHERE id = 't1'", [], |x| {
            Ok((x.get(0)?, x.get(1)?, x.get(2)?))
        })
        .unwrap();
    let expect = 240000 + 57000 + 5000 + 3 - 3 + 1250 + 9999;
    assert_eq!((sub, total, balance), (expect, expect, expect));
}

#[test]
fn totals_of_a_sample_of_invoices_follow_br_sales_02() {
    let r = run(0.2, false);
    let c = &r.conn;
    let ids: Vec<String> = {
        let mut s = c.prepare("SELECT id FROM sales_transaction ORDER BY id LIMIT 80").unwrap();
        let rows = s.query_map([], |x| x.get(0)).unwrap().map(Result::unwrap).collect();
        rows
    };
    assert_eq!(ids.len(), 80);
    for id in ids {
        let mut s = c
            .prepare(
                "SELECT CAST(sl.qty AS TEXT), sl.unit_price, sl.taxable, COALESCE(it.name, '') = 'Retainer', sl.amount \
                 FROM sales_line sl LEFT JOIN item it ON it.id = sl.item_id WHERE sl.sales_transaction_id = ?1",
            )
            .unwrap();
        let lines: Vec<(Line, i64)> = s
            .query_map([&id], |x| {
                let qty = x.get::<_, Option<String>>(0)?.and_then(|q| Qty::parse_optional(&q).ok()).flatten();
                let line = Line {
                    qty,
                    unit_price: Cents(x.get(1)?),
                    taxable: x.get::<_, Option<i64>>(2)?.unwrap_or(0) == 1,
                    retainer: x.get(3)?,
                };
                Ok((line, x.get(4)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for (l, amount) in &lines {
            assert_eq!(l.amount().unwrap().0, *amount, "{id}: BR-SALES-01");
        }
        let (bp, ship, receipt, paid): (i64, i64, bool, i64) = c
            .query_row(
                "SELECT tax_rate_bp, COALESCE(ship_amt, 0), COALESCE(sales_receipt_ind, 0) = 1, \
                 (SELECT COALESCE(SUM(amount), 0) FROM payment_line WHERE id_invoice = st.id) FROM sales_transaction st WHERE id = ?1",
                [&id],
                |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?)),
            )
            .unwrap();
        let only: Vec<Line> = lines.iter().map(|(l, _)| *l).collect();
        let t = sales::totals(&only, bp, Cents(ship), Cents(paid)).unwrap();
        let stored: (i64, i64, i64, i64, i64, i64) = c
            .query_row(
                "SELECT subtotal, taxable_subtotal, total_tax, total_amt, balance, retainer FROM sales_transaction WHERE id = ?1",
                [&id],
                |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?, x.get(4)?, x.get(5)?)),
            )
            .unwrap();
        let balance = if receipt { 0 } else { t.balance.0 };
        assert_eq!(
            stored,
            (t.subtotal.0, t.taxable_subtotal.0, t.total_tax.0, t.total_amt.0, balance, t.retainer.0),
            "{id}"
        );
    }
    // BR-SALES-06: each customer's retainer balance is its posted retainer amounts plus the adjustment.
    let bad = count(
        c,
        "SELECT COUNT(*) FROM customer cu WHERE cu.retainer_balance_cached IS NOT COALESCE(cu.retainer_adjustment, 0) + \
         COALESCE((SELECT SUM(retainer) FROM sales_transaction st WHERE st.id_customer = cu.id AND st.locked_ind = 1 \
         AND COALESCE(st.deleted, 0) = 0), 0)",
    );
    assert_eq!(bad, 0);
    // The demo anchors keep their story: INV-1048, Halden & Pryce's balance and retainer (04_demo_seed.sql).
    let inv: (i64, i64, i64, i64) = c
        .query_row(
            "SELECT subtotal, total_tax, total_amt, balance FROM sales_transaction WHERE id = 't-inv-1048'",
            [],
            |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?)),
        )
        .unwrap();
    assert_eq!(inv, (305000, 715, 305715, 205715));
    let hp: (i64, i64) = c
        .query_row("SELECT balance, retainer_balance_cached FROM customer WHERE id = 'r-hp'", [], |x| {
            Ok((x.get(0)?, x.get(1)?))
        })
        .unwrap();
    assert_eq!(hp, (305715, 150000));
}

#[test]
fn party_caches_are_recomputed_after_import() {
    // BR-PARTY-06: every party's caches equal the derived values.
    let r = run(0.1, true);
    let c = &r.conn;
    let wrong_name = count(
        c,
        "SELECT COUNT(*) FROM party p WHERE p.name_cached IS NOT COALESCE( \
           (SELECT organization_name FROM organization o WHERE o.id = p.id), \
           (SELECT first_name || ' ' || last_name FROM person pe WHERE pe.id = p.id))",
    );
    assert_eq!(wrong_name, 0);
    let wrong_phone = count(
        c,
        "SELECT COUNT(*) FROM party p WHERE p.phone_number_cached IS NOT (SELECT ph.number FROM contact_method cm \
         JOIN phone ph ON ph.id = cm.id WHERE cm.party_id = p.id ORDER BY ph.primary_ind DESC, cm.creation_timestamp, cm.id LIMIT 1)",
    );
    assert_eq!(wrong_phone, 0);
    let (name, phone, email, address): (String, String, String, String) = c
        .query_row(
            "SELECT name_cached, phone_number_cached, email_address_cached, address_display_cached FROM party WHERE id = 'p-hp'",
            [],
            |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (name.as_str(), phone.as_str(), email.as_str(), address.as_str()),
        (
            "Halden & Pryce Architects",
            "(207) 555-0142",
            "studio@haldenpryce.example",
            "220 Commercial Street, Portland, ME 04101"
        )
    );
    assert_eq!(count(c, "SELECT COUNT(*) FROM party WHERE email_address_cached IS NOT NULL"), count(c, "SELECT COUNT(DISTINCT cm.party_id) FROM contact_method cm JOIN network_address n ON n.id = cm.id WHERE n.email_address_ind = 1"));
}

#[test]
fn stamps_ids_and_formats_are_kept() {
    let r = run(0.05, false);
    let c = &r.conn;
    // Source ids are the primary keys; stamps are converted to local civil time (D-05, D-39).
    let (created, account): (String, String) = c
        .query_row("SELECT creation_timestamp, creation_account_name FROM party WHERE id = 'p-ellison'", [], |x| {
            Ok((x.get(0)?, x.get(1)?))
        })
        .unwrap();
    assert_eq!(created, "2019-04-01 09:00:00");
    assert!(["mara", "devon", "priya"].contains(&account.as_str()));
    let bad = count(
        c,
        "SELECT COUNT(*) FROM sales_transaction WHERE date NOT GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'",
    );
    assert_eq!(bad, 0);
    let bad = count(c, "SELECT COUNT(*) FROM activity WHERE time_display NOT GLOB '[0-2][0-9]:[0-5][0-9]:[0-5][0-9]'");
    assert_eq!(bad, 0);
    // A FileMaker return (vertical tab) is a line break again.
    assert!(count(c, "SELECT COUNT(*) FROM activity WHERE message LIKE '%' || char(10) || '%'") > 0);
    assert_eq!(count(c, "SELECT COUNT(*) FROM activity WHERE message LIKE '%' || char(11) || '%'"), 0);
    // Folded subtypes (D-15): notes and meetings marked on the parent; email messages on the activity.
    assert!(count(c, "SELECT COUNT(*) FROM activity WHERE note_ind = 1") > 0);
    assert!(count(c, "SELECT COUNT(*) FROM event WHERE meeting_ind = 1") > 0);
    // Task completion (D-29): CompletedInd is not imported; completed_at carries it.
    assert!(count(c, "SELECT COUNT(*) FROM task WHERE completed_at IS NOT NULL") > 0);
    // Containers are copied into files/ and the stored path is relative to the data folder (D-17).
    let path: String = c
        .query_row("SELECT container_path FROM file WHERE container_path IS NOT NULL LIMIT 1", [], |x| x.get(0))
        .unwrap();
    assert!(path.starts_with("files/"), "{path}");
    assert!(r.data.join(&path).is_file());
    // Lineage: one import_file row per CSV, the batch completed, the report on disk.
    assert_eq!(
        count(c, "SELECT COUNT(*) FROM import_file WHERE batch_id = 'batch-test'") as usize,
        r.report.files.len()
    );
    assert_eq!(count(c, "SELECT COUNT(*) FROM import_batch WHERE status = 'completed' AND finished_at IS NOT NULL"), 1);
    assert!(r.report.report_path.is_file());
    let sha: String =
        c.query_row("SELECT sha256 FROM import_file WHERE source_table = 'Party'", [], |x| x.get(0)).unwrap();
    assert_eq!(sha.len(), 64);
    // "Me": the earliest user by default (BR-ADMIN-04).
    assert_eq!(r.report.me_user_id.as_deref(), Some("u-mara"));
}

#[test]
fn refusals_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let export = dir.path().join("export");
    karbon_synth::generate(karbon_synth::Config { scale: 0.05, ..Default::default() }, &export).unwrap();
    let data = dir.path().join("data");

    // "Me" must be a user of the export; the failed run removes the database it created.
    let opts = Options { me_user_id: Some("u-nobody".into()), ..fixed() };
    assert!(matches!(import_export(&export, &data, &opts), Err(ImportError::MeNotFound(_))));
    assert!(!db::database_path(&data).exists());
    assert!(walk(&data.join("files")).is_empty(), "copied containers are removed");

    // A good run, then a second one into the same folder is refused.
    let opts = Options { me_user_id: Some("u-priya".into()), ..fixed() };
    let r = import_export(&export, &data, &opts).unwrap();
    assert_eq!(r.me_user_id.as_deref(), Some("u-priya"));
    let conn = Connection::open(db::database_path(&data)).unwrap();
    assert_eq!(karbon_server::admin::current_user(&conn).unwrap().unwrap().user_id.as_deref(), Some("u-priya"));
    assert!(matches!(import_export(&export, &data, &fixed()), Err(ImportError::DatabaseExists(_))));

    // Unknown table, missing Party.csv, not UTF-8, broken CSV.
    let bad = dir.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("Party.csv"), "\"Id\"\r\n\"p1\"\r\n").unwrap();
    std::fs::write(bad.join("Spaceship.csv"), "\"Id\"\r\n").unwrap();
    assert!(
        matches!(import_export(&bad, &dir.path().join("d1"), &fixed()), Err(ImportError::UnknownTable(t)) if t == "Spaceship")
    );
    std::fs::remove_file(bad.join("Spaceship.csv")).unwrap();
    std::fs::rename(bad.join("Party.csv"), bad.join("Person.csv")).unwrap();
    assert!(matches!(import_export(&bad, &dir.path().join("d2"), &fixed()), Err(ImportError::MissingTable("Party"))));
    std::fs::write(bad.join("Party.csv"), b"\"Id\"\r\n\"caf\xe9\"\r\n").unwrap();
    assert!(matches!(import_export(&bad, &dir.path().join("d3"), &fixed()), Err(ImportError::NotUtf8(_))));
    std::fs::write(bad.join("Party.csv"), "\"Id\"\r\n\"unclosed\r\n").unwrap();
    assert!(matches!(import_export(&bad, &dir.path().join("d4"), &fixed()), Err(ImportError::Csv { .. })));
    for d in ["d1", "d2", "d3", "d4"] {
        assert!(!db::database_path(&dir.path().join(d)).exists(), "{d}");
    }
}

#[tokio::test]
async fn the_app_serves_an_imported_database() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let r = run(0.1, true);
    drop(r.conn);
    // The demo seed never loads into an imported database (it has parties).
    let (conn, report) = db::open(&db::database_path(&r.data), db::Seed::Demo).unwrap();
    assert!(!report.demo_seed_loaded && report.migrations_applied.is_empty());
    assert!(!db::has_demo_data(&conn).unwrap());
    let app = router(AppState::new(conn).with_data_dir(r.data.clone()));
    let mut seen = BTreeMap::new();
    for uri in ["/", "/customers", "/customers?page=2", "/customers?q=Halden", "/sales/INV-1048", "/settings"] {
        let res = app.clone().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        let body = String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes()).into_owned();
        assert_eq!(status, StatusCode::OK, "{uri}");
        seen.insert(uri, body);
    }
    assert!(seen["/sales/INV-1048"].contains("$2,057.15"), "INV-1048 balance due");
    let found = &seen["/customers?q=Halden"];
    assert!(found.contains("Halden &amp; Pryce") || found.contains("Halden &#38; Pryce"), "search finds the anchor");
}
