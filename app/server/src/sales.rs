//! Invoices and sales receipts: OP-SALES-04 `invoice_save`, OP-SALES-05 `sales_receipt_save`, OP-SALES-06
//! `sales_transaction_lock`, OP-SALES-07 `sales_transaction_delete`, OP-SALES-08 `sales_lines_remove`. Plain SQL, one
//! `BEGIN IMMEDIATE` transaction per command. Totals come only from the lines (BR-SALES-02); a locked transaction refuses
//! every change (AP-05). `sales_transaction`, `sales_line` and `sales_receipt` have no stamp columns in 04; `invoice` has.
//! Payment rows are never written here (AP-06).

use crate::admin::{parse_percent_bp, CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::ids::new_id;
use karbon_money::sales::{self, Line, Totals, RETAINER_ITEM_NAME};
use karbon_money::{Cents, Qty};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

/// Longest document number (SalesTransaction field 4).
pub const DOC_NUMBER_MAX: usize = 21;
/// The longest description of a line (SalesLine field 5).
pub const DESCRIPTION_MAX: usize = 4000;

/// An invoice or a sales receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Invoice,
    Receipt,
}

impl Kind {
    /// The document number's prefix (Q1).
    pub fn prefix(self) -> &'static str {
        match self {
            Kind::Invoice => "INV-",
            Kind::Receipt => "SR-",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Invoice => "Invoice",
            Kind::Receipt => "Sales receipt",
        }
    }
    /// The route of the save command (05).
    pub fn path(self) -> &'static str {
        match self {
            Kind::Invoice => "/invoices",
            Kind::Receipt => "/sales-receipts",
        }
    }
}

/// One line as the form has it. Numbers stay text until the save parses them, so a refused value goes back unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    pub id: String,
    pub item_id: String,
    pub description: String,
    pub qty: String,
    pub unit_price: String,
    pub taxable: bool,
}

/// The form of an invoice or a sales receipt (layouts 69, 94). It has no total, balance or payment field: BR-SALES-02,
/// AP-06.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TxnInput {
    pub doc_number: String,
    pub date: String,
    pub due_date: String,
    pub term: String,
    pub tax_rate: String,
    pub ship_amt: String,
    pub ship_date: String,
    pub customer_memo: String,
    pub private_note: String,
    pub bill_email_address: String,
    pub bill: Address,
    pub ship: Address,
    pub id_account_deposit: String,
    pub payment_ref_num: String,
    pub lines: Vec<LineInput>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Address {
    pub line_1: String,
    pub line_2: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

/// A transaction as the editor shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxnEdit {
    pub id: String,
    pub kind: Kind,
    pub locked: bool,
    pub customer: String,
    pub party_id: String,
    pub customer_role_id: String,
    pub input: TxnInput,
    /// The amount of each line of `input.lines`, in order.
    pub amounts: Vec<Cents>,
    pub totals: Totals,
    pub paid: Cents,
    pub tax_rate_bp: i64,
}

fn field(field: &'static str, message: impl Into<String>) -> FieldError {
    FieldError { field, message: message.into() }
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// The next document number of a kind: the highest number used + 1 (Q1: only the QBO scripts 414 and 447 filled it).
pub fn next_doc_number(conn: &Connection, kind: Kind) -> rusqlite::Result<String> {
    let prefix = kind.prefix();
    let highest: Option<String> = conn
        .query_row(
            "SELECT substr(doc_number, ?1) FROM sales_transaction WHERE doc_number GLOB ?2
             ORDER BY CAST(substr(doc_number, ?1) AS INTEGER) DESC LIMIT 1",
            params![prefix.len() as i64 + 1, format!("{prefix}[0-9]*")],
            |r| r.get(0),
        )
        .optional()?;
    let digits = highest.as_deref().unwrap_or("1000");
    let n: i64 = digits.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(1000);
    // SR-0311 is followed by SR-0312: the width stays.
    Ok(format!("{prefix}{:0width$}", n + 1, width = digits.len()))
}

/// The date `Net N` days after `date` (Q2); none for another term or a date that does not parse.
pub fn due_from_term(date: &str, term: &str) -> Option<String> {
    let days: i64 = term.trim().strip_prefix("Net ")?.trim().parse().ok()?;
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some((d + chrono::Duration::days(days)).format("%Y-%m-%d").to_string())
}

/// The customer's flagged address (Party_ContactMethod#Billing_Address, #Shipping_Address).
fn flagged_address(conn: &Connection, party_id: &str, flag: &str) -> rusqlite::Result<Address> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT COALESCE(a.address_line_1, ''), COALESCE(a.address_line_2, ''), COALESCE(a.city, ''),
                        COALESCE(a.state_province, ''), COALESCE(a.postal_code, ''), COALESCE(a.country, '')
                 FROM contact_method cm JOIN address a ON a.id = cm.id
                 WHERE cm.party_id = ?1 AND cm.{flag} = 1 ORDER BY cm.rowid LIMIT 1"
            ),
            [party_id],
            |r| {
                Ok(Address {
                    line_1: r.get(0)?,
                    line_2: r.get(1)?,
                    city: r.get(2)?,
                    state: r.get(3)?,
                    postal_code: r.get(4)?,
                    country: r.get(5)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

/// Creates a draft for a customer (kontacts 272 s10, 514 s4): date today, the customer's addresses, the tax rate of
/// Settings (OD-07-06), the customer's term (Q2). It is not posted (OD-07-05). Returns (id, document number).
pub fn transaction_create(
    conn: &mut Connection,
    kind: Kind,
    customer_role_id: &str,
    today: &str,
    actor: &Actor,
) -> Result<(String, String), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let customer: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT r.party_id, c.sales_term_ref FROM customer c JOIN party_role r ON r.id = c.id WHERE c.id = ?1",
            [customer_role_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((party_id, term)) = customer else {
        return Err(CommandError::Validation(vec![field("customer_id", "Choose a customer.")]));
    };
    let bill = flagged_address(&tx, &party_id, "billing_ind_cached")?;
    let ship = flagged_address(&tx, &party_id, "shipping_ind_cached")?;
    let bp: i64 = tx
        .query_row("SELECT COALESCE(tax_rate_bp, 0) FROM system_setting WHERE singleton = 1", [], |r| r.get(0))
        .optional()?
        .unwrap_or(0);
    let id = new_id();
    let doc = next_doc_number(&tx, kind)?;
    let invoice = kind == Kind::Invoice;
    let due = if invoice { term.as_deref().and_then(|t| due_from_term(today, t)) } else { None };
    let ship_date = (!invoice).then(|| today.to_owned());
    tx.execute(
        "INSERT INTO sales_transaction (id, doc_number, date, due_date, id_customer, tax_rate_bp, status, deleted,
            invoice_ind, sales_receipt_ind, locked_ind, ship_date,
            bill_address_line_1, bill_address_line_2, bill_address_city, bill_address_state, bill_address_postal_code,
            bill_address_country,
            ship_address_line_1, ship_address_line_2, ship_address_city, ship_address_state, ship_address_postal_code,
            ship_address_country)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'Draft', 0, ?7, ?8, 0, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        params![
            id,
            doc,
            today,
            due,
            customer_role_id,
            bp,
            i64::from(invoice),
            i64::from(!invoice),
            ship_date,
            bill.line_1,
            bill.line_2,
            bill.city,
            bill.state,
            bill.postal_code,
            bill.country,
            ship.line_1,
            ship.line_2,
            ship.city,
            ship.state,
            ship.postal_code,
            ship.country,
        ],
    )?;
    let stamp = Stamp::now(actor);
    if invoice {
        tx.execute(
            "INSERT INTO invoice (id, id_term, created_by, modified_by) VALUES (?1, ?2, ?3, ?3)",
            params![id, term, actor.account_name],
        )?;
        audit::stamp_created(&tx, "invoice", &id, &stamp)?;
    } else {
        tx.execute("INSERT INTO sales_receipt (id) VALUES (?1)", [&id])?;
    }
    recompute_totals(&tx, &id)?;
    tx.commit()?;
    Ok((id, doc))
}

struct Head {
    kind: Kind,
    locked: bool,
    customer: Option<String>,
}

fn head(conn: &Connection, id: &str) -> Result<Head, CommandError> {
    conn.query_row(
        "SELECT COALESCE(invoice_ind, 0) = 1, COALESCE(locked_ind, 0) <> 0, id_customer FROM sales_transaction
         WHERE id = ?1 AND COALESCE(deleted, 0) = 0",
        [id],
        |r| {
            Ok(Head {
                kind: if r.get::<_, bool>(0)? { Kind::Invoice } else { Kind::Receipt },
                locked: r.get(1)?,
                customer: r.get(2)?,
            })
        },
    )
    .optional()?
    .ok_or(CommandError::NotFound)
}

/// A valid `YYYY-MM-DD`, or empty.
fn check_date(errors: &mut Vec<FieldError>, name: &'static str, value: &str) {
    if let Some(v) = opt(value) {
        if chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_err() {
            errors.push(field(name, "Enter a date as YYYY-MM-DD."));
        }
    }
}

/// The stored line, to tell whether the form changed its item.
struct StoredLine {
    item_id: Option<String>,
    description: String,
    unit_price: String,
}

fn stored_lines(conn: &Connection, id: &str) -> rusqlite::Result<Vec<(String, StoredLine)>> {
    let mut stmt = conn.prepare(
        "SELECT id, item_id, COALESCE(description, ''), unit_price FROM sales_line WHERE sales_transaction_id = ?1
         ORDER BY rowid",
    )?;
    let rows = stmt.query_map([id], |r| {
        let price: Option<i64> = r.get(3)?;
        Ok((
            r.get::<_, String>(0)?,
            StoredLine {
                item_id: r.get(1)?,
                description: r.get(2)?,
                unit_price: price.map(|p| Cents(p).to_input_value()).unwrap_or_default(),
            },
        ))
    })?;
    rows.collect()
}

/// A line ready to write.
struct CleanLine {
    id: String,
    item_id: Option<String>,
    description: String,
    qty: Option<Qty>,
    unit_price: Option<Cents>,
    taxable: bool,
}

/// Applies the source's rules to one line of the form. A changed item fills unit price, description and taxable from
/// the item unless the form also changed them (fields 7, 5, 11; kontroller 471 s111-112 sets the description last, "so
/// that it overrides the lookup"). No item is a description-only line: no quantity, no price (field 6).
fn clean_line(
    conn: &Connection,
    number: usize,
    input: &LineInput,
    stored: &StoredLine,
    errors: &mut Vec<FieldError>,
) -> rusqlite::Result<CleanLine> {
    let mut problem = |message: String| errors.push(field("lines", format!("Line {number}: {message}")));
    let chars = input.description.chars().count();
    if chars > DESCRIPTION_MAX {
        problem(format!("The description is {chars} characters; the most is {DESCRIPTION_MAX}."));
    }
    let Some(item_id) = opt(&input.item_id) else {
        return Ok(CleanLine {
            id: input.id.clone(),
            item_id: None,
            description: input.description.clone(),
            qty: None,
            unit_price: None,
            taxable: false,
        });
    };
    let mut description = input.description.clone();
    let mut price_text = input.unit_price.clone();
    let mut taxable = input.taxable;
    if stored.item_id.as_deref() != Some(item_id) {
        let item: Option<(String, Option<i64>, bool, bool)> = conn
            .query_row(
                "SELECT COALESCE(description, ''), unit_price, COALESCE(taxable, 0) <> 0, COALESCE(active, 0) <> 0
                 FROM item WHERE id = ?1",
                [item_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        match item {
            None => problem("Choose an item from the list.".into()),
            // BR-SALES-16: not offered on a new line. A line that has it already keeps it.
            Some((_, _, _, false)) => problem("This item is inactive.".into()),
            Some((item_description, item_price, item_taxable, true)) => {
                if input.description == stored.description {
                    description = item_description;
                }
                if input.unit_price == stored.unit_price {
                    price_text = item_price.map(|p| Cents(p).to_input_value()).unwrap_or_default();
                }
                taxable = item_taxable;
            }
        }
    }
    let qty = match Qty::parse_optional(&input.qty) {
        Ok(q) => q,
        Err(e) => {
            problem(format!("Quantity: {e}"));
            None
        }
    };
    let unit_price = match Cents::parse_optional(&price_text) {
        Ok(p) => p,
        Err(e) => {
            problem(format!("Rate: {e}"));
            None
        }
    };
    Ok(CleanLine { id: input.id.clone(), item_id: Some(item_id.to_owned()), description, qty, unit_price, taxable })
}

/// OP-SALES-04 `invoice_save`, OP-SALES-05 `sales_receipt_save`: writes the header and the lines of an unlocked
/// transaction, deletes the lines left out of the form (BR-SALES-08) and recomputes the totals (BR-SALES-02). It never
/// posts (OD-07-05). `kind` is the route's: an invoice id on the receipt route is not found. `add_line` appends an empty
/// line after the save (a portal row of layout 69).
pub fn transaction_save(
    conn: &mut Connection,
    kind: Kind,
    id: &str,
    input: &TxnInput,
    add_line: bool,
    actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = head(&tx, id)?;
    if current.kind != kind {
        return Err(CommandError::NotFound);
    }
    if current.locked {
        return Err(CommandError::Locked);
    }
    let mut errors = Vec::new();
    let doc = input.doc_number.trim();
    if doc.is_empty() {
        errors.push(field("doc_number", "Enter a document number."));
    } else if doc.chars().count() > DOC_NUMBER_MAX {
        errors.push(field("doc_number", format!("At most {DOC_NUMBER_MAX} characters.")));
    } else {
        let taken: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM sales_transaction WHERE doc_number = ?1 AND id <> ?2)",
            params![doc, id],
            |r| r.get(0),
        )?;
        if taken {
            errors.push(field("doc_number", format!("{doc} is used by another transaction.")));
        }
    }
    check_date(&mut errors, "date", &input.date);
    check_date(&mut errors, "due_date", &input.due_date);
    check_date(&mut errors, "ship_date", &input.ship_date);
    let bp = match parse_percent_bp(&input.tax_rate) {
        Ok(bp) => bp,
        Err(e) => {
            errors.push(field("tax_rate", e));
            0
        }
    };
    let ship_amt = match Cents::parse_optional(&input.ship_amt) {
        Ok(c) => c,
        Err(e) => {
            errors.push(field("ship_amt", e.to_string()));
            None
        }
    };
    let stored = stored_lines(&tx, id)?;
    let mut clean = Vec::with_capacity(input.lines.len());
    for (i, l) in input.lines.iter().enumerate() {
        match stored.iter().find(|(sid, _)| *sid == l.id) {
            None => errors.push(field("lines", format!("Line {} is not on this transaction.", i + 1))),
            Some((_, s)) => clean.push(clean_line(&tx, i + 1, l, s, &mut errors)?),
        }
    }
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }

    let date = opt(&input.date);
    let term = opt(&input.term);
    let due = match (opt(&input.due_date), kind, date, term) {
        (Some(d), ..) => Some(d.to_owned()),
        // Q2: an empty due date with a Net N term.
        (None, Kind::Invoice, Some(date), Some(term)) => due_from_term(date, term),
        _ => None,
    };
    tx.execute(
        "UPDATE sales_transaction SET doc_number = ?2, date = ?3, due_date = ?4, tax_rate_bp = ?5, ship_amt = ?6,
            ship_date = ?7, customer_memo = ?8, private_note = ?9, bill_email_address = ?10,
            bill_address_line_1 = ?11, bill_address_line_2 = ?12, bill_address_city = ?13, bill_address_state = ?14,
            bill_address_postal_code = ?15, bill_address_country = ?16,
            ship_address_line_1 = ?17, ship_address_line_2 = ?18, ship_address_city = ?19, ship_address_state = ?20,
            ship_address_postal_code = ?21, ship_address_country = ?22
         WHERE id = ?1",
        params![
            id,
            doc,
            date,
            due,
            bp,
            ship_amt.map(|c| c.0),
            opt(&input.ship_date),
            opt(&input.customer_memo),
            opt(&input.private_note),
            opt(&input.bill_email_address),
            input.bill.line_1,
            input.bill.line_2,
            input.bill.city,
            input.bill.state,
            input.bill.postal_code,
            input.bill.country,
            input.ship.line_1,
            input.ship.line_2,
            input.ship.city,
            input.ship.state,
            input.ship.postal_code,
            input.ship.country,
        ],
    )?;
    match kind {
        Kind::Invoice => {
            tx.execute(
                "UPDATE invoice SET id_term = ?2, modified_by = ?3 WHERE id = ?1",
                params![id, term, actor.account_name],
            )?;
            audit::stamp_modified(&tx, "invoice", &id.to_owned(), &Stamp::now(actor))?;
        }
        Kind::Receipt => {
            tx.execute(
                "UPDATE sales_receipt SET id_account_deposit = ?2, payment_ref_num = ?3 WHERE id = ?1",
                params![id, opt(&input.id_account_deposit), opt(&input.payment_ref_num)],
            )?;
        }
    }
    for (sid, _) in &stored {
        if !input.lines.iter().any(|l| l.id == *sid) {
            tx.execute("DELETE FROM sales_line WHERE id = ?1", [sid])?;
        }
    }
    for l in &clean {
        tx.execute(
            "UPDATE sales_line SET item_id = ?2, description = ?3, qty = ?4, unit_price = ?5, taxable = ?6 WHERE id = ?1",
            params![
                l.id,
                l.item_id,
                l.description,
                l.qty.map(|q| q.to_input_value()),
                l.unit_price.map(|c| c.0),
                i64::from(l.taxable)
            ],
        )?;
    }
    if add_line {
        tx.execute(
            "INSERT INTO sales_line (id, sales_transaction_id, taxable) VALUES (?1, ?2, 0)",
            params![new_id(), id],
        )?;
    }
    recompute_totals(&tx, id)?;
    if let Some(customer) = &current.customer {
        recompute_customer(&tx, customer)?;
    }
    tx.commit()?;
    Ok(())
}

/// Recomputes the lines' amounts and the transaction's stored totals, period, paid_ind and address displays (BR-SALES-01,
/// 02, 03, 07; fields 57, 81, 82). A sales receipt is paid when sold: balance 0 (X12).
pub fn recompute_totals(conn: &Connection, id: &str) -> Result<(), CommandError> {
    let overflow = || CommandError::Validation(vec![field("lines", "An amount is too large.")]);
    struct Row {
        id: String,
        line: Line,
    }
    let rows: Vec<Row> = {
        let mut stmt = conn.prepare(
            "SELECT sl.id, CAST(sl.qty AS TEXT), COALESCE(sl.unit_price, 0), COALESCE(sl.taxable, 0) <> 0, COALESCE(it.name, '')
             FROM sales_line sl LEFT JOIN item it ON it.id = sl.item_id WHERE sl.sales_transaction_id = ?1 ORDER BY sl.rowid",
        )?;
        let rows = stmt.query_map([id], |r| {
            let qty: Option<String> = r.get(1)?;
            let qty = qty
                .map(|q| Qty::parse(&q))
                .transpose()
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?;
            let name: String = r.get(4)?;
            Ok(Row {
                id: r.get(0)?,
                line: Line {
                    qty,
                    unit_price: Cents(r.get(2)?),
                    taxable: r.get(3)?,
                    retainer: name == RETAINER_ITEM_NAME,
                },
            })
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for row in &rows {
        let amount = row.line.amount().map_err(|_| overflow())?;
        conn.execute(
            "UPDATE sales_line SET amount = ?2, amount_taxable = ?3 WHERE id = ?1",
            params![row.id, amount.0, if row.line.taxable { amount.0 } else { 0 }],
        )?;
    }
    type Header = (bool, i64, i64, Option<String>, [Option<String>; 6], [Option<String>; 6]);
    let (invoice, bp, ship, date, bill, ship_to): Header = conn.query_row(
        "SELECT COALESCE(invoice_ind, 0) = 1, COALESCE(tax_rate_bp, 0), COALESCE(ship_amt, 0), date,
                bill_address_line_1, bill_address_line_2, bill_address_city, bill_address_state, bill_address_postal_code,
                bill_address_country,
                ship_address_line_1, ship_address_line_2, ship_address_city, ship_address_state, ship_address_postal_code,
                ship_address_country
         FROM sales_transaction WHERE id = ?1",
        [id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                [r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?],
                [r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?],
            ))
        },
    )?;
    let paid: i64 =
        conn.query_row("SELECT COALESCE(SUM(amount), 0) FROM payment_line WHERE id_invoice = ?1", [id], |r| r.get(0))?;
    let lines: Vec<Line> = rows.iter().map(|r| r.line).collect();
    let mut t = sales::totals(&lines, bp, Cents(ship), Cents(paid)).map_err(|_| overflow())?;
    if !invoice {
        t.balance = Cents::ZERO;
    }
    let period: Option<i64> = date.as_deref().and_then(|d| d.get(..7)).and_then(|ym| ym.replace('-', "").parse().ok());
    conn.execute(
        "UPDATE sales_transaction SET subtotal = ?2, taxable_subtotal = ?3, total_tax = ?4, total_amt = ?5, balance = ?6,
            retainer = ?7, period = ?8, paid_ind = ?9, billing_address_display = ?10, shipping_address_display = ?11
         WHERE id = ?1",
        params![
            id,
            t.subtotal.0,
            t.taxable_subtotal.0,
            t.total_tax.0,
            t.total_amt.0,
            t.balance.0,
            t.retainer.0,
            period,
            i64::from(t.balance.0 == 0),
            crate::import::recompute::display(&bill),
            crate::import::recompute::display(&ship_to),
        ],
    )?;
    Ok(())
}

/// The posted transactions of the customer row `t` (SQL).
const POSTED: &str = "st.id_customer = t.id AND st.locked_ind = 1 AND COALESCE(st.deleted, 0) = 0";

/// `customer.retainer_balance_cached` over the row `t` (BR-SALES-06, OD-07-07): the import and the commands use it.
pub fn retainer_balance_sql() -> String {
    format!(
        "COALESCE(t.retainer_adjustment, 0) + COALESCE((SELECT SUM(st.retainer) FROM sales_transaction st WHERE {POSTED}), 0)"
    )
}

/// `customer.balance` over the row `t`: the posted invoices' balances (D-39).
pub fn customer_balance_sql() -> String {
    format!("COALESCE((SELECT SUM(st.balance) FROM sales_transaction st WHERE {POSTED} AND st.invoice_ind = 1), 0)")
}

/// Recomputes one customer's two caches. Recounted, never added to (576 s86 adds, so posting twice counted twice: D-07).
pub fn recompute_customer(conn: &Connection, customer_role_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        &format!(
            "UPDATE customer AS t SET retainer_balance_cached = {}, balance = {} WHERE t.id = ?1",
            retainer_balance_sql(),
            customer_balance_sql()
        ),
        [customer_role_id],
    )?;
    Ok(())
}

/// OP-SALES-06 `sales_transaction_lock`: posts or unposts (BR-SALES-06, BR-SALES-15). The state changes only when it
/// differs (576 s93-97); the customer's caches are recomputed either way. Returns the document number.
pub fn transaction_lock(conn: &mut Connection, id: &str, lock: bool, actor: &Actor) -> Result<String, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = head(&tx, id)?;
    if current.locked != lock {
        tx.execute("UPDATE sales_transaction SET locked_ind = ?2 WHERE id = ?1", params![id, i64::from(lock)])?;
        if current.kind == Kind::Invoice {
            audit::stamp_modified(&tx, "invoice", &id.to_owned(), &Stamp::now(actor))?;
        }
    }
    if let Some(customer) = &current.customer {
        recompute_customer(&tx, customer)?;
    }
    let doc: String =
        tx.query_row("SELECT COALESCE(doc_number, '') FROM sales_transaction WHERE id = ?1", [id], |r| r.get(0))?;
    tx.commit()?;
    Ok(doc)
}

/// OP-SALES-07 `sales_transaction_delete` (AP-05, AP-07, BR-SALES-11): removes the lines, the invoice or receipt row and
/// the file links (rels 59, 76, 77, 82); the files stay. A transaction with payment lines is refused: the source leaves
/// them orphaned (rel 78 has no cascade), foreign keys forbid that (OD-04-01) and only `payment_save` writes them (AP-06).
pub fn transaction_delete(conn: &mut Connection, id: &str, confirm: bool) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = head(&tx, id)?;
    if current.locked {
        return Err(CommandError::Locked);
    }
    let count = |sql: &str| tx.query_row(sql, [id], |r| r.get::<_, i64>(0));
    if count("SELECT COUNT(*) FROM payment_line WHERE id_invoice = ?1")? > 0 {
        return Err(CommandError::Refused(
            "This invoice has payments. Remove them from the payment first, then delete it.".into(),
        ));
    }
    let lines = count("SELECT COUNT(*) FROM sales_line WHERE sales_transaction_id = ?1")?;
    let files = count("SELECT COUNT(*) FROM file_attachment WHERE entity_type_id = 12 AND entity_id = ?1")?;
    if !confirm {
        let doc: String =
            tx.query_row("SELECT COALESCE(doc_number, '') FROM sales_transaction WHERE id = ?1", [id], |r| r.get(0))?;
        let plural = |n: i64| if n == 1 { "" } else { "s" };
        return Err(CommandError::ConfirmationRequired(format!(
            "Deletes {} {doc}, {lines} line{} and {files} attachment link{}. This cannot be undone. The files stay.",
            current.kind.label().to_lowercase(),
            plural(lines),
            plural(files),
        )));
    }
    tx.execute("DELETE FROM sales_line WHERE sales_transaction_id = ?1", [id])?;
    tx.execute("DELETE FROM invoice WHERE id = ?1", [id])?;
    tx.execute("DELETE FROM sales_receipt WHERE id = ?1", [id])?;
    tx.execute("DELETE FROM file_attachment WHERE entity_type_id = 12 AND entity_id = ?1", [id])?;
    tx.execute("DELETE FROM sales_transaction WHERE id = ?1", [id])?;
    if let Some(customer) = &current.customer {
        recompute_customer(&tx, customer)?;
    }
    tx.commit()?;
    Ok(())
}

/// OP-SALES-08 `sales_lines_remove` (BR-SALES-11, AP-05, AP-07): every line must belong to this unlocked transaction
/// (587 s38-39, s44-45, s51-52). The lines are deleted (587 only unlinks time entries: D-07).
pub fn lines_remove(
    conn: &mut Connection,
    id: &str,
    line_ids: &[String],
    confirm: bool,
    actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = head(&tx, id)?;
    if current.locked {
        return Err(CommandError::Locked);
    }
    if line_ids.is_empty() {
        return Err(CommandError::Validation(vec![field("lines", "Choose a line to remove.")]));
    }
    for line in line_ids {
        let owner: Option<String> = tx
            .query_row("SELECT sales_transaction_id FROM sales_line WHERE id = ?1", [line], |r| r.get(0))
            .optional()?;
        if owner.as_deref() != Some(id) {
            return Err(CommandError::Validation(vec![field("lines", "Every line must belong to this transaction.")]));
        }
    }
    if !confirm {
        let n = line_ids.len();
        return Err(CommandError::ConfirmationRequired(format!(
            "Removes {n} line{} and recomputes the totals.",
            if n == 1 { "" } else { "s" }
        )));
    }
    for line in line_ids {
        tx.execute("DELETE FROM sales_line WHERE id = ?1", [line])?;
    }
    recompute_totals(&tx, id)?;
    if current.kind == Kind::Invoice {
        audit::stamp_modified(&tx, "invoice", &id.to_owned(), &Stamp::now(actor))?;
    }
    tx.commit()?;
    Ok(())
}

/// A transaction for the editor, by id.
pub fn transaction_get(conn: &Connection, id: &str) -> Result<TxnEdit, CommandError> {
    let current = head(conn, id)?;
    let mut e = conn.query_row(
        "SELECT COALESCE(st.doc_number, ''), COALESCE(st.date, ''), COALESCE(st.due_date, ''), COALESCE(st.tax_rate_bp, 0),
                st.ship_amt, COALESCE(st.ship_date, ''), COALESCE(st.customer_memo, ''), COALESCE(st.private_note, ''),
                COALESCE(st.bill_email_address, ''),
                COALESCE(st.bill_address_line_1, ''), COALESCE(st.bill_address_line_2, ''), COALESCE(st.bill_address_city, ''),
                COALESCE(st.bill_address_state, ''), COALESCE(st.bill_address_postal_code, ''), COALESCE(st.bill_address_country, ''),
                COALESCE(st.ship_address_line_1, ''), COALESCE(st.ship_address_line_2, ''), COALESCE(st.ship_address_city, ''),
                COALESCE(st.ship_address_state, ''), COALESCE(st.ship_address_postal_code, ''), COALESCE(st.ship_address_country, ''),
                COALESCE(st.id_customer, ''), COALESCE(p.name_cached, ''), COALESCE(p.id, ''),
                COALESCE(i.id_term, ''), COALESCE(sr.id_account_deposit, ''), COALESCE(sr.payment_ref_num, ''),
                COALESCE(st.subtotal, 0), COALESCE(st.taxable_subtotal, 0), COALESCE(st.total_tax, 0), COALESCE(st.total_amt, 0),
                COALESCE(st.balance, 0), COALESCE(st.retainer, 0)
         FROM sales_transaction st
         LEFT JOIN party_role r ON r.id = st.id_customer LEFT JOIN party p ON p.id = r.party_id
         LEFT JOIN invoice i ON i.id = st.id LEFT JOIN sales_receipt sr ON sr.id = st.id
         WHERE st.id = ?1",
        [id],
        |r| {
            let bp: i64 = r.get(3)?;
            let ship: Option<i64> = r.get(4)?;
            Ok(TxnEdit {
                id: id.to_owned(),
                kind: current.kind,
                locked: current.locked,
                customer_role_id: r.get(21)?,
                customer: r.get(22)?,
                party_id: r.get(23)?,
                tax_rate_bp: bp,
                amounts: Vec::new(),
                paid: Cents::ZERO,
                totals: Totals {
                    subtotal: Cents(r.get(27)?),
                    taxable_subtotal: Cents(r.get(28)?),
                    total_tax: Cents(r.get(29)?),
                    total_amt: Cents(r.get(30)?),
                    balance: Cents(r.get(31)?),
                    retainer: Cents(r.get(32)?),
                },
                input: TxnInput {
                    doc_number: r.get(0)?,
                    date: r.get(1)?,
                    due_date: r.get(2)?,
                    term: r.get(24)?,
                    tax_rate: crate::admin::format_percent(bp),
                    ship_amt: ship.map(|c| Cents(c).to_input_value()).unwrap_or_default(),
                    ship_date: r.get(5)?,
                    customer_memo: r.get(6)?,
                    private_note: r.get(7)?,
                    bill_email_address: r.get(8)?,
                    bill: Address {
                        line_1: r.get(9)?,
                        line_2: r.get(10)?,
                        city: r.get(11)?,
                        state: r.get(12)?,
                        postal_code: r.get(13)?,
                        country: r.get(14)?,
                    },
                    ship: Address {
                        line_1: r.get(15)?,
                        line_2: r.get(16)?,
                        city: r.get(17)?,
                        state: r.get(18)?,
                        postal_code: r.get(19)?,
                        country: r.get(20)?,
                    },
                    id_account_deposit: r.get(25)?,
                    payment_ref_num: r.get(26)?,
                    lines: Vec::new(),
                },
            })
        },
    )?;
    let mut stmt = conn.prepare(
        "SELECT sl.id, COALESCE(sl.item_id, ''), COALESCE(sl.description, ''), CAST(sl.qty AS TEXT), sl.unit_price,
                COALESCE(sl.taxable, 0) <> 0, COALESCE(sl.amount, 0)
         FROM sales_line sl WHERE sl.sales_transaction_id = ?1 ORDER BY sl.rowid",
    )?;
    let rows = stmt.query_map([id], |r| {
        let qty: Option<String> = r.get(3)?;
        let price: Option<i64> = r.get(4)?;
        Ok((
            LineInput {
                id: r.get(0)?,
                item_id: r.get(1)?,
                description: r.get(2)?,
                qty: qty.and_then(|q| Qty::parse(&q).ok()).map(|q| q.to_input_value()).unwrap_or_default(),
                unit_price: price.map(|p| Cents(p).to_input_value()).unwrap_or_default(),
                taxable: r.get(5)?,
            },
            Cents(r.get(6)?),
        ))
    })?;
    for row in rows {
        let (line, amount) = row?;
        e.input.lines.push(line);
        e.amounts.push(amount);
    }
    e.paid = Cents(conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM payment_line WHERE id_invoice = ?1",
        [id],
        |r| r.get(0),
    )?);
    Ok(e)
}

/// The id of a transaction by its document number.
pub fn id_of_doc(conn: &Connection, doc_number: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM sales_transaction WHERE doc_number = ?1 AND COALESCE(deleted, 0) = 0",
        [doc_number],
        |r| r.get(0),
    )
    .optional()
}

/// The terms a form offers (Q2): those in use, plus the usual ones.
pub fn term_choices(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id_term FROM invoice WHERE COALESCE(id_term, '') <> ''
         UNION SELECT sales_term_ref FROM customer WHERE COALESCE(sales_term_ref, '') <> ''
         UNION SELECT column1 FROM (VALUES ('Net 15'), ('Net 30'), ('Net 60'), ('Due on receipt'))",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// (customer role id, name) of the current customers that match, for the customer picker.
pub fn customer_search(conn: &Connection, q: &str, limit: i64) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, p.name_cached FROM customer c JOIN party_role r ON r.id = c.id JOIN party p ON p.id = r.party_id
         WHERE COALESCE(p.deleted_ind, 0) = 0 AND (?1 = '' OR instr(lower(p.name_cached), lower(?1)) > 0)
         ORDER BY p.name_cached COLLATE NOCASE LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![q.trim(), limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_dates_from_terms() {
        assert_eq!(due_from_term("2026-10-14", "Net 15").as_deref(), Some("2026-10-29"));
        assert_eq!(due_from_term("2026-12-20", "Net 30").as_deref(), Some("2027-01-19"));
        assert_eq!(due_from_term("2026-10-14", "Due on receipt"), None);
        assert_eq!(due_from_term("", "Net 15"), None);
    }

    #[test]
    fn document_numbers_follow_the_highest() {
        let conn = crate::db::open_demo(None).unwrap();
        assert_eq!(next_doc_number(&conn, Kind::Invoice).unwrap(), "INV-1052");
        assert_eq!(next_doc_number(&conn, Kind::Receipt).unwrap(), "SR-0312");
    }
}
