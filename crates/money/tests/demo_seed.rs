//! Loads 04_database_schema.sql and 04_demo_seed.sql into SQLite, recomputes every
//! stored money value with the crate, and checks the figures shown in design/.

use std::collections::BTreeMap;
use std::path::PathBuf;

use karbon_money::sales::{retainer_balance, totals, Line, Totals, RETAINER_ITEM_NAME};
use karbon_money::{Cents, Qty};
use rusqlite::Connection;

fn demo_db() -> Connection {
    let migration = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migration");
    let read = |name: &str| std::fs::read_to_string(migration.join(name)).unwrap();
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(&read("04_database_schema.sql")).unwrap();
    db.execute_batch(&read("04_demo_seed.sql")).unwrap();
    db
}

struct Txn {
    id: String,
    customer: String,
    receipt: bool,
    locked: bool,
    tax_rate_bp: i64,
    ship_amt: Cents,
    stored: Totals,
}

fn transactions(db: &Connection) -> Vec<Txn> {
    let mut stmt = db
        .prepare(
            "SELECT id, id_customer, sales_receipt_ind, locked_ind, tax_rate_bp, ship_amt,
                    subtotal, taxable_subtotal, total_tax, total_amt, balance, retainer
             FROM sales_transaction ORDER BY id",
        )
        .unwrap();
    let rows = stmt.query_map([], |r| {
        Ok(Txn {
            id: r.get(0)?,
            customer: r.get(1)?,
            receipt: r.get::<_, i64>(2)? == 1,
            locked: r.get::<_, i64>(3)? == 1,
            tax_rate_bp: r.get(4)?,
            ship_amt: Cents(r.get(5)?),
            stored: Totals {
                subtotal: Cents(r.get(6)?),
                taxable_subtotal: Cents(r.get(7)?),
                total_tax: Cents(r.get(8)?),
                total_amt: Cents(r.get(9)?),
                balance: Cents(r.get(10)?),
                retainer: Cents(r.get(11)?),
            },
        })
    });
    rows.unwrap().map(Result::unwrap).collect()
}

/// The lines of a transaction, with the stored amount and amount_taxable.
fn lines(db: &Connection, txn: &str) -> Vec<(Line, Cents, Cents)> {
    // qty is NUMERIC (a REAL for 1.5): read it as text and parse it exactly.
    let mut stmt = db
        .prepare(
            "SELECT CAST(l.qty AS TEXT), l.unit_price, l.taxable, i.name, l.amount, l.amount_taxable
             FROM sales_line l JOIN item i ON i.id = l.item_id
             WHERE l.sales_transaction_id = ?1 ORDER BY l.id",
        )
        .unwrap();
    let rows = stmt.query_map([txn], |r| {
        let qty: Option<String> = r.get(0)?;
        let line = Line {
            qty: qty.map(|q| Qty::parse(&q).unwrap()),
            unit_price: Cents(r.get(1)?),
            taxable: r.get::<_, i64>(2)? == 1,
            retainer: r.get::<_, String>(3)? == RETAINER_ITEM_NAME,
        };
        Ok((line, Cents(r.get(4)?), Cents(r.get(5)?)))
    });
    rows.unwrap().map(Result::unwrap).collect()
}

fn paid(db: &Connection, txn: &Txn) -> Cents {
    if txn.receipt {
        // A sales receipt is paid at the sale.
        return txn.stored.total_amt;
    }
    let sum: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM payment_line WHERE id_invoice = ?1",
            [&txn.id],
            |r| r.get(0),
        )
        .unwrap();
    Cents(sum)
}

fn sum_cents(db: &Connection, sql: &str) -> Cents {
    Cents(db.query_row(sql, [], |r| r.get(0)).unwrap())
}

#[test]
fn every_stored_line_and_total_recomputes() {
    let db = demo_db();
    let txns = transactions(&db);
    assert_eq!(txns.len(), 16);
    for txn in &txns {
        let lines = lines(&db, &txn.id);
        assert!(!lines.is_empty(), "{} has no lines", txn.id);
        for (line, amount, amount_taxable) in &lines {
            assert_eq!(line.amount().unwrap(), *amount, "line amount on {}", txn.id);
            let expected_taxable = if line.taxable { *amount } else { Cents::ZERO };
            assert_eq!(
                *amount_taxable, expected_taxable,
                "amount_taxable on {}",
                txn.id
            );
        }
        let only_lines: Vec<Line> = lines.iter().map(|(l, _, _)| *l).collect();
        let computed = totals(&only_lines, txn.tax_rate_bp, txn.ship_amt, paid(&db, txn)).unwrap();
        assert_eq!(computed, txn.stored, "totals of {}", txn.id);
    }
}

#[test]
fn every_customer_balance_and_retainer_balance_recomputes() {
    let db = demo_db();
    let mut balance: BTreeMap<String, Cents> = BTreeMap::new();
    let mut retainer: BTreeMap<String, Vec<Cents>> = BTreeMap::new();
    for txn in transactions(&db) {
        *balance.entry(txn.customer.clone()).or_default() += txn.stored.balance;
        if txn.locked {
            retainer
                .entry(txn.customer.clone())
                .or_default()
                .push(txn.stored.retainer);
        }
    }
    let mut stmt = db
        .prepare("SELECT id, balance, retainer_balance_cached, retainer_adjustment FROM customer")
        .unwrap();
    let customers: Vec<(String, i64, i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(customers.len(), 14);
    for (id, stored_balance, stored_retainer, adjustment) in customers {
        let expected_balance = balance.get(&id).copied().unwrap_or_default();
        assert_eq!(Cents(stored_balance), expected_balance, "balance of {id}");
        let posted = retainer.get(&id).cloned().unwrap_or_default();
        let expected_retainer = retainer_balance(posted, Cents(adjustment)).unwrap();
        assert_eq!(
            Cents(stored_retainer),
            expected_retainer,
            "retainer balance of {id}"
        );
    }
}

/// customers.html: balance due, overdue on 2026-10-28, retainer held.
#[test]
fn customers_screen_figures() {
    let db = demo_db();
    assert_eq!(
        sum_cents(&db, "SELECT SUM(balance) FROM customer").to_string(),
        "$29,619.05"
    );
    let overdue =
        "FROM sales_transaction WHERE invoice_ind = 1 AND balance > 0 AND due_date < '2026-10-28'";
    assert_eq!(
        sum_cents(&db, &format!("SELECT SUM(balance) {overdue}")).to_string(),
        "$4,457.40"
    );
    let overdue_customers: i64 = db
        .query_row(
            &format!("SELECT COUNT(DISTINCT id_customer) {overdue}"),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(overdue_customers, 3);
    assert_eq!(
        sum_cents(&db, "SELECT SUM(retainer_balance_cached) FROM customer").to_string(),
        "$4,450.00"
    );
    let holders: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM customer WHERE retainer_balance_cached > 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holders, 4);
}

/// customer.html (Halden & Pryce) and invoice.html (INV-1048).
#[test]
fn customer_and_invoice_screen_figures() {
    let db = demo_db();
    let hp = |col: &str| {
        sum_cents(
            &db,
            &format!("SELECT {col} FROM customer WHERE id = 'r-hp'"),
        )
    };
    assert_eq!(hp("balance").to_string(), "$3,057.15");
    assert_eq!(hp("retainer_balance_cached").to_string(), "$1,500.00");
    // Revenue in the last 12 months: the non-retainer line amounts.
    let revenue = sum_cents(
        &db,
        "SELECT SUM(l.amount) FROM sales_line l
         JOIN sales_transaction t ON t.id = l.sales_transaction_id
         JOIN item i ON i.id = l.item_id
         WHERE t.id_customer = 'r-hp' AND i.name <> 'Retainer' AND t.date > '2025-10-28'",
    );
    assert_eq!(revenue.to_string(), "$16,370.00");

    // The mockup shows the subtotal before the retainer credit and the credit below it.
    let inv = lines(&db, "t-inv-1048");
    let before_retainer: Cents = inv
        .iter()
        .filter(|(l, _, _)| !l.retainer)
        .map(|(_, a, _)| *a)
        .sum();
    let credit: Cents = inv
        .iter()
        .filter(|(l, _, _)| l.retainer)
        .map(|(_, a, _)| *a)
        .sum();
    let t = transactions(&db)
        .into_iter()
        .find(|t| t.id == "t-inv-1048")
        .unwrap()
        .stored;
    assert_eq!(before_retainer.to_string(), "$3,550.00");
    assert_eq!(t.taxable_subtotal.to_string(), "$130.00");
    assert_eq!(t.total_tax.to_string(), "$7.15");
    assert_eq!((-credit).to_string(), "$500.00");
    assert_eq!(t.balance.to_string(), "$2,057.15");
}
