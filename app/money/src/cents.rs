//! Money as integer cents (OD-04-02), with the two text edges: parsing dollar
//! text from a form field and formatting cents for a template.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};
use std::str::FromStr;

use crate::decimal::{group_thousands, parse_scaled, ParseError};

/// An amount of money in US cents. Every money column in 04 holds one of these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cents(pub i64);

impl Cents {
    pub const ZERO: Cents = Cents(0);

    /// Parses dollar text from a form field: "1,234.56", "$5", "0.5", "-$12.30",
    /// "$-12.30", ".75". At most two decimal places: "1.234" is an error, not a
    /// rounded value. An empty field is `ParseError::Empty`; the handler decides
    /// whether that means NULL or a required-field error. Negative amounts parse;
    /// the handler refuses them where the field must not be negative.
    pub fn parse(input: &str) -> Result<Cents, ParseError> {
        parse_scaled(input, 2, true).map(Cents)
    }

    /// Like `parse`, but an empty field is `None` (an optional money column).
    pub fn parse_optional(input: &str) -> Result<Option<Cents>, ParseError> {
        match Cents::parse(input) {
            Err(ParseError::Empty) => Ok(None),
            other => other.map(Some),
        }
    }

    /// The value for an `<input>` field: no `$`, no commas, always two decimals
    /// ("-1234.50"). `Cents::parse` reads it back unchanged.
    pub fn to_input_value(self) -> String {
        let (sign, whole, frac) = self.parts();
        format!("{sign}{whole}.{frac:02}")
    }

    pub fn checked_add(self, other: Cents) -> Option<Cents> {
        self.0.checked_add(other.0).map(Cents)
    }

    pub fn checked_sub(self, other: Cents) -> Option<Cents> {
        self.0.checked_sub(other.0).map(Cents)
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    fn parts(self) -> (&'static str, u64, u64) {
        let abs = self.0.unsigned_abs();
        let sign = if self.0 < 0 { "-" } else { "" };
        (sign, abs / 100, abs % 100)
    }
}

/// The template format: "$1,234.56", "-$1,234.56", "$0.00". askama renders
/// `{{ amount }}` with this.
impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (sign, whole, frac) = self.parts();
        let text = format!("{sign}${}.{frac:02}", group_thousands(&whole.to_string()));
        f.pad(&text)
    }
}

impl FromStr for Cents {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Cents::parse(s)
    }
}

// The operators panic on overflow in every build profile, never wrap. An
// overflow needs amounts near 92 quadrillion dollars; the command layer uses
// the checked_ forms where input is not bounded.
impl Add for Cents {
    type Output = Cents;

    fn add(self, other: Cents) -> Cents {
        self.checked_add(other).expect("cents overflow")
    }
}

impl Sub for Cents {
    type Output = Cents;

    fn sub(self, other: Cents) -> Cents {
        self.checked_sub(other).expect("cents overflow")
    }
}

impl AddAssign for Cents {
    fn add_assign(&mut self, other: Cents) {
        *self = *self + other;
    }
}

impl SubAssign for Cents {
    fn sub_assign(&mut self, other: Cents) {
        *self = *self - other;
    }
}

impl Neg for Cents {
    type Output = Cents;

    fn neg(self) -> Cents {
        Cents(self.0.checked_neg().expect("cents overflow"))
    }
}

impl Sum for Cents {
    fn sum<I: Iterator<Item = Cents>>(iter: I) -> Cents {
        iter.fold(Cents::ZERO, Add::add)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<i64, ParseError> {
        Cents::parse(s).map(|c| c.0)
    }

    #[test]
    fn parses_plain_and_grouped_dollars() {
        assert_eq!(p("1,234.56"), Ok(123_456));
        assert_eq!(p("1234.56"), Ok(123_456));
        assert_eq!(p("1,234,567.89"), Ok(123_456_789));
        assert_eq!(p("999"), Ok(99_900));
        assert_eq!(p("0"), Ok(0));
        assert_eq!(p("007.10"), Ok(710));
    }

    #[test]
    fn parses_dollar_sign() {
        assert_eq!(p("$5"), Ok(500));
        assert_eq!(p("$1,200.00"), Ok(120_000));
        assert_eq!(p("  $5  "), Ok(500));
    }

    #[test]
    fn parses_short_decimals() {
        assert_eq!(p("0.5"), Ok(50));
        assert_eq!(p(".5"), Ok(50));
        assert_eq!(p(".05"), Ok(5));
        assert_eq!(p("5."), Ok(500));
        assert_eq!(p("$.99"), Ok(99));
    }

    #[test]
    fn parses_negatives() {
        assert_eq!(p("-5"), Ok(-500));
        assert_eq!(p("-$5"), Ok(-500));
        assert_eq!(p("$-5"), Ok(-500));
        assert_eq!(p("-1,000.00"), Ok(-100_000));
        assert_eq!(p("-0.01"), Ok(-1));
        assert_eq!(p("-0"), Ok(0));
        assert_eq!(p("+5"), Ok(500));
    }

    #[test]
    fn rejects_too_many_decimals() {
        assert_eq!(p("1.234"), Err(ParseError::TooManyDecimals { max: 2 }));
        assert_eq!(p("0.005"), Err(ParseError::TooManyDecimals { max: 2 }));
        // Trailing zeros too: "1.500" may be a European 1500.
        assert_eq!(p("1.500"), Err(ParseError::TooManyDecimals { max: 2 }));
        // The 7-decimal source unit price is an import concern (deferred with I3).
        assert_eq!(p("33.3333333"), Err(ParseError::TooManyDecimals { max: 2 }));
    }

    #[test]
    fn rejects_bad_grouping() {
        assert_eq!(p("12,34"), Err(ParseError::BadGrouping));
        assert_eq!(p("1,2345"), Err(ParseError::BadGrouping));
        assert_eq!(p("1234,567"), Err(ParseError::BadGrouping));
        assert_eq!(p(",123"), Err(ParseError::BadGrouping));
        assert_eq!(p("1,,234"), Err(ParseError::BadGrouping));
        assert_eq!(p("1,234,"), Err(ParseError::BadGrouping));
        // European decimal comma: an error, never 150 or 15000 cents.
        assert_eq!(p("1,5"), Err(ParseError::BadGrouping));
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(p(""), Err(ParseError::Empty));
        assert_eq!(p("   "), Err(ParseError::Empty));
        assert_eq!(p("$"), Err(ParseError::NoDigits));
        assert_eq!(p("-"), Err(ParseError::NoDigits));
        assert_eq!(p("."), Err(ParseError::NoDigits));
        assert_eq!(p("-$."), Err(ParseError::NoDigits));
        for junk in [
            "abc",
            "12a",
            "1 000",
            "5-",
            "--5",
            "-$-5",
            "$$5",
            "5$",
            "1.2.3",
            "1e3",
            "0x10",
            "(5.00)",
            "\u{2212}5",
            "５",
            "NaN",
            "inf",
            "1_000",
            "USD 5",
            "5 USD",
            "€5",
        ] {
            assert!(p(junk).is_err(), "{junk:?} parsed as {:?}", p(junk));
        }
    }

    #[test]
    fn rejects_overflow_and_keeps_the_extremes() {
        assert_eq!(p("92,233,720,368,547,758.07"), Ok(i64::MAX));
        assert_eq!(p("-92,233,720,368,547,758.08"), Ok(i64::MIN));
        assert_eq!(p("92,233,720,368,547,758.08"), Err(ParseError::Overflow));
        assert_eq!(p("99999999999999999999"), Err(ParseError::Overflow));
    }

    #[test]
    fn optional_field() {
        assert_eq!(Cents::parse_optional(""), Ok(None));
        assert_eq!(Cents::parse_optional(" "), Ok(None));
        assert_eq!(Cents::parse_optional("$5"), Ok(Some(Cents(500))));
        assert!(Cents::parse_optional("x").is_err());
    }

    #[test]
    fn formats_for_templates() {
        assert_eq!(Cents(0).to_string(), "$0.00");
        assert_eq!(Cents(5).to_string(), "$0.05");
        assert_eq!(Cents(50).to_string(), "$0.50");
        assert_eq!(Cents(100_000).to_string(), "$1,000.00");
        assert_eq!(Cents(2_961_905).to_string(), "$29,619.05");
        assert_eq!(Cents(-205_715).to_string(), "-$2,057.15");
        assert_eq!(Cents(-1).to_string(), "-$0.01");
        assert_eq!(Cents(i64::MAX).to_string(), "$92,233,720,368,547,758.07");
        assert_eq!(Cents(i64::MIN).to_string(), "-$92,233,720,368,547,758.08");
        assert_eq!(format!("{:>12}", Cents(715)), "       $7.15");
    }

    #[test]
    fn formats_for_input_fields() {
        assert_eq!(Cents(0).to_input_value(), "0.00");
        assert_eq!(Cents(123_456).to_input_value(), "1234.56");
        assert_eq!(Cents(-5).to_input_value(), "-0.05");
        assert_eq!(Cents(i64::MIN).to_input_value(), "-92233720368547758.08");
    }

    #[test]
    fn parse_error_messages_read_as_field_errors() {
        assert_eq!(Cents::parse("1.234").unwrap_err().to_string(), "Use at most 2 decimal places.");
        assert_eq!(Cents::parse("abc").unwrap_err().to_string(), "Enter a number, like 1,234.56.");
    }

    #[test]
    fn arithmetic() {
        assert_eq!(Cents(100) + Cents(-250), Cents(-150));
        assert_eq!(Cents(100) - Cents(250), Cents(-150));
        assert_eq!(-Cents(5), Cents(-5));
        assert_eq!([Cents(1), Cents(2), Cents(3)].into_iter().sum::<Cents>(), Cents(6));
        assert_eq!(Cents(i64::MAX).checked_add(Cents(1)), None);
    }

    #[test]
    #[should_panic(expected = "cents overflow")]
    fn add_overflow_panics_instead_of_wrapping() {
        let _ = Cents(i64::MAX) + Cents(1);
    }
}
