//! The one parser behind dollar and quantity text: a signed decimal with optional
//! thousands separators, read exactly into an integer at a fixed scale.

use std::fmt;

/// Why a text value is not a valid amount or quantity. The `Display` text is
/// written for a form field error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Nothing but whitespace.
    Empty,
    /// No digit at all ("$", "-", ".").
    NoDigits,
    /// A character that has no place in a number ("12a", "1 000", "5-", "--5").
    InvalidCharacter,
    /// Commas that are not thousands separators ("12,34", "1,2345", ",5").
    BadGrouping,
    /// More decimal places than the scale holds ("1.234" for money).
    TooManyDecimals { max: u32 },
    /// Outside the i64 range at this scale.
    Overflow,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => f.write_str("Enter a value."),
            ParseError::NoDigits | ParseError::InvalidCharacter => {
                f.write_str("Enter a number, like 1,234.56.")
            }
            ParseError::BadGrouping => {
                f.write_str("Commas must separate thousands, like 1,234.56.")
            }
            ParseError::TooManyDecimals { max } => {
                write!(f, "Use at most {max} decimal places.")
            }
            ParseError::Overflow => f.write_str("The value is too large."),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses `input` into an integer of units of 10^-scale.
///
/// Accepted: surrounding whitespace; one sign (`-` or `+`); when `dollar` is
/// true, one `$` before or after the sign ("-$5", "$-5"); digits with optional
/// thousands commas ("1,234"); an optional `.` with at most `scale` digits after
/// it (".5" and "5." are accepted). Everything else is an error: nothing is
/// rounded or guessed, so the user sees the field error and retypes.
pub(crate) fn parse_scaled(input: &str, scale: u32, dollar: bool) -> Result<i64, ParseError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut rest = s;
    let mut negative = false;
    let mut signed = false;
    let mut dollared = false;
    // At most one sign and one dollar sign, in either order, before the digits.
    loop {
        if !signed && (rest.starts_with('-') || rest.starts_with('+')) {
            negative = rest.starts_with('-');
            signed = true;
            rest = &rest[1..];
        } else if dollar && !dollared && rest.starts_with('$') {
            dollared = true;
            rest = &rest[1..];
        } else {
            break;
        }
    }

    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, f),
        None => (rest, ""),
    };
    if !int_part.chars().all(|c| c.is_ascii_digit() || c == ',')
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return Err(ParseError::InvalidCharacter);
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(ParseError::NoDigits);
    }
    if int_part.contains(',') {
        let mut groups = int_part.split(',');
        let first = groups.next().unwrap_or("");
        if first.is_empty() || first.len() > 3 || groups.any(|g| g.len() != 3) {
            return Err(ParseError::BadGrouping);
        }
    }
    if frac_part.len() > scale as usize {
        return Err(ParseError::TooManyDecimals { max: scale });
    }

    // Accumulate as a negative number so that i64::MIN parses too.
    let mut value: i64 = 0;
    let digits = int_part
        .bytes()
        .filter(|b| *b != b',')
        .chain(frac_part.bytes())
        .chain(std::iter::repeat_n(b'0', scale as usize - frac_part.len()));
    for b in digits {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_sub(i64::from(b - b'0')))
            .ok_or(ParseError::Overflow)?;
    }
    if negative {
        Ok(value)
    } else {
        value.checked_neg().ok_or(ParseError::Overflow)
    }
}

/// Inserts thousands commas into a run of ASCII digits.
pub(crate) fn group_thousands(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(group_thousands("0"), "0");
        assert_eq!(group_thousands("999"), "999");
        assert_eq!(group_thousands("1000"), "1,000");
        assert_eq!(group_thousands("1234567"), "1,234,567");
    }

    #[test]
    fn scale_zero_rejects_any_decimal() {
        assert_eq!(parse_scaled("5", 0, false), Ok(5));
        assert_eq!(parse_scaled("5.", 0, false), Ok(5));
        assert_eq!(
            parse_scaled("5.0", 0, false),
            Err(ParseError::TooManyDecimals { max: 0 })
        );
    }
}
