//! OP-SALES-09 `payment_save` (BR-SALES-09, BR-SALES-10): the only command that writes `payment` and `payment_line`
//! (AP-06; it replaces the source's `$$allow_commit` commit guard, kontacts 278). Plain SQL, one `BEGIN IMMEDIATE`
//! transaction. Paying a posted invoice is allowed: only its balance and paid_ind change, not its lines or header.

use crate::admin::{CommandError, FieldError};
use crate::ids::new_id;
use crate::sales::{recompute_customer, recompute_totals};
use karbon_money::Cents;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

/// The source's refusal (kontacts 283 s3).
pub const OVER_ALLOCATED: &str = "The total allocated cannot exceed the Payment amount.";
/// The source's refusal (kontacts 221 s4).
pub const NO_CUSTOMER: &str = "You can't add a Payment to an invoice that doesn't have a customer.";

/// Value list 13's custom values. The fuller list came from QBO (D-02).
pub const METHODS: [&str; 5] = ["American Express", "MasterCard", "Visa", "Cash", "Personal Check"];
/// Value list 14's custom values. Layout 85 binds list 18, which has only QBO's accounts (D-02).
pub const DEPOSIT_ACCOUNTS: [&str; 2] = ["Checking", "Savings"];

/// The card as typed (layout 85). Amounts stay text until the save parses them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaymentInput {
    pub customer_id: String,
    pub date: String,
    pub amount: String,
    pub method: String,
    pub reference: String,
    pub deposit: String,
    /// (invoice id, amount to be credited). An empty or zero amount is no line.
    pub lines: Vec<(String, String)>,
}

fn field(name: &'static str, message: impl Into<String>) -> FieldError {
    FieldError { field: name, message: message.into() }
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// OP-SALES-09: writes the payment, replaces its lines (BR-SALES-10; 404 s122-137 starts its delete loop at 0: D-07), and
/// recomputes the balance of every invoice touched, old and new, and the customers' balances (BR-SALES-02; field 58).
/// Returns the payment id. `id` none is a new payment.
pub fn payment_save(conn: &mut Connection, id: Option<&str>, input: &PaymentInput) -> Result<String, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(id) = id {
        let exists: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM payment WHERE id = ?1)", [id], |r| r.get(0))?;
        if !exists {
            return Err(CommandError::NotFound);
        }
    }
    let mut errors = Vec::new();
    let customer = input.customer_id.trim();
    let known: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM customer WHERE id = ?1)", [customer], |r| r.get(0))?;
    if !known {
        errors.push(field("customer_id", NO_CUSTOMER));
    }
    if let Some(d) = opt(&input.date) {
        if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
            errors.push(field("date", "Enter a date as YYYY-MM-DD."));
        }
    }
    let amount = match Cents::parse(&input.amount) {
        Ok(c) => c,
        Err(e) => {
            errors.push(field("amount", e.to_string()));
            Cents::ZERO
        }
    };
    let mut lines: Vec<(String, Cents)> = Vec::new();
    for (invoice, text) in &input.lines {
        let credited = match Cents::parse_optional(text) {
            Ok(c) => c.unwrap_or(Cents::ZERO),
            Err(e) => {
                errors.push(field("lines", e.to_string()));
                continue;
            }
        };
        if credited == Cents::ZERO {
            continue;
        }
        // Only an invoice of this customer can be paid (kontacts rel 85, 280 s11).
        let ok: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM sales_transaction WHERE id = ?1 AND id_customer = ?2 AND invoice_ind = 1
                            AND COALESCE(deleted, 0) = 0)",
            params![invoice, customer],
            |r| r.get(0),
        )?;
        if !ok {
            errors.push(field("lines", "Only this customer's invoices can be paid."));
            continue;
        }
        lines.push((invoice.clone(), credited));
    }
    let allocated = lines.iter().try_fold(Cents::ZERO, |acc, (_, c)| acc.checked_add(*c));
    if allocated.is_none_or(|total| amount < total) && errors.iter().all(|e| e.field != "amount") {
        errors.push(field("lines", OVER_ALLOCATED));
    }
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }

    let payment_id = id.map(str::to_owned).unwrap_or_else(new_id);
    let (mut invoices, mut customers): (Vec<String>, Vec<String>) = (Vec::new(), vec![customer.to_owned()]);
    if id.is_some() {
        let mut stmt = tx.prepare("SELECT id_invoice, id_customer FROM payment_line WHERE id_payment = ?1")?;
        let rows =
            stmt.query_map([&payment_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)))?;
        for row in rows {
            let (invoice, who) = row?;
            invoices.extend(invoice);
            customers.extend(who);
        }
        drop(stmt);
        let old: Option<String> = tx
            .query_row("SELECT id_customer FROM payment WHERE id = ?1", [&payment_id], |r| r.get(0))
            .optional()?
            .flatten();
        customers.extend(old);
    }
    tx.execute(
        "INSERT INTO payment (id, id_customer, date, amount, id_deposit_account, payment_ref_num, id_payment_method)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (id) DO UPDATE SET id_customer = ?2, date = ?3, amount = ?4, id_deposit_account = ?5,
            payment_ref_num = ?6, id_payment_method = ?7",
        params![
            payment_id,
            customer,
            opt(&input.date),
            amount.0,
            opt(&input.deposit),
            opt(&input.reference),
            opt(&input.method)
        ],
    )?;
    tx.execute("DELETE FROM payment_line WHERE id_payment = ?1", [&payment_id])?;
    for (invoice, credited) in &lines {
        tx.execute(
            "INSERT INTO payment_line (id, id_payment, id_invoice, id_customer, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_id(), payment_id, invoice, customer, credited.0],
        )?;
        invoices.push(invoice.clone());
    }
    invoices.sort();
    invoices.dedup();
    customers.sort();
    customers.dedup();
    for invoice in &invoices {
        recompute_totals(&tx, invoice)?;
    }
    for who in &customers {
        recompute_customer(&tx, who)?;
    }
    tx.commit()?;
    Ok(payment_id)
}

/// One invoice row of the card (layout 85: Invoice, Date, Original, Balance Due).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenInvoice {
    pub id: String,
    pub doc_number: String,
    pub date: String,
    pub total: i64,
    pub balance: i64,
}

/// The customer's unpaid invoices, oldest first (kontacts rel 85: paid_ind is not 1, posted or not).
pub fn open_invoices(conn: &Connection, customer_id: &str) -> rusqlite::Result<Vec<OpenInvoice>> {
    let mut stmt = conn.prepare(&format!(
        "{INVOICE_ROW} WHERE id_customer = ?1 AND COALESCE(paid_ind, 0) <> 1 AND invoice_ind = 1
         AND COALESCE(deleted, 0) = 0 ORDER BY date, doc_number"
    ))?;
    let rows = stmt.query_map([customer_id], read_invoice)?;
    rows.collect()
}

const INVOICE_ROW: &str =
    "SELECT id, COALESCE(doc_number, ''), COALESCE(date, ''), COALESCE(total_amt, 0), COALESCE(balance, 0)
    FROM sales_transaction";

fn read_invoice(r: &rusqlite::Row<'_>) -> rusqlite::Result<OpenInvoice> {
    Ok(OpenInvoice { id: r.get(0)?, doc_number: r.get(1)?, date: r.get(2)?, total: r.get(3)?, balance: r.get(4)? })
}

/// One invoice by id.
pub fn invoice_row(conn: &Connection, id: &str) -> rusqlite::Result<Option<OpenInvoice>> {
    conn.query_row(&format!("{INVOICE_ROW} WHERE id = ?1"), [id], read_invoice).optional()
}

/// A stored payment as the card shows it.
pub fn payment_get(conn: &Connection, id: &str) -> Result<PaymentInput, CommandError> {
    let mut p = conn
        .query_row(
            "SELECT COALESCE(id_customer, ''), COALESCE(date, ''), COALESCE(amount, 0), COALESCE(id_payment_method, ''),
                    COALESCE(payment_ref_num, ''), COALESCE(id_deposit_account, '')
             FROM payment WHERE id = ?1",
            [id],
            |r| {
                Ok(PaymentInput {
                    customer_id: r.get(0)?,
                    date: r.get(1)?,
                    amount: Cents(r.get(2)?).to_input_value(),
                    method: r.get(3)?,
                    reference: r.get(4)?,
                    deposit: r.get(5)?,
                    lines: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let mut stmt = conn.prepare(
        "SELECT COALESCE(id_invoice, ''), COALESCE(amount, 0) FROM payment_line WHERE id_payment = ?1 ORDER BY rowid",
    )?;
    let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, Cents(r.get(1)?).to_input_value())))?;
    p.lines = rows.collect::<rusqlite::Result<_>>()?;
    Ok(p)
}

/// One payment on an invoice's page (layouts 69, 143: Date, Amount, a link to the payment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoicePayment {
    pub payment_id: String,
    pub date: String,
    pub reference: String,
    pub amount: i64,
}

/// The payments applied to an invoice, by date.
pub fn payments_of_invoice(conn: &Connection, invoice_id: &str) -> rusqlite::Result<Vec<InvoicePayment>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, COALESCE(p.date, ''), COALESCE(p.payment_ref_num, p.id_payment_method, ''), COALESCE(pl.amount, 0)
         FROM payment_line pl JOIN payment p ON p.id = pl.id_payment WHERE pl.id_invoice = ?1 ORDER BY p.date, p.id",
    )?;
    let rows = stmt.query_map([invoice_id], |r| {
        Ok(InvoicePayment { payment_id: r.get(0)?, date: r.get(1)?, reference: r.get(2)?, amount: r.get(3)? })
    })?;
    rows.collect()
}

/// The customer's name, for the card's title.
pub fn customer_name(conn: &Connection, customer_id: &str) -> rusqlite::Result<String> {
    Ok(conn
        .query_row(
            "SELECT p.name_cached FROM party_role r JOIN party p ON p.id = r.party_id WHERE r.id = ?1",
            [customer_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default())
}

/// The customer of a transaction, when it has one.
pub fn customer_of(conn: &Connection, transaction_id: &str) -> rusqlite::Result<Option<Option<String>>> {
    conn.query_row("SELECT id_customer FROM sales_transaction WHERE id = ?1", [transaction_id], |r| r.get(0)).optional()
}
