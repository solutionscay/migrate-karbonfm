//! After the load: stored calculations, caches and totals are recomputed, not imported (D-39). A value the export had
//! and the recompute changed is listed in the report (import_changed_value); an empty exported value that is filled
//! is only counted.
//!
//! - sales lines: amount = qty x unit price, half away from zero (BR-SALES-01); amount_taxable;
//! - sales transactions: subtotal, taxable_subtotal, total_tax, total_amt, balance (BR-SALES-02), retainer
//!   (BR-SALES-07), period, paid_ind and the address displays (the source's stored calculations). The source has no
//!   tax rate: each transaction's rate is derived from the tax and taxable subtotal FileMaker stored, and the most
//!   common rate becomes system_setting.tax_rate_bp when it is empty (OD-07-06). A sales receipt is paid when sold;
//! - customers: retainer_balance_cached (BR-SALES-06) and balance (posted open invoices);
//! - party display caches (BR-PARTY-06) and the other name, status, count and flag caches.
//!
//! The party cache expressions here are the import's. When I2's commands own BR-PARTY-06, this should call theirs.

use super::{Changed, Report};
use karbon_money::sales::{self, Line, RETAINER_ITEM_NAME};
use karbon_money::{Cents, Qty};
use rusqlite::{params, Transaction};
use std::collections::BTreeMap;

type NameOf<'a> = dyn Fn(&str) -> Option<(String, String)> + 'a;

pub fn all(tx: &Transaction, report: &mut Report, name_of: &NameOf) -> rusqlite::Result<()> {
    let mut t = std::time::Instant::now();
    let mut lap = |report: &mut Report, name: &str| {
        report.timings.push((format!("recompute: {name}"), t.elapsed().as_millis()));
        t = std::time::Instant::now();
    };
    sales_lines_and_totals(tx, report, name_of)?;
    lap(report, "sales");
    customers(tx, report, name_of)?;
    lap(report, "customers");
    party_caches(tx, report, name_of)?;
    lap(report, "party caches");
    other_caches(tx, report, name_of)?;
    lap(report, "other caches");
    Ok(())
}

fn push_change(
    report: &mut Report,
    name_of: &NameOf,
    target: &str,
    key: &str,
    from: Option<String>,
    to: Option<String>,
    why: &str,
) {
    let (table, field) = name_of(target).unwrap_or_else(|| ("".into(), target.into()));
    report.changed.push(Changed {
        source_table: table,
        source_key: Some(key.to_string()),
        source_field: field,
        source_value: from,
        stored_value: to,
        reason: why.to_string(),
    });
}

fn money(c: Option<i64>) -> Option<String> {
    c.map(|c| Cents(c).to_input_value())
}

fn sales_lines_and_totals(tx: &Transaction, report: &mut Report, name_of: &NameOf) -> rusqlite::Result<()> {
    struct L {
        id: String,
        txn: String,
        line: Line,
        stored: Option<i64>,
    }
    let mut lines: Vec<L> = Vec::new();
    {
        let mut stmt = tx.prepare(
            "SELECT sl.id, sl.sales_transaction_id, CAST(sl.qty AS TEXT), sl.unit_price, sl.amount, COALESCE(sl.taxable, 0), \
             COALESCE(it.name, '') FROM sales_line sl LEFT JOIN item it ON it.id = sl.item_id ORDER BY sl.id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let qty_text: Option<String> = r.get(2)?;
            let qty = qty_text.as_deref().and_then(|q| Qty::parse_optional(q).ok()).flatten();
            let stored: Option<i64> = r.get(4)?;
            // A line with an amount and no unit price keeps its amount as the unit price of one (BR-SALES-01).
            let unit: i64 = r.get::<_, Option<i64>>(3)?.or(stored).unwrap_or(0);
            let taxable: f64 = r.get::<_, rusqlite::types::Value>(5).map(|v| match v {
                rusqlite::types::Value::Integer(i) => i as f64,
                rusqlite::types::Value::Real(f) => f,
                rusqlite::types::Value::Text(t) => t.parse().unwrap_or(0.0),
                _ => 0.0,
            })?;
            let name: String = r.get(6)?;
            lines.push(L {
                id: r.get(0)?,
                txn: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                line: Line {
                    qty,
                    unit_price: Cents(unit),
                    taxable: taxable != 0.0,
                    retainer: name == RETAINER_ITEM_NAME,
                },
                stored,
            });
        }
    }
    let mut by_txn: BTreeMap<String, Vec<Line>> = BTreeMap::new();
    {
        let mut upd =
            tx.prepare("UPDATE sales_line SET amount = ?2, amount_taxable = ?3, unit_price = ?4 WHERE id = ?1")?;
        for l in &lines {
            let amount = l.line.amount().map_err(|_| overflow("sales_line", &l.id))?;
            let taxable = if l.line.taxable { amount } else { Cents::ZERO };
            upd.execute(params![l.id, amount.0, taxable.0, l.line.unit_price.0])?;
            if l.stored.is_some_and(|s| s != amount.0) {
                push_change(
                    report,
                    name_of,
                    "sales_line.amount",
                    &l.id,
                    money(l.stored),
                    money(Some(amount.0)),
                    "line amount recomputed as quantity x unit price (BR-SALES-01)",
                );
            }
            by_txn.entry(l.txn.clone()).or_default().push(l.line);
        }
        *report.recomputed.entry("sales_line.amount".into()).or_default() += lines.len();
    }

    struct T {
        id: String,
        date: Option<String>,
        invoice: bool,
        receipt: bool,
        ship: i64,
        exported: [Option<i64>; 5], // subtotal, taxable_subtotal, total_tax, total_amt, balance
        /// line 1, line 2, city, state, postal code, country
        bill: [Option<String>; 6],
        ship_to: [Option<String>; 6],
        bp: Option<i64>,
    }
    let mut txns: Vec<T> = Vec::new();
    {
        let mut stmt = tx.prepare(
            "SELECT id, date, COALESCE(invoice_ind, 0) = 1, COALESCE(sales_receipt_ind, 0) = 1, COALESCE(ship_amt, 0), \
             subtotal, taxable_subtotal, total_tax, total_amt, balance, bill_address_line_1, bill_address_line_2, \
             bill_address_city, bill_address_state, bill_address_postal_code, bill_address_country, ship_address_line_1, \
             ship_address_line_2, ship_address_city, ship_address_state, ship_address_postal_code, ship_address_country, \
             tax_rate_bp FROM sales_transaction ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let s = |i: usize| r.get::<_, Option<String>>(i);
            txns.push(T {
                id: r.get(0)?,
                date: r.get(1)?,
                invoice: r.get(2)?,
                receipt: r.get(3)?,
                ship: r.get(4)?,
                exported: [r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?],
                bill: [s(10)?, s(11)?, s(12)?, s(13)?, s(14)?, s(15)?],
                ship_to: [s(16)?, s(17)?, s(18)?, s(19)?, s(20)?, s(21)?],
                bp: r.get(22)?,
            });
        }
    }
    // The tax rate of each transaction, from what FileMaker stored (the source left tax to QBO; OD-07-06).
    let mut votes: BTreeMap<i64, usize> = BTreeMap::new();
    let derived: Vec<Option<i64>> = txns
        .iter()
        .map(|t| {
            let (taxable, tax) = (t.exported[1]?, t.exported[2]?);
            if taxable <= 0 || tax <= 0 {
                return None;
            }
            let guess = (tax as i128 * 10_000 + taxable as i128 / 2) / taxable as i128;
            let bp = (guess - 2..=guess + 2)
                .map(|b| b as i64)
                .find(|b| sales::tax(Cents(taxable), *b).is_ok_and(|c| c.0 == tax))
                .unwrap_or(guess as i64);
            *votes.entry(bp).or_default() += 1;
            Some(bp)
        })
        .collect();
    let common = votes.iter().max_by_key(|(bp, n)| (**n, -**bp)).map(|(bp, _)| *bp);
    let system: Option<i64> =
        tx.query_row("SELECT tax_rate_bp FROM system_setting WHERE singleton = 1", [], |r| r.get(0)).unwrap_or(None);
    if system.is_none() {
        if let Some(bp) = common {
            if tx.execute("UPDATE system_setting SET tax_rate_bp = ?1 WHERE singleton = 1", [bp])? > 0 {
                *report.recomputed.entry("system_setting.tax_rate_bp".into()).or_default() += 1;
            }
        }
    }
    let default_bp = system.or(common).unwrap_or(0);
    let mut paid: BTreeMap<String, i64> = BTreeMap::new();
    {
        let mut stmt = tx.prepare("SELECT id_invoice, COALESCE(SUM(amount), 0) FROM payment_line WHERE id_invoice IS NOT NULL GROUP BY id_invoice")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (k, v) = row?;
            paid.insert(k, v);
        }
    }
    let mut upd = tx.prepare(
        "UPDATE sales_transaction SET subtotal = ?2, taxable_subtotal = ?3, total_tax = ?4, total_amt = ?5, balance = ?6, \
         retainer = ?7, tax_rate_bp = ?8, period = ?9, paid_ind = ?10, billing_address_display = ?11, \
         shipping_address_display = ?12 WHERE id = ?1",
    )?;
    let names = [
        "sales_transaction.subtotal",
        "sales_transaction.taxable_subtotal",
        "sales_transaction.total_tax",
        "sales_transaction.total_amt",
        "sales_transaction.balance",
    ];
    for (t, derived) in txns.iter().zip(derived) {
        let bp = t.bp.or(derived).unwrap_or(default_bp);
        let empty = Vec::new();
        let ls = by_txn.get(&t.id).unwrap_or(&empty);
        let mut tot =
            sales::totals(ls, bp, Cents(t.ship), Cents::ZERO).map_err(|_| overflow("sales_transaction", &t.id))?;
        tot.balance = if t.receipt && !t.invoice {
            Cents::ZERO
        } else {
            tot.total_amt
                .checked_sub(Cents(*paid.get(&t.id).unwrap_or(&0)))
                .ok_or_else(|| overflow("sales_transaction", &t.id))?
        };
        let period: Option<i64> =
            t.date.as_deref().and_then(|d| d.get(..7)).and_then(|ym| ym.replace('-', "").parse().ok());
        upd.execute(params![
            t.id,
            tot.subtotal.0,
            tot.taxable_subtotal.0,
            tot.total_tax.0,
            tot.total_amt.0,
            tot.balance.0,
            tot.retainer.0,
            bp,
            period,
            i64::from(tot.balance.0 == 0),
            display(&t.bill),
            display(&t.ship_to),
        ])?;
        let new = [tot.subtotal.0, tot.taxable_subtotal.0, tot.total_tax.0, tot.total_amt.0, tot.balance.0];
        for i in 0..5 {
            if t.exported[i].is_some_and(|e| e != new[i]) {
                let why = if i == 4 {
                    "balance recomputed as total less payments (BR-SALES-02)"
                } else {
                    "total recomputed from the lines (BR-SALES-02)"
                };
                push_change(report, name_of, names[i], &t.id, money(t.exported[i]), money(Some(new[i])), why);
            }
        }
    }
    *report.recomputed.entry("sales_transaction totals".into()).or_default() += txns.len();
    Ok(())
}

fn text(x: &Option<String>) -> Option<&str> {
    x.as_deref().filter(|s| !s.is_empty())
}

/// The source's address display: List(line 1; line 2; "city state postal"; country), one value per line.
fn display(a: &[Option<String>; 6]) -> Option<String> {
    let mut parts: Vec<&str> = a[..2].iter().filter_map(text).collect();
    let csp: Vec<&str> = a[2..5].iter().filter_map(text).collect();
    let csp = csp.join(" ");
    if !csp.is_empty() {
        parts.push(&csp);
    }
    parts.extend(text(&a[5]));
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn overflow(table: &str, id: &str) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(format!("{table} {id}: amount out of range").into())
}

/// Recomputes `table.column` as `expr` (SQL over the row `t`), lists the rows whose exported value differs, counts
/// the rows written.
#[allow(clippy::too_many_arguments)]
fn cache(
    tx: &Transaction,
    report: &mut Report,
    name_of: &NameOf,
    table: &str,
    key: &str,
    column: &str,
    expr: &str,
    why: &str,
) -> rusqlite::Result<()> {
    let target = format!("{table}.{column}");
    {
        let mut stmt = tx.prepare(&format!(
            "SELECT t.{key}, CAST(t.{column} AS TEXT), CAST(({expr}) AS TEXT) FROM \"{table}\" AS t \
             WHERE t.{column} IS NOT NULL AND t.{column} IS NOT ({expr}) ORDER BY t.{key}"
        ))?;
        let rows: Vec<(String, Option<String>, Option<String>)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        for (k, from, to) in rows {
            push_change(report, name_of, &target, &k, from, to, why);
        }
    }
    let n =
        tx.execute(&format!("UPDATE \"{table}\" AS t SET {column} = ({expr}) WHERE t.{column} IS NOT ({expr})"), [])?;
    *report.recomputed.entry(target).or_default() += n;
    Ok(())
}

fn customers(tx: &Transaction, report: &mut Report, name_of: &NameOf) -> rusqlite::Result<()> {
    let posted = "st.id_customer = t.id AND st.locked_ind = 1 AND COALESCE(st.deleted, 0) = 0";
    cache(
        tx,
        report,
        name_of,
        "customer",
        "id",
        "retainer_balance_cached",
        &format!("COALESCE(t.retainer_adjustment, 0) + COALESCE((SELECT SUM(st.retainer) FROM sales_transaction st WHERE {posted}), 0)"),
        "retainer balance recomputed from the posted transactions (BR-SALES-06)",
    )?;
    cache(
        tx,
        report,
        name_of,
        "customer",
        "id",
        "balance",
        &format!(
            "COALESCE((SELECT SUM(st.balance) FROM sales_transaction st WHERE {posted} AND st.invoice_ind = 1), 0)"
        ),
        "customer balance recomputed from the posted invoices (D-39)",
    )
}

/// BR-PARTY-06: name, primary phone, email and address of every party, with their labels and ids.
pub fn party_caches(tx: &Transaction, report: &mut Report, name_of: &NameOf) -> rusqlite::Result<()> {
    let why = "party cache recomputed (BR-PARTY-06)";
    let name = "COALESCE(CASE WHEN COALESCE(t.organization_ind, 0) = 1 \
                THEN (SELECT o.organization_name FROM organization o WHERE o.id = t.id) \
                ELSE (SELECT NULLIF(trim(COALESCE(pe.first_name, '') || ' ' || COALESCE(pe.last_name, '')), '') \
                      FROM person pe WHERE pe.id = t.id) END, t.name_cached)";
    cache(tx, report, name_of, "party", "id", "name_cached", name, why)?;
    let phone = |col: &str| {
        format!(
            "(SELECT {col} FROM contact_method cm JOIN phone p ON p.id = cm.id WHERE cm.party_id = t.id \
             ORDER BY COALESCE(p.primary_ind, 0) DESC, cm.creation_timestamp, cm.id LIMIT 1)"
        )
    };
    cache(tx, report, name_of, "party", "id", "phone_id_cached", &phone("cm.id"), why)?;
    cache(tx, report, name_of, "party", "id", "phone_number_cached", &phone("p.number"), why)?;
    cache(tx, report, name_of, "party", "id", "phone_label_cached", &phone("cm.label"), why)?;
    let email = |col: &str| {
        format!(
            "(SELECT {col} FROM contact_method cm JOIN network_address n ON n.id = cm.id WHERE cm.party_id = t.id \
             AND COALESCE(n.email_address_ind, 0) = 1 \
             ORDER BY COALESCE(n.primary_ind, 0) DESC, cm.creation_timestamp, cm.id LIMIT 1)"
        )
    };
    cache(tx, report, name_of, "party", "id", "email_address_id_cached", &email("cm.id"), why)?;
    cache(tx, report, name_of, "party", "id", "email_address_cached", &email("n.address"), why)?;
    cache(tx, report, name_of, "party", "id", "email_address_label_cached", &email("cm.label"), why)?;
    let address = |col: &str| {
        format!(
            "(SELECT {col} FROM contact_method cm JOIN address a ON a.id = cm.id WHERE cm.party_id = t.id \
             ORDER BY COALESCE(a.primary_ind, 0) DESC, cm.creation_timestamp, cm.id LIMIT 1)"
        )
    };
    let display =
        "COALESCE(a.address_line_1, '') || COALESCE(', ' || a.city, '') || COALESCE(', ' || a.state_province, '') \
                   || COALESCE(' ' || a.postal_code, '')";
    cache(tx, report, name_of, "party", "id", "address_id_cached", &address("cm.id"), why)?;
    cache(tx, report, name_of, "party", "id", "address_display_cached", &address(display), why)?;
    cache(tx, report, name_of, "party", "id", "address_label_cached", &address("cm.label"), why)
}

fn other_caches(tx: &Transaction, report: &mut Report, name_of: &NameOf) -> rusqlite::Result<()> {
    let why = "cache recomputed (D-39)";
    let party_name = |col: &str| format!("(SELECT p.name_cached FROM party p WHERE p.id = {col})");
    let role_party = "(SELECT p.name_cached FROM party_role r JOIN party p ON p.id = r.party_id WHERE r.id = t.id)";
    for table in ["employee", "vendor", "lead"] {
        cache(tx, report, name_of, table, "id", "name_cached", &format!("COALESCE({role_party}, t.name_cached)"), why)?;
    }
    cache(tx, report, name_of, "activity_party", "id", "party_name_cached", &party_name("t.party_id"), why)?;
    cache(tx, report, name_of, "opportunity", "id", "party_name_cached", &party_name("t.party_id"), why)?;
    cache(
        tx,
        report,
        name_of,
        "opportunity",
        "id",
        "referring_party_name_cached",
        &party_name("t.referring_party_id"),
        why,
    )?;
    let status = "(SELECT s.name FROM opportunity_status s WHERE s.id = t.opportunity_status_id)";
    cache(tx, report, name_of, "opportunity", "id", "status_cached", status, why)?;
    let calc = "stored calculation recomputed (D-39)";
    cache(
        tx,
        report,
        name_of,
        "opportunity",
        "id",
        "type_display",
        "CASE WHEN t.lead_id IS NOT NULL THEN 'Lead' ELSE 'Customer' END",
        calc,
    )?;
    for (col, kind) in [("billing_ind_cached", 1), ("shipping_ind_cached", 2), ("mailing_ind_cached", 3)] {
        let e = format!(
            "EXISTS (SELECT 1 FROM contact_method_purpose x WHERE x.contact_method_id = t.id AND x.contact_method_purpose_type_id = {kind})"
        );
        cache(tx, report, name_of, "contact_method", "id", col, &e, why)?;
    }
    cache(
        tx,
        report,
        name_of,
        "activity",
        "id",
        "comment_count_cached",
        "(SELECT COUNT(*) FROM activity_comment c WHERE c.activity_id = t.id)",
        why,
    )?;
    let by_role = |role: i64| {
        format!(
            "(SELECT group_concat(n, ', ') FROM (SELECT p.name_cached AS n FROM activity_party ap JOIN party p ON p.id = ap.party_id \
             WHERE ap.activity_id = t.id AND +ap.activity_party_role_id = {role} ORDER BY p.name_cached))"
        )
    };
    cache(tx, report, name_of, "activity", "id", "creator_cached", &by_role(50), why)?;
    cache(tx, report, name_of, "activity", "id", "parties_cached", &by_role(63), why)?;
    let digits =
        "(WITH RECURSIVE c(i, d) AS (SELECT 1, '' UNION ALL SELECT i + 1, d || CASE WHEN substr(t.number, i, 1) \
                  GLOB '[0-9]' THEN substr(t.number, i, 1) ELSE '' END FROM c WHERE i <= length(t.number)) \
                  SELECT NULLIF(d, '') FROM c ORDER BY i DESC LIMIT 1)";
    cache(tx, report, name_of, "phone", "id", "number_digits_only", digits, calc)?;
    cache(
        tx,
        report,
        name_of,
        "item",
        "id",
        "name_for_active_value_list",
        "CASE WHEN COALESCE(t.active, 0) <> 0 THEN t.name END",
        calc,
    )?;
    Ok(())
}
