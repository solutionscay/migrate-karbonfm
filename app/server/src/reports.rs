//! OP-SALES-11 `sales_summary_by_month` (BR-SALES-17): posted revenue per month, read from the view of the same name.
//! Retainer lines, tax and shipping are not revenue (migration V3). A month with no posted sale reads 0.

use crate::admin::{CommandError, FieldError};
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// A range longer than this is refused; the source has no such report, so this only keeps a typed range sane.
pub const MAX_MONTHS: i64 = 240;

/// A month, as the view's `period`: 202610.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Period {
    year: i64,
    month: i64,
}

impl Period {
    /// `YYYYMM`, a real month. Anything else is `None`.
    pub fn parse(text: &str) -> Option<Period> {
        let t = text.trim();
        if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n: i64 = t.parse().ok()?;
        let (year, month) = (n / 100, n % 100);
        ((1..=12).contains(&month) && year >= 1000).then_some(Period { year, month })
    }

    /// The month of an ISO date.
    pub fn of_date(iso: &str) -> Option<Period> {
        Period::parse(&format!("{}{}", iso.get(..4)?, iso.get(5..7)?))
    }

    pub fn key(self) -> i64 {
        self.year * 100 + self.month
    }

    /// This month plus `n` months (`n` may be negative).
    pub fn plus(self, n: i64) -> Period {
        let total = self.year * 12 + (self.month - 1) + n;
        Period { year: total.div_euclid(12), month: total.rem_euclid(12) + 1 }
    }

    /// "Oct 2026".
    pub fn label(self) -> String {
        const NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        format!("{} {}", NAMES[(self.month - 1) as usize], self.year)
    }

    fn months_until(self, end: Period) -> i64 {
        (end.year - self.year) * 12 + (end.month - self.month)
    }
}

/// One month of the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthRow {
    pub period: Period,
    pub revenue: i64,
    pub transactions: i64,
}

fn invalid(field: &'static str, message: &str) -> CommandError {
    CommandError::Validation(vec![FieldError { field, message: message.to_owned() }])
}

/// Reads `from` and `to` (Z2): the default is the 12 months ending in today's month; one bound given sets the other 12
/// months away, except that a missing `to` is today's month.
pub fn parse_range(from: &str, to: &str, today: &str) -> Result<(Period, Period), CommandError> {
    let bound = |field: &'static str, text: &str| -> Result<Option<Period>, CommandError> {
        if text.trim().is_empty() {
            return Ok(None);
        }
        Period::parse(text).map(Some).ok_or_else(|| invalid(field, "Enter a month as YYYYMM, like 202610."))
    };
    let from = bound("from", from)?;
    let to = bound("to", to)?;
    let this_month = Period::of_date(today).ok_or_else(|| invalid("to", "Today is not a date."))?;
    let to_or_now = to.unwrap_or(this_month);
    let from = from.unwrap_or_else(|| to_or_now.plus(-11));
    if from > to_or_now {
        return Err(invalid("from", "The first month comes after the last."));
    }
    if from.months_until(to_or_now) >= MAX_MONTHS {
        return Err(invalid("from", "Choose at most 240 months."));
    }
    Ok((from, to_or_now))
}

/// Every month of the range, oldest first.
pub fn sales_summary_by_month(conn: &Connection, from: Period, to: Period) -> rusqlite::Result<Vec<MonthRow>> {
    let mut stmt = conn.prepare(
        "SELECT period, COALESCE(revenue, 0), COALESCE(transactions, 0) FROM sales_summary_by_month
         WHERE period BETWEEN ?1 AND ?2",
    )?;
    let found: HashMap<i64, (i64, i64)> = stmt
        .query_map(params![from.key(), to.key()], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    let months = from.months_until(to);
    Ok((0..=months)
        .map(|n| {
            let period = from.plus(n);
            let (revenue, transactions) = found.get(&period.key()).copied().unwrap_or((0, 0));
            MonthRow { period, revenue, transactions }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods() {
        assert_eq!(Period::parse("202610").unwrap().key(), 202610);
        for bad in ["", "2026", "20261", "202613", "202600", "2026-10", "abcdef", "20261a", "000101"] {
            assert_eq!(Period::parse(bad), None, "{bad}");
        }
        let oct = Period::of_date("2026-10-28").unwrap();
        assert_eq!(oct.label(), "Oct 2026");
        assert_eq!(oct.plus(-11).key(), 202511, "over a year end");
        assert_eq!(oct.plus(3).key(), 202701);
        assert_eq!(Period::parse("202601").unwrap().plus(-1).key(), 202512);
    }

    #[test]
    fn ranges() {
        let key = |r: (Period, Period)| (r.0.key(), r.1.key());
        assert_eq!(key(parse_range("", "", "2026-10-28").unwrap()), (202511, 202610));
        assert_eq!(key(parse_range("202608", "202609", "2026-10-28").unwrap()), (202608, 202609));
        assert_eq!(key(parse_range("", "202601", "2026-10-28").unwrap()), (202502, 202601));
        assert_eq!(key(parse_range("202608", "", "2026-10-28").unwrap()), (202608, 202610));
        assert!(parse_range("202610", "202609", "2026-10-28").is_err());
        assert!(parse_range("200001", "202610", "2026-10-28").is_err(), "too long");
    }
}
