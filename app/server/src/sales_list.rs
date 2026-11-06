//! Read-only sales and payments lists (UI-13) and a customer's sales transactions (the Sales tab of UI-04). The
//! writes (OP-SALES-04 invoice_save, OP-SALES-09 payment_save) are I5.

use crate::admin::CommandError;
use rusqlite::{params_from_iter, Connection};

/// Rows per page (05: list pages 50-100).
pub const PAGE_SIZE: i64 = 50;

/// The status a list shows, derived from the stored totals (BR-SALES-06: only posted, locked, transactions count):
/// Draft (not locked), Paid (no balance), Overdue (an open invoice past its due date, by the local date), Partially
/// paid, Open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Any,
    Draft,
    Open,
    PartiallyPaid,
    Overdue,
    Paid,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s {
            "draft" => Status::Draft,
            "open" => Status::Open,
            "partial" => Status::PartiallyPaid,
            "overdue" => Status::Overdue,
            "paid" => Status::Paid,
            _ => Status::Any,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Status::Any => "all",
            Status::Draft => "draft",
            Status::Open => "open",
            Status::PartiallyPaid => "partial",
            Status::Overdue => "overdue",
            Status::Paid => "paid",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Any => "All",
            Status::Draft => "Draft",
            Status::Open => "Open",
            Status::PartiallyPaid => "Partially paid",
            Status::Overdue => "Overdue",
            Status::Paid => "Paid",
        }
    }
    /// The chip class of the design system.
    pub fn chip(self) -> &'static str {
        match self {
            Status::Paid => "ok",
            Status::PartiallyPaid => "warn",
            Status::Overdue => "bad",
            Status::Open => "info",
            Status::Draft | Status::Any => "",
        }
    }
    fn from_key(s: &str) -> Self {
        Self::parse(s)
    }
}

/// The SQL expression of [`Status`] over a `sales_transaction st`, as its key. `?1` is today (`YYYY-MM-DD`).
pub const STATUS_SQL: &str = "CASE
  WHEN COALESCE(st.locked_ind, 0) = 0 THEN 'draft'
  WHEN COALESCE(st.balance, 0) = 0 THEN 'paid'
  WHEN COALESCE(st.invoice_ind, 0) = 1 AND st.due_date < ?1 THEN 'overdue'
  WHEN COALESCE(st.balance, 0) < COALESCE(st.total_amt, 0) THEN 'partial'
  ELSE 'open' END";

/// Invoices, sales receipts or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocKind {
    #[default]
    All,
    Invoice,
    Receipt,
}

impl DocKind {
    pub fn parse(s: &str) -> Self {
        match s {
            "invoice" => DocKind::Invoice,
            "receipt" => DocKind::Receipt,
            _ => DocKind::All,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            DocKind::All => "all",
            DocKind::Invoice => "invoice",
            DocKind::Receipt => "receipt",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SalesQuery {
    /// Document number or customer name contains this (case-insensitive).
    pub query: String,
    pub status: Status,
    pub kind: DocKind,
    /// Only this customer role's transactions (the Sales tab of a party).
    pub customer_role_id: Option<String>,
    pub page: i64,
    /// 0 means [`PAGE_SIZE`].
    pub page_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalesRow {
    pub doc_number: String,
    pub date: String,
    pub due_date: String,
    pub invoice: bool,
    pub total: i64,
    pub balance: i64,
    pub status: Status,
    pub customer: String,
    pub party_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub rows: Vec<T>,
    pub matched: i64,
    pub page: i64,
    pub pages: i64,
    pub page_size: i64,
    /// Sum of `total` and `balance` over every matched row (posted rows only for the balance).
    pub total: i64,
    pub balance: i64,
}

impl<T> Page<T> {
    /// 1-based numbers of the first and last row on this page (0, 0 when empty).
    pub fn span(&self) -> (i64, i64) {
        if self.rows.is_empty() {
            return (0, 0);
        }
        let first = (self.page - 1) * self.page_size + 1;
        (first, first + self.rows.len() as i64 - 1)
    }
}

pub(crate) fn paging(matched: i64, page: i64, page_size: i64) -> (i64, i64, i64) {
    let size = if page_size <= 0 { PAGE_SIZE } else { page_size };
    let pages = ((matched + size - 1) / size).max(1);
    (page.clamp(1, pages), pages, size)
}

/// The sales transactions that match, newest first. Deleted rows are hidden.
pub fn sales_list(conn: &Connection, q: &SalesQuery, today: &str) -> Result<Page<SalesRow>, CommandError> {
    // `?1 IS NOT NULL` uses today in the count query too, which may not read the status.
    let mut clauses = vec!["?1 IS NOT NULL".to_owned(), "COALESCE(st.deleted, 0) = 0".to_owned()];
    let mut values: Vec<String> = vec![today.to_owned()];
    match q.kind {
        DocKind::All => {}
        DocKind::Invoice => clauses.push("COALESCE(st.invoice_ind, 0) = 1".into()),
        DocKind::Receipt => clauses.push("COALESCE(st.sales_receipt_ind, 0) = 1".into()),
    }
    if q.status != Status::Any {
        values.push(q.status.key().to_owned());
        clauses.push(format!("({STATUS_SQL}) = ?{}", values.len()));
    }
    if let Some(role) = &q.customer_role_id {
        values.push(role.clone());
        clauses.push(format!("st.id_customer = ?{}", values.len()));
    }
    let text = q.query.trim().to_lowercase();
    if !text.is_empty() {
        values.push(text);
        let n = values.len();
        clauses.push(format!(
            "(instr(lower(COALESCE(st.doc_number, '')), ?{n}) > 0 OR instr(lower(COALESCE(p.name_cached, '')), ?{n}) > 0)"
        ));
    }
    let from = "FROM sales_transaction st LEFT JOIN party_role r ON r.id = st.id_customer \
                LEFT JOIN party p ON p.id = r.party_id";
    let filter = format!("WHERE {}", clauses.join(" AND "));
    let (matched, total, balance): (i64, i64, i64) = conn.query_row(
        &format!(
            "SELECT COUNT(*), COALESCE(SUM(st.total_amt), 0),
                    COALESCE(SUM(CASE WHEN COALESCE(st.locked_ind, 0) = 1 THEN st.balance END), 0) {from} {filter}"
        ),
        params_from_iter(values.iter()),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let (page, pages, size) = paging(matched, q.page, q.page_size);
    let sql = format!(
        "SELECT COALESCE(st.doc_number, ''), COALESCE(st.date, ''), COALESCE(st.due_date, ''),
                COALESCE(st.invoice_ind, 0) = 1, COALESCE(st.total_amt, 0), COALESCE(st.balance, 0), {STATUS_SQL},
                COALESCE(p.name_cached, ''), COALESCE(p.id, '')
         {from} {filter} ORDER BY st.date DESC, st.doc_number DESC LIMIT {size} OFFSET {}",
        (page - 1) * size
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), |r| {
            Ok(SalesRow {
                doc_number: r.get(0)?,
                date: r.get(1)?,
                due_date: r.get(2)?,
                invoice: r.get(3)?,
                total: r.get(4)?,
                balance: r.get(5)?,
                status: Status::from_key(&r.get::<_, String>(6)?),
                customer: r.get(7)?,
                party_id: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Page { rows, matched, page, pages, page_size: size, total, balance })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRow {
    pub id: String,
    pub date: String,
    pub reference: String,
    pub amount: i64,
    pub customer: String,
    pub party_id: String,
    /// Document numbers the payment is applied to (payment_line -> sales_transaction).
    pub applied_to: Vec<String>,
}

/// Payments, newest first, with the documents each one pays. `query` matches the customer name or the reference.
pub fn payment_list(
    conn: &Connection,
    query: &str,
    page: i64,
    page_size: i64,
) -> Result<Page<PaymentRow>, CommandError> {
    let text = query.trim().to_lowercase();
    let from =
        "FROM payment pay LEFT JOIN party_role r ON r.id = pay.id_customer LEFT JOIN party p ON p.id = r.party_id";
    let filter = "WHERE (?1 = '' OR instr(lower(COALESCE(p.name_cached, '')), ?1) > 0 \
                  OR instr(lower(COALESCE(pay.payment_ref_num, '')), ?1) > 0)";
    let (matched, total): (i64, i64) =
        conn.query_row(&format!("SELECT COUNT(*), COALESCE(SUM(pay.amount), 0) {from} {filter}"), [&text], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let (page, pages, size) = paging(matched, page, page_size);
    let sql = format!(
        "SELECT pay.id, COALESCE(pay.date, ''), COALESCE(pay.payment_ref_num, ''), COALESCE(pay.amount, 0),
                COALESCE(p.name_cached, ''), COALESCE(p.id, ''),
                COALESCE((SELECT group_concat(doc, ' ') FROM (SELECT st.doc_number AS doc FROM payment_line pl
                          JOIN sales_transaction st ON st.id = pl.id_invoice WHERE pl.id_payment = pay.id
                          ORDER BY st.doc_number)), '')
         {from} {filter} ORDER BY pay.date DESC, pay.id LIMIT {size} OFFSET {}",
        (page - 1) * size
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([&text], |r| {
            let applied: String = r.get(6)?;
            Ok(PaymentRow {
                id: r.get(0)?,
                date: r.get(1)?,
                reference: r.get(2)?,
                amount: r.get(3)?,
                customer: r.get(4)?,
                party_id: r.get(5)?,
                applied_to: applied.split_whitespace().map(str::to_owned).collect(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Page { rows, matched, page, pages, page_size: size, total, balance: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_in_memory, Seed};

    const TODAY: &str = "2026-10-28";

    fn demo() -> Connection {
        open_in_memory(Seed::Demo).unwrap().0
    }

    #[test]
    fn every_sales_transaction_newest_first() {
        let conn = demo();
        let page = sales_list(&conn, &SalesQuery::default(), TODAY).unwrap();
        assert_eq!(page.matched, 16);
        assert_eq!(page.rows[0].doc_number, "INV-1051");
        assert!(page.rows.windows(2).all(|w| w[0].date >= w[1].date));
        assert_eq!(page.balance, 2_961_905, "the posted open balance of the seed, as on /customers");
    }

    #[test]
    fn status_kind_customer_and_text_filters() {
        let conn = demo();
        let only = |q: SalesQuery| {
            sales_list(&conn, &q, TODAY).unwrap().rows.into_iter().map(|r| r.doc_number).collect::<Vec<_>>()
        };
        let hp = only(SalesQuery { customer_role_id: Some("r-hp".into()), ..Default::default() });
        assert_eq!(hp, ["INV-1048", "INV-1042", "SR-0311", "INV-1035", "RET-0042", "INV-1027"]);
        let receipts = only(SalesQuery { kind: DocKind::Receipt, ..Default::default() });
        assert_eq!(receipts.len(), 5);
        let paid = sales_list(&conn, &SalesQuery { status: Status::Paid, ..Default::default() }, TODAY).unwrap();
        assert!(paid.rows.iter().all(|r| r.balance == 0 && r.status == Status::Paid));
        assert_eq!(only(SalesQuery { query: "inv-1048".into(), ..Default::default() }), ["INV-1048"]);
        assert_eq!(only(SalesQuery { query: "bluefin".into(), ..Default::default() }), ["INV-1051"]);
    }

    #[test]
    fn overdue_and_drafts_and_deleted() {
        let conn = demo();
        conn.execute("UPDATE sales_transaction SET due_date = '2000-01-01' WHERE doc_number = 'INV-1051'", []).unwrap();
        conn.execute("UPDATE sales_transaction SET locked_ind = 0 WHERE doc_number = 'INV-1050'", []).unwrap();
        conn.execute("UPDATE sales_transaction SET deleted = 1 WHERE doc_number = 'INV-1049'", []).unwrap();
        let all = sales_list(&conn, &SalesQuery::default(), TODAY).unwrap();
        let status = |doc: &str| all.rows.iter().find(|r| r.doc_number == doc).map(|r| r.status);
        assert_eq!(status("INV-1051"), Some(Status::Overdue));
        assert_eq!(status("INV-1050"), Some(Status::Draft));
        assert_eq!(status("INV-1049"), None, "deleted rows are hidden");
        assert_eq!(status("INV-1027"), Some(Status::Paid));
        let drafts = sales_list(&conn, &SalesQuery { status: Status::Draft, ..Default::default() }, TODAY).unwrap();
        assert_eq!(drafts.matched, 1);
    }

    #[test]
    fn payments_with_what_they_pay() {
        let conn = demo();
        let page = payment_list(&conn, "", 1, 0).unwrap();
        assert_eq!(page.matched, 4);
        assert!(page.rows.iter().all(|p| !p.applied_to.is_empty()));
        let hp = payment_list(&conn, "halden", 1, 0).unwrap();
        assert!(hp.rows.iter().all(|p| p.customer == "Halden & Pryce Architects"));
        assert!(payment_list(&conn, "no such", 1, 0).unwrap().rows.is_empty());
    }
}
