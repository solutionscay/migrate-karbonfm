//! The import edge (BR-IMPORT-01): a number as a FileMaker export writes it ("33.3333333", "-0.005", ".5", "1200"),
//! read exactly and rounded half away from zero to cents or to the 4 decimals of a quantity, as FileMaker Round()
//! does. The caller learns whether rounding changed the value, so the import can list it (BR-SALES-01).

use crate::decimal::ParseError;
use crate::{Cents, Qty, QTY_DECIMALS};

/// A value read from the export and whether it is exactly the exported number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rounded<T> {
    pub value: T,
    /// False when digits were dropped by rounding ("33.3333333" -> 3333 cents).
    pub exact: bool,
}

/// Reads a FileMaker number into cents: 2 decimals, half away from zero. "33.3333333" -> 3333 (inexact),
/// "100.005" -> 10001 (inexact), "-0.005" -> -1, "1200" -> 120000 (exact). No thousands separators, no `$`.
pub fn cents(input: &str) -> Result<Rounded<Cents>, ParseError> {
    scaled(input, 2).map(|(value, exact)| Rounded { value: Cents(value), exact })
}

/// Reads a FileMaker number into a quantity: 4 decimals, half away from zero (BR-SALES-01 allows at most 4).
pub fn qty(input: &str) -> Result<Rounded<Qty>, ParseError> {
    scaled(input, QTY_DECIMALS).map(|(value, exact)| Rounded { value: Qty::from_scaled(value), exact })
}

/// `input` in units of 10^-scale, rounded half away from zero, and whether it was exact.
fn scaled(input: &str, scale: u32) -> Result<(i64, bool), ParseError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ParseError::Empty);
    }
    let (negative, rest) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let (whole, frac) = match rest.split_once('.') {
        Some((w, f)) => (w, f),
        None => (rest, ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return Err(ParseError::NoDigits);
    }
    if !whole.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return Err(ParseError::InvalidCharacter);
    }
    let keep = frac.len().min(scale as usize);
    let (kept, dropped) = frac.split_at(keep);
    let mut units: i64 = 0;
    for b in whole.bytes().chain(kept.bytes()) {
        units = units.checked_mul(10).and_then(|u| u.checked_add(i64::from(b - b'0'))).ok_or(ParseError::Overflow)?;
    }
    for _ in keep..scale as usize {
        units = units.checked_mul(10).ok_or(ParseError::Overflow)?;
    }
    // Half away from zero on the magnitude: the first dropped digit decides ("5" and more rounds up, ties included).
    if dropped.as_bytes().first().is_some_and(|&d| d >= b'5') {
        units = units.checked_add(1).ok_or(ParseError::Overflow)?;
    }
    let exact = dropped.bytes().all(|b| b == b'0');
    Ok((if negative { -units } else { units }, exact))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> (i64, bool) {
        let r = cents(s).unwrap();
        (r.value.0, r.exact)
    }

    #[test]
    fn br_sales_01_import_examples() {
        // BR-SALES-01: "import of 33.3333333 -> 3333".
        assert_eq!(c("33.3333333"), (3333, false));
        assert_eq!(c("1200"), (120000, true));
        assert_eq!(c("1200.00"), (120000, true));
        assert_eq!(c("1200.0000000"), (120000, true));
        assert_eq!(c("32.5"), (3250, true));
        assert_eq!(c(".75"), (75, true));
        assert_eq!(c("5."), (500, true));
    }

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(c("100.005"), (10001, false));
        assert_eq!(c("100.0049999"), (10000, false));
        assert_eq!(c("-0.005"), (-1, false));
        assert_eq!(c("-0.0049"), (0, false));
        assert_eq!(c("0.125"), (13, false));
        assert_eq!(c("-2.675"), (-268, false));
        assert_eq!(c("+4.5"), (450, true));
    }

    #[test]
    fn quantities_keep_four_decimals() {
        assert_eq!(qty("1.5").unwrap(), Rounded { value: Qty::from_scaled(15000), exact: true });
        assert_eq!(qty("1.33333").unwrap(), Rounded { value: Qty::from_scaled(13333), exact: false });
        assert_eq!(qty("0.00005").unwrap(), Rounded { value: Qty::from_scaled(1), exact: false });
        assert_eq!(qty("2").unwrap(), Rounded { value: Qty::from_scaled(20000), exact: true });
    }

    #[test]
    fn refuses_what_is_not_a_filemaker_number() {
        assert_eq!(cents(""), Err(ParseError::Empty));
        assert_eq!(cents("  "), Err(ParseError::Empty));
        assert_eq!(cents("-"), Err(ParseError::NoDigits));
        assert_eq!(cents("."), Err(ParseError::NoDigits));
        for junk in ["call first", "1,200.00", "$12", "12a", "1.2.3", "1e5", "--5", "5-"] {
            assert_eq!(cents(junk), Err(ParseError::InvalidCharacter), "{junk}");
        }
        assert_eq!(cents("99999999999999999999"), Err(ParseError::Overflow));
    }
}
