//! Money display. Money is integer cents everywhere (OD-04-02); this formats it at the template edge.
//! Parsing and rounding at the edges are spike S-03.

/// Formats cents as US dollars: `305715` -> `$3,057.15`, `-50000` -> `-$500.00`.
pub fn usd(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let dollars = (abs / 100).to_string();
    let mut grouped = String::with_capacity(dollars.len() + dollars.len() / 3);
    for (i, ch) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}${grouped}.{:02}", abs % 100)
}

#[cfg(test)]
mod tests {
    use super::usd;

    #[test]
    fn formats_cents() {
        assert_eq!(usd(0), "$0.00");
        assert_eq!(usd(5), "$0.05");
        assert_eq!(usd(305715), "$3,057.15");
        assert_eq!(usd(2961905), "$29,619.05");
        assert_eq!(usd(123456789), "$1,234,567.89");
        assert_eq!(usd(-50000), "-$500.00");
    }
}
