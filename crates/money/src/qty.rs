//! Sales line quantities as exact fixed-point numbers.
//!
//! `sales_line.qty` is NUMERIC in 04, so SQLite stores 1.5 as a REAL. Never
//! multiply that f64 by cents: read the column as text (`CAST(qty AS TEXT)`,
//! which round-trips up to 15 significant digits) and parse it with `Qty::parse`.

use std::fmt;
use std::str::FromStr;

use crate::decimal::{group_thousands, parse_scaled, ParseError};

/// Decimal places a quantity keeps (0.0001). Enough for hours in minutes
/// (0.25, 1.3333) and unit fractions; more is a field error.
pub const QTY_DECIMALS: u32 = 4;

/// 10^QTY_DECIMALS: the number of `Qty` units in one.
pub const QTY_SCALE: i64 = 10_000;

/// A quantity in units of 0.0001.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Qty(i64);

impl Qty {
    pub const ONE: Qty = Qty(QTY_SCALE);

    /// The quantity of `units / 10_000`.
    pub const fn from_scaled(units: i64) -> Qty {
        Qty(units)
    }

    /// A whole quantity; `None` on overflow.
    pub fn from_whole(n: i64) -> Option<Qty> {
        n.checked_mul(QTY_SCALE).map(Qty)
    }

    /// The raw value in units of 0.0001.
    pub const fn scaled(self) -> i64 {
        self.0
    }

    /// Parses "1.5", "2", "0.25", "-1", "1,000". At most four decimal places.
    pub fn parse(input: &str) -> Result<Qty, ParseError> {
        parse_scaled(input, QTY_DECIMALS, false).map(Qty)
    }

    /// Like `parse`, but an empty field is `None`. An empty quantity counts as 1
    /// in the line amount (BR-SALES-01).
    pub fn parse_optional(input: &str) -> Result<Option<Qty>, ParseError> {
        match Qty::parse(input) {
            Err(ParseError::Empty) => Ok(None),
            other => other.map(Some),
        }
    }

    /// The value for an `<input>` field and for writing the column: no commas,
    /// no trailing zeros ("1.5", "2", "-0.0001").
    pub fn to_input_value(self) -> String {
        let (sign, whole, frac) = self.parts();
        match frac {
            Some(frac) => format!("{sign}{whole}.{frac}"),
            None => format!("{sign}{whole}"),
        }
    }

    fn parts(self) -> (&'static str, u64, Option<String>) {
        let abs = self.0.unsigned_abs();
        let sign = if self.0 < 0 { "-" } else { "" };
        let scale = QTY_SCALE as u64;
        let frac = abs % scale;
        let frac = (frac != 0).then(|| {
            let digits = format!("{frac:0width$}", width = QTY_DECIMALS as usize);
            digits.trim_end_matches('0').to_string()
        });
        (sign, abs / scale, frac)
    }
}

/// The template format: thousands commas, no trailing zeros ("1,000", "1.5").
impl fmt::Display for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (sign, whole, frac) = self.parts();
        let whole = group_thousands(&whole.to_string());
        let text = match frac {
            Some(frac) => format!("{sign}{whole}.{frac}"),
            None => format!("{sign}{whole}"),
        };
        f.pad(&text)
    }
}

impl FromStr for Qty {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Qty::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Result<i64, ParseError> {
        Qty::parse(s).map(Qty::scaled)
    }

    #[test]
    fn parses_quantities() {
        assert_eq!(q("1"), Ok(10_000));
        assert_eq!(q("1.5"), Ok(15_000));
        assert_eq!(q("0.25"), Ok(2_500));
        assert_eq!(q(".5"), Ok(5_000));
        assert_eq!(q("1.3333"), Ok(13_333));
        assert_eq!(q("1,000"), Ok(10_000_000));
        assert_eq!(q("-1"), Ok(-10_000));
        assert_eq!(q("1.50"), Ok(15_000)); // what CAST(1.50 AS TEXT) never gives, but a user may type
    }

    #[test]
    fn rejects_bad_quantities() {
        assert_eq!(q("1.33333"), Err(ParseError::TooManyDecimals { max: 4 }));
        assert_eq!(q("$1"), Err(ParseError::InvalidCharacter));
        assert_eq!(q("1,5"), Err(ParseError::BadGrouping));
        assert_eq!(q("1e3"), Err(ParseError::InvalidCharacter));
        assert_eq!(q(""), Err(ParseError::Empty));
        assert_eq!(Qty::parse_optional(""), Ok(None));
    }

    #[test]
    fn sqlite_real_text_round_trips() {
        // CAST(qty AS TEXT) of the REAL SQLite stores for these inputs.
        for (text, units) in [
            ("1.5", 15_000),
            ("0.1", 1_000),
            ("1.3333", 13_333),
            ("2", 20_000),
        ] {
            assert_eq!(q(text), Ok(units));
        }
    }

    #[test]
    fn formats() {
        assert_eq!(Qty::ONE.to_string(), "1");
        assert_eq!(Qty::from_scaled(15_000).to_string(), "1.5");
        assert_eq!(Qty::from_scaled(2_500).to_string(), "0.25");
        assert_eq!(Qty::from_scaled(1).to_string(), "0.0001");
        assert_eq!(Qty::from_scaled(-12_345_000).to_string(), "-1,234.5");
        assert_eq!(Qty::from_scaled(-12_345_000).to_input_value(), "-1234.5");
        assert_eq!(Qty::from_whole(6), Some(Qty::from_scaled(60_000)));
        assert_eq!(Qty::from_whole(i64::MAX), None);
    }
}
