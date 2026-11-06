//! The invoice document (UI-12 view, UI-20 PDF): one model that the HTML page and the PDF both render, so the
//! printed totals are the money crate's totals (BR-SALES-01, BR-SALES-02, BR-SALES-07; 03: "PDF matches totals").
//!
//! The stored totals are not read: the model recomputes them from the lines with `karbon_money::sales`. The
//! document shows the amount before the retainer credit and the credit below the total, as design/invoice.html
//! does; the balance due is the same as the stored one (04_demo_seed.sql header). Only a negative retainer line is a
//! credit: on a retainer receipt (RET-0042) the positive line is the sale itself, an ordinary line of the document.

use karbon_money::sales::{self, Totals, RETAINER_ITEM_NAME};
use karbon_money::{Cents, Qty};
use rusqlite::{Connection, OptionalExtension};

/// The document number of the long-invoice fixture (S-02): INV-1048 with 60 lines, for page-break tests. Tests build
/// it with [`long_fixture`]; it is not a page of the app.
pub const LONG_FIXTURE: &str = "FIXTURE-60";

#[derive(Clone, Debug)]
pub struct Seller {
    pub name: String,
    pub tagline: String,
    pub address: String,
    pub contact: String,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub description: String,
    pub item: String,
    pub qty: Option<Qty>,
    pub rate: Cents,
    pub amount: Cents,
    pub taxable: bool,
    pub retainer: bool,
}

#[derive(Clone, Debug)]
pub struct Payment {
    pub date: String,
    pub method: String,
    pub amount: Cents,
}

#[derive(Clone, Debug)]
pub struct Invoice {
    pub id: String,
    pub party_id: String,
    pub locked: bool,
    /// False for a sales receipt.
    pub is_invoice: bool,
    /// The derived status (`sales_list::STATUS_SQL`; X13: the `status` column is not read).
    pub state: crate::sales_list::Status,
    pub doc_number: String,
    pub status: String,
    pub date: String,
    pub due_date: String,
    pub terms: String,
    pub tax_rate_bp: i64,
    pub seller: Seller,
    pub customer: String,
    pub bill_to: Vec<String>,
    /// The stored ship-to address; empty when the transaction has none.
    pub ship_to: Vec<String>,
    /// Shipping (field 41): in the total, not taxed.
    pub ship_amt: Cents,
    pub memo: String,
    /// Every line, retainer credit lines included, in entry order.
    pub lines: Vec<Line>,
    pub payments: Vec<Payment>,
    pub totals: Totals,
}

impl Invoice {
    /// Loads an invoice by document number and recomputes its totals, with the status as of the local date.
    pub fn load(conn: &Connection, doc_number: &str) -> rusqlite::Result<Invoice> {
        Invoice::load_at(conn, doc_number, &crate::audit::local_now()[..10])
    }

    /// [`Invoice::load`], with the status as of `today` (`AppState::today`).
    pub fn load_at(conn: &Connection, doc_number: &str, today: &str) -> rusqlite::Result<Invoice> {
        let sql = format!(
            "SELECT st.id, ({}), COALESCE(st.date, ''), COALESCE(st.due_date, ''), COALESCE(i.id_term, ''),
                    COALESCE(st.tax_rate_bp, 0), p.name_cached, p.id, COALESCE(st.customer_memo, ''),
                    COALESCE(st.locked_ind, 0) <> 0, COALESCE(st.invoice_ind, 0) = 1
             FROM sales_transaction st JOIN party_role pr ON pr.id = st.id_customer JOIN party p ON p.id = pr.party_id
             LEFT JOIN invoice i ON i.id = st.id
             WHERE st.doc_number = ?2 AND COALESCE(st.deleted, 0) = 0",
            crate::sales_list::STATUS_SQL
        );
        let (id, state, date, due_date, terms, tax_rate_bp, customer, party_id, memo, locked, is_invoice) = conn
            .query_row(&sql, [today, doc_number], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    crate::sales_list::Status::parse(&r.get::<_, String>(1)?),
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get::<_, String>(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                ))
            })?;

        // Quantity is read as text and parsed as fixed-point, never multiplied as f64 (S-03 finding 1).
        let mut stmt = conn.prepare(
            "SELECT COALESCE(sl.description, ''), COALESCE(it.sku, ''), COALESCE(it.name, ''), CAST(sl.qty AS TEXT),
                    COALESCE(sl.unit_price, 0), COALESCE(sl.taxable, 0)
             FROM sales_line sl LEFT JOIN item it ON it.id = sl.item_id
             WHERE sl.sales_transaction_id = ?1 ORDER BY sl.rowid",
        )?;
        let lines = stmt
            .query_map([&id], |r| {
                let qty: Option<String> = r.get(3)?;
                let qty = qty.map(|q| Qty::parse(&q)).transpose().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
                })?;
                let item_name: String = r.get(2)?;
                Ok(line(r.get(0)?, r.get(1)?, qty, Cents(r.get(4)?), r.get::<_, i64>(5)? != 0, item_name))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut stmt = conn.prepare(
            "SELECT p.date, COALESCE(p.payment_ref_num, p.id_payment_method, ''), pl.amount
             FROM payment_line pl JOIN payment p ON p.id = pl.id_payment WHERE pl.id_invoice = ?1 ORDER BY p.date",
        )?;
        let payments = stmt
            .query_map([&id], |r| Ok(Payment { date: r.get(0)?, method: r.get(1)?, amount: Cents(r.get(2)?) }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // F3: the address stored on the transaction (layout 143, field 81). An older row with none (the seed) falls
        // back to the party's address.
        let (billing, shipping, ship_amt): (String, String, i64) = conn.query_row(
            "SELECT COALESCE(billing_address_display, ''), COALESCE(shipping_address_display, ''), COALESCE(ship_amt, 0)
             FROM sales_transaction WHERE id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut bill_to = stored_lines(&billing);
        if bill_to.is_empty() {
            bill_to = conn
                .query_row(
                    "SELECT a.address_line_1, a.address_line_2, a.city, a.state_province, a.postal_code
                     FROM contact_method cm JOIN address a ON a.id = cm.id
                     WHERE cm.party_id = ?1 AND cm.address_ind = 1 ORDER BY cm.billing_ind_cached DESC LIMIT 1",
                    [&party_id],
                    address_lines,
                )
                .optional()?
                .unwrap_or_default();
        }

        let seller = conn
            .query_row(
                "SELECT p.id, p.name_cached, COALESCE(o.company_type, ''), COALESCE(p.email_address_cached, ''),
                        COALESCE(p.phone_number_cached, '')
                 FROM system_setting s JOIN party p ON p.id = s.owner_id LEFT JOIN organization o ON o.id = p.id",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)),
            )
            .optional()?;
        let seller = match seller {
            Some((party, name, tagline, email, phone)) => {
                let address = conn
                    .query_row(
                        "SELECT a.address_line_1, a.address_line_2, a.city, a.state_province, a.postal_code
                         FROM contact_method cm JOIN address a ON a.id = cm.id
                         WHERE cm.party_id = ?1 AND cm.address_ind = 1 LIMIT 1",
                        [&party],
                        address_lines,
                    )
                    .optional()?
                    .unwrap_or_default()
                    .join(", ");
                let contact = [email, phone].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                Seller { name, tagline, address, contact }
            }
            None => {
                Seller { name: String::new(), tagline: String::new(), address: String::new(), contact: String::new() }
            }
        };

        let mut inv = Invoice {
            id,
            party_id,
            locked,
            is_invoice,
            state,
            doc_number: doc_number.to_string(),
            status: state.label().to_owned(),
            date,
            due_date,
            terms,
            tax_rate_bp,
            seller,
            customer,
            bill_to,
            ship_to: stored_lines(&shipping),
            ship_amt: Cents(ship_amt),
            memo,
            lines,
            payments,
            totals: Totals::default(),
        };
        inv.recompute().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(inv)
    }

    /// Recomputes the totals from the lines, the tax rate, shipping and the payments (BR-SALES-02). A sales receipt
    /// is paid when sold, so its balance is 0 (X12, field 57).
    pub fn recompute(&mut self) -> Result<(), sales::Overflow> {
        let lines: Vec<sales::Line> = self
            .lines
            .iter()
            .map(|l| sales::Line { qty: l.qty, unit_price: l.rate, taxable: l.taxable, retainer: l.retainer })
            .collect();
        self.totals = sales::totals(&lines, self.tax_rate_bp, self.ship_amt, self.paid())?;
        if !self.is_invoice {
            self.totals.balance = Cents::ZERO;
        }
        Ok(())
    }

    /// The lines shown in the line table: every line except the retainer credits, which show below the total.
    pub fn item_lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| !(l.retainer && l.amount < Cents::ZERO))
    }

    /// Subtotal before the retainer credit ($3,550.00 on INV-1048).
    pub fn subtotal_before_retainer(&self) -> Cents {
        self.totals.subtotal - self.retainer()
    }

    /// Total before the retainer credit ($3,557.15 on INV-1048): the "Total" of the design, shipping included.
    pub fn total_before_retainer(&self) -> Cents {
        self.subtotal_before_retainer() + self.totals.total_tax + self.ship_amt
    }

    /// The document's name: "Invoice", or "Sales receipt" (06 UI-20).
    pub fn kind_label(&self) -> &'static str {
        if self.is_invoice {
            "Invoice"
        } else {
            "Sales receipt"
        }
    }

    /// The retainer credit, negative (-$500.00 on INV-1048): the negative retainer lines.
    pub fn retainer(&self) -> Cents {
        self.lines.iter().filter(|l| l.retainer && l.amount < Cents::ZERO).map(|l| l.amount).sum()
    }

    pub fn paid(&self) -> Cents {
        self.payments.iter().map(|p| p.amount).sum()
    }

    /// Balance due ($2,057.15 on INV-1048).
    pub fn balance(&self) -> Cents {
        self.totals.balance
    }

    /// "5.5%" for 550 basis points.
    pub fn tax_rate_label(&self) -> String {
        let bp = self.tax_rate_bp;
        let whole = bp / 100;
        let frac = (bp % 100).abs();
        match frac {
            0 => format!("{whole}%"),
            f if f % 10 == 0 => format!("{whole}.{}%", f / 10),
            f => format!("{whole}.{f:02}%"),
        }
    }
}

impl Line {
    /// A line with no item carries text only (SalesLine field 6): no quantity, rate or amount to show.
    pub fn description_only(&self) -> bool {
        self.item.is_empty() && self.rate.0 == 0 && self.amount.0 == 0
    }

    /// The quantity as the document shows it: "2", "1.5"; empty quantity counts as 1 (BR-SALES-01).
    pub fn qty_text(&self) -> String {
        self.qty.unwrap_or(Qty::ONE).to_string()
    }
}

/// "Oct 14, 2026" for "2026-10-14"; anything else is returned as it is.
pub fn display_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut parts = iso.splitn(3, '-');
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else { return iso.to_string() };
    match (m.parse::<usize>(), d.parse::<u32>()) {
        (Ok(m @ 1..=12), Ok(d)) if y.len() == 4 => format!("{} {d}, {y}", MONTHS[m - 1]),
        _ => iso.to_string(),
    }
}

/// A D-05 stamp ("2026-11-01 19:40:30") as the pages show dates: "Nov 1, 2026 · 19:40".
pub fn display_stamp(ts: &str) -> String {
    match (ts.get(..10), ts.get(11..16)) {
        (Some(d), Some(t)) => format!("{} · {t}", display_date(d)),
        _ => display_date(ts),
    }
}

fn line(description: String, item: String, qty: Option<Qty>, rate: Cents, taxable: bool, item_name: String) -> Line {
    let amount = sales::line_amount(qty, rate).unwrap_or(Cents::ZERO);
    Line { description, item, qty, rate, amount, taxable, retainer: item_name == RETAINER_ITEM_NAME }
}

/// The lines of a stored address display.
fn stored_lines(display: &str) -> Vec<String> {
    display.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

fn address_lines(r: &rusqlite::Row<'_>) -> rusqlite::Result<Vec<String>> {
    let l1: Option<String> = r.get(0)?;
    let l2: Option<String> = r.get(1)?;
    let city: Option<String> = r.get(2)?;
    let st: Option<String> = r.get(3)?;
    let zip: Option<String> = r.get(4)?;
    let last = format!("{}, {} {}", city.unwrap_or_default(), st.unwrap_or_default(), zip.unwrap_or_default());
    Ok([l1, l2, Some(last)].into_iter().flatten().filter(|s| !s.trim().trim_matches(',').trim().is_empty()).collect())
}

/// INV-1048 with `n` item lines (cycling its four items with varied quantities and some long descriptions) and its
/// retainer credit and payment, for page-break tests. Totals are recomputed.
pub fn long_fixture(mut base: Invoice, n: usize) -> Invoice {
    let items: Vec<Line> = base.item_lines().cloned().collect();
    let retainer: Vec<Line> = base.lines.iter().filter(|l| l.retainer).cloned().collect();
    let mut lines = Vec::with_capacity(n + retainer.len());
    for i in 0..n {
        let mut l = items[i % items.len()].clone();
        l.qty = Qty::from_whole(1 + (i as i64 * 7) % 9);
        if i % 9 == 4 {
            l.description = format!(
                "{} — extended scope, revision {}: coordination with the client team, consultants and contractor",
                l.description,
                i + 1
            );
        } else {
            l.description = format!("{} (week {})", l.description, i / 4 + 1);
        }
        l.amount = sales::line_amount(l.qty, l.rate).unwrap_or(Cents::ZERO);
        lines.push(l);
    }
    lines.extend(retainer);
    base.doc_number = LONG_FIXTURE.to_string();
    base.lines = lines;
    base.recompute().expect("fixture totals fit in i64");
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_rates() {
        assert_eq!(display_date("2026-10-14"), "Oct 14, 2026");
        assert_eq!(display_date(""), "");
        let mut inv = fixture();
        assert_eq!(inv.tax_rate_label(), "5.5%");
        inv.tax_rate_bp = 600;
        assert_eq!(inv.tax_rate_label(), "6%");
        inv.tax_rate_bp = 825;
        assert_eq!(inv.tax_rate_label(), "8.25%");
    }

    fn fixture() -> Invoice {
        let conn = crate::db::open_demo(None).unwrap();
        Invoice::load(&conn, "INV-1048").unwrap()
    }
}
