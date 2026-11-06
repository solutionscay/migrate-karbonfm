//! The sales money rules through the commands, on `Seed::Demo`: stored values are checked, in integer cents
//! (BR-SALES-01, 02, 03, 06, 07, 18; AP-05). S4 adds the payment and retainer fixtures.

use karbon_server::admin;
use karbon_server::audit::Actor;
use karbon_server::db::{self, Seed};
use karbon_server::sales::{self, Kind, TxnInput};
use rusqlite::Connection;

const TODAY: &str = "2026-11-06";

fn demo() -> (Connection, Actor) {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let actor = admin::current_user(&conn).unwrap().unwrap();
    (conn, actor)
}

fn int(conn: &Connection, sql: &str, id: &str) -> i64 {
    conn.query_row(sql, [id], |r| r.get(0)).unwrap()
}

/// A new draft for the customer with `n` empty lines, and its form as the editor would send it.
fn draft(conn: &mut Connection, a: &Actor, kind: Kind, customer: &str, n: usize) -> (String, TxnInput) {
    let (id, _) = sales::transaction_create(conn, kind, customer, TODAY, a).unwrap();
    for _ in 0..n {
        let input = sales::transaction_get(conn, &id).unwrap().input;
        sales::transaction_save(conn, kind, &id, &input, true, a).unwrap();
    }
    let input = sales::transaction_get(conn, &id).unwrap().input;
    (id, input)
}

/// Fills a line: (item, qty, unit price).
fn set(input: &mut TxnInput, i: usize, item: &str, qty: &str, price: &str) {
    let l = &mut input.lines[i];
    l.item_id = item.into();
    l.qty = qty.into();
    l.unit_price = price.into();
}

fn totals(conn: &Connection, id: &str) -> (i64, i64, i64, i64, i64, i64) {
    conn.query_row(
        "SELECT subtotal, taxable_subtotal, total_tax, total_amt, balance, retainer FROM sales_transaction WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )
    .unwrap()
}

#[test]
fn fx_inv_1048_totals() {
    // INV-1048's lines, typed into a new invoice for the same customer. The seed's own row carries the same figures.
    let (mut c, a) = demo();
    let (id, mut f) = draft(&mut c, &a, Kind::Invoice, "r-hp", 5);
    f.tax_rate = "5.5".into();
    set(&mut f, 0, "i-svc-workshop", "2", "1200.00");
    set(&mut f, 1, "i-svc-survey", "1", "450.00");
    set(&mut f, 2, "i-svc-hourly", "6", "95.00");
    set(&mut f, 3, "i-prd-print-a1", "4", "32.50");
    set(&mut f, 4, "i-retainer", "1", "-500.00");
    sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap();
    // The print item is taxable by the item (field 11); the others are not.
    assert_eq!(totals(&c, &id), (305_000, 13_000, 715, 305_715, 305_715, -50_000));
    let seeded = int(&c, "SELECT total_amt FROM sales_transaction WHERE doc_number = ?1", "INV-1048");
    assert_eq!(seeded, 305_715);
}

#[test]
fn fx_line_rounding() {
    let (mut c, a) = demo();
    let (id, mut f) = draft(&mut c, &a, Kind::Invoice, "r-bm", 5);
    set(&mut f, 0, "i-svc-workshop", "2", "1200.00");
    set(&mut f, 1, "i-svc-design", "1.5", "33.33"); // 4999.5 rounds up
    set(&mut f, 2, "i-svc-design", "0.5", "0.05"); // 2.5 rounds up
    set(&mut f, 3, "i-svc-design", "0.5", "-0.05"); // -2.5 rounds away from zero
    set(&mut f, 4, "i-svc-design", "", "450.00"); // an empty quantity counts as 1
    sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap();
    let amounts: Vec<i64> = {
        let mut stmt =
            c.prepare("SELECT amount FROM sales_line WHERE sales_transaction_id = ?1 ORDER BY rowid").unwrap();
        let rows = stmt.query_map([&id], |r| r.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(amounts, [240_000, 5_000, 3, -3, 45_000]);

    // Quantity 1.33333 is refused, nothing is written.
    set(&mut f, 0, "i-svc-workshop", "1.33333", "1200.00");
    let e = sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap_err();
    assert!(matches!(e, admin::CommandError::Validation(_)), "{e}");
    assert_eq!(int(&c, "SELECT subtotal FROM sales_transaction WHERE id = ?1", &id), 290_000);
}

#[test]
fn fx_tax_rounded_once_on_subtotal() {
    let (mut c, a) = demo();
    let (id, mut f) = draft(&mut c, &a, Kind::Invoice, "r-bm", 3);
    f.tax_rate = "5.5".into();
    for i in 0..3 {
        set(&mut f, i, "i-prd-print-a1", "1", "1.00");
    }
    sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap();
    assert_eq!(int(&c, "SELECT total_tax FROM sales_transaction WHERE id = ?1", &id), 17, "not 3 x 6 = 18");
}

#[test]
fn fx_rate_change_per_invoice() {
    let (mut c, a) = demo();
    // A new transaction copies the rate of Settings.
    let (id, mut f) = draft(&mut c, &a, Kind::Invoice, "r-bm", 1);
    assert_eq!(int(&c, "SELECT tax_rate_bp FROM sales_transaction WHERE id = ?1", &id), 550);
    f.tax_rate = "8.25".into();
    set(&mut f, 0, "i-prd-print-a1", "1", "100.00");
    sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap();
    assert_eq!(int(&c, "SELECT tax_rate_bp FROM sales_transaction WHERE id = ?1", &id), 825);
    assert_eq!(int(&c, "SELECT total_tax FROM sales_transaction WHERE id = ?1", &id), 825);
    // Settings changing later leaves this invoice alone; a new one takes the new rate.
    c.execute("UPDATE system_setting SET tax_rate_bp = 600", []).unwrap();
    let again = sales::transaction_get(&c, &id).unwrap().input;
    sales::transaction_save(&mut c, Kind::Invoice, &id, &again, false, &a).unwrap();
    assert_eq!(int(&c, "SELECT total_tax FROM sales_transaction WHERE id = ?1", &id), 825);
    let (next, _) = sales::transaction_create(&mut c, Kind::Invoice, "r-bm", TODAY, &a).unwrap();
    assert_eq!(int(&c, "SELECT tax_rate_bp FROM sales_transaction WHERE id = ?1", &next), 600);
}

#[test]
fn fx_shipping_in_total() {
    let (mut c, a) = demo();
    let (id, mut f) = draft(&mut c, &a, Kind::Invoice, "r-bm", 1);
    set(&mut f, 0, "i-prd-print-a1", "1", "10.00");
    f.ship_amt = "2.50".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap();
    // 1000 + tax 55 + shipping 250. Shipping is not taxed.
    assert_eq!(totals(&c, &id), (1_000, 1_000, 55, 1_305, 1_305, 0));
}

#[test]
fn fx_description_only_line_adds_nothing() {
    let (mut c, a) = demo();
    let (id, mut f) = draft(&mut c, &a, Kind::Invoice, "r-bm", 2);
    set(&mut f, 0, "i-svc-survey", "1", "450.00");
    f.lines[1].description = "Site visit notes".into();
    f.lines[1].qty = "3".into();
    f.lines[1].unit_price = "99.00".into();
    sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap();
    assert_eq!(totals(&c, &id).3, 45_000);
    let (qty, price, amount): (Option<f64>, Option<i64>, i64) = c
        .query_row("SELECT qty, unit_price, amount FROM sales_line WHERE description = 'Site visit notes'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((qty, price, amount), (None, None, 0));
}

#[test]
fn fx_period() {
    let (mut c, a) = demo();
    let (id, mut f) = draft(&mut c, &a, Kind::Invoice, "r-bm", 0);
    for (date, period) in [("2027-01-31", 202701), ("2026-12-01", 202612)] {
        f.date = date.into();
        sales::transaction_save(&mut c, Kind::Invoice, &id, &f, false, &a).unwrap();
        assert_eq!(int(&c, "SELECT period FROM sales_transaction WHERE id = ?1", &id), period);
    }
}

#[test]
fn fx_receipt_balance_is_zero() {
    let (mut c, a) = demo();
    let (id, mut f) = draft(&mut c, &a, Kind::Receipt, "r-bm", 1);
    set(&mut f, 0, "i-svc-survey", "1", "450.00");
    f.payment_ref_num = "Card".into();
    sales::transaction_save(&mut c, Kind::Receipt, &id, &f, false, &a).unwrap();
    let t = totals(&c, &id);
    assert_eq!((t.3, t.4), (45_000, 0));
    assert_eq!(int(&c, "SELECT paid_ind FROM sales_transaction WHERE id = ?1", &id), 1);
    assert_eq!(int(&c, "SELECT COUNT(*) FROM sales_receipt WHERE id = ?1 AND payment_ref_num = 'Card'", &id), 1);
}

#[test]
fn fx_retainer_balance_recomputed() {
    let (mut c, a) = demo();
    let held = |c: &Connection| int(c, "SELECT retainer_balance_cached FROM customer WHERE id = ?1", "r-hp");
    assert_eq!(held(&c), 150_000);
    let inv = sales::id_of_doc(&c, "INV-1048").unwrap().unwrap();
    // Posting twice changes nothing (BR-SALES-06; 576 s86 counted it twice: D-07).
    sales::transaction_lock(&mut c, &inv, true, &a).unwrap();
    sales::transaction_lock(&mut c, &inv, true, &a).unwrap();
    assert_eq!(held(&c), 150_000);
    // Unposted, its -$500.00 credit no longer counts (OD-07-07).
    sales::transaction_lock(&mut c, &inv, false, &a).unwrap();
    assert_eq!(held(&c), 200_000);
    // A new unposted retainer line does not count; posted, it does.
    let (id, mut f) = draft(&mut c, &a, Kind::Receipt, "r-hp", 1);
    set(&mut f, 0, "i-retainer", "1", "100.00");
    sales::transaction_save(&mut c, Kind::Receipt, &id, &f, false, &a).unwrap();
    assert_eq!(held(&c), 200_000);
    sales::transaction_lock(&mut c, &id, true, &a).unwrap();
    assert_eq!(held(&c), 210_000);
}
