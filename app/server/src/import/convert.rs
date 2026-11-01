//! FileMaker export text to the stored form of 04 (BR-IMPORT-01; D-05 local civil time, ISO 8601 text):
//! dates `M/D/YYYY` -> `YYYY-MM-DD`, times `H:MM[:SS] [AM|PM]` -> `HH:MM:SS`, timestamps -> `YYYY-MM-DD HH:MM:SS`.
//! ISO input is accepted as well, so an export made with ISO formats loads too.

use chrono::{NaiveDate, NaiveTime};

/// `YYYY-MM-DD`, or None when the text is not a real date ("2/30/2025", "13/01/2024").
pub fn date(s: &str) -> Option<String> {
    let s = s.trim();
    let d = if let Some((y, rest)) = s.split_once('-') {
        let (m, d) = rest.split_once('-')?;
        NaiveDate::from_ymd_opt(num(y)?, num(m)?, num(d)?)?
    } else {
        let mut p = s.split('/');
        let (m, d, y) = (p.next()?, p.next()?, p.next()?);
        if p.next().is_some() || y.len() != 4 {
            return None;
        }
        NaiveDate::from_ymd_opt(num(y)?, num(m)?, num(d)?)?
    };
    Some(d.format("%Y-%m-%d").to_string())
}

/// `HH:MM:SS`, or None.
pub fn time(s: &str) -> Option<String> {
    let s = s.trim();
    let (clock, meridiem) = match s.rsplit_once(' ') {
        Some((c, m)) if m.eq_ignore_ascii_case("AM") || m.eq_ignore_ascii_case("PM") => (c.trim(), Some(m)),
        _ => (s, None),
    };
    let mut p = clock.split(':');
    let mut h: u32 = num(p.next()?)?;
    let m: u32 = num(p.next()?)?;
    let sec: u32 = match p.next() {
        // FileMaker may write fractional seconds; the stored time keeps whole seconds.
        Some(x) => num(x.split('.').next()?)?,
        None => 0,
    };
    if p.next().is_some() {
        return None;
    }
    if let Some(mer) = meridiem {
        if !(1..=12).contains(&h) {
            return None;
        }
        let pm = mer.eq_ignore_ascii_case("PM");
        h = match (h, pm) {
            (12, false) => 0,
            (12, true) => 12,
            (h, true) => h + 12,
            (h, false) => h,
        };
    }
    NaiveTime::from_hms_opt(h, m, sec).map(|t| t.format("%H:%M:%S").to_string())
}

/// `YYYY-MM-DD HH:MM:SS`, or None. A timestamp with no time part is midnight.
pub fn timestamp(s: &str) -> Option<String> {
    let s = s.trim();
    let (d, t) = match s.split_once([' ', 'T']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let t = match t {
        Some(t) => time(t)?,
        None => "00:00:00".into(),
    };
    Some(format!("{} {t}", date(d)?))
}

/// Whether the text is a FileMaker number: optional sign, digits, optional decimals.
pub fn is_number(s: &str) -> bool {
    let s = s.trim();
    let s = s.strip_prefix(['-', '+']).unwrap_or(s);
    let (w, f) = s.split_once('.').unwrap_or((s, ""));
    !(w.is_empty() && f.is_empty()) && w.bytes().chain(f.bytes()).all(|b| b.is_ascii_digit())
}

/// The canonical text of a key value for matching a parent row: a numeric key "2.0" or "02" is "2".
pub fn canonical_key(s: &str, numeric: bool) -> String {
    let s = s.trim();
    if numeric && is_number(s) {
        if let Ok(f) = s.parse::<f64>() {
            if f.fract() == 0.0 && f.abs() < 9e15 {
                return format!("{}", f as i64);
            }
        }
    }
    s.to_string()
}

fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date("3/7/2026").as_deref(), Some("2026-03-07"));
        assert_eq!(date("12/31/2025").as_deref(), Some("2025-12-31"));
        assert_eq!(date("2026-03-07").as_deref(), Some("2026-03-07"));
        assert_eq!(date("2/29/2024").as_deref(), Some("2024-02-29"));
        for bad in ["2/30/2025", "13/01/2024", "1/1/25", "1/1/2025/1", "", "today", "3-7-2026x"] {
            assert_eq!(date(bad), None, "{bad}");
        }
    }

    #[test]
    fn times_and_timestamps() {
        assert_eq!(time("2:05:09 PM").as_deref(), Some("14:05:09"));
        assert_eq!(time("12:30:00 AM").as_deref(), Some("00:30:00"));
        assert_eq!(time("12:00:00 PM").as_deref(), Some("12:00:00"));
        assert_eq!(time("14:05").as_deref(), Some("14:05:00"));
        assert_eq!(time("9:15:30.25").as_deref(), Some("09:15:30"));
        assert_eq!(time("13:00 PM"), None);
        assert_eq!(time("25:00"), None);
        assert_eq!(timestamp("3/7/2026 2:05:09 PM").as_deref(), Some("2026-03-07 14:05:09"));
        assert_eq!(timestamp("3/7/2026 0:00:00").as_deref(), Some("2026-03-07 00:00:00"));
        assert_eq!(timestamp("2026-03-07T08:00:00").as_deref(), Some("2026-03-07 08:00:00"));
        assert_eq!(timestamp("3/7/2026").as_deref(), Some("2026-03-07 00:00:00"));
        assert_eq!(timestamp("2/30/2026 1:00:00 PM"), None);
    }

    #[test]
    fn numbers_and_keys() {
        assert!(is_number("12") && is_number("-0.5") && is_number(".5") && is_number("5."));
        assert!(!is_number("TBD") && !is_number("") && !is_number("1,200") && !is_number("."));
        assert_eq!(canonical_key("2.0", true), "2");
        assert_eq!(canonical_key("02", true), "2");
        assert_eq!(canonical_key("02", false), "02");
        assert_eq!(canonical_key("abc", true), "abc");
    }
}
