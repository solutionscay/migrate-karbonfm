//! The CSV of a FileMaker export (OD-03-01): RFC 4180 quoting, records ended by CRLF, LF or CR (FileMaker on macOS
//! writes CR), a return inside a field written as a vertical tab (ASCII 11), turned back into a line break here.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvError {
    /// 1-based record number where the problem starts.
    pub record: usize,
    pub message: &'static str,
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "record {}: {}", self.record, self.message)
    }
}

/// Parses the whole file into records of fields. A trailing line end does not make an empty record.
pub fn parse(text: &str) -> Result<Vec<Vec<String>>, CsvError> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut quoted = false; // the current field started with a quote
    let mut started = false; // the current record has any character
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                '\u{0B}' => field.push('\n'),
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !quoted => {
                in_quotes = true;
                quoted = true;
                started = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                quoted = false;
                started = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if started || !field.is_empty() {
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                quoted = false;
                started = false;
            }
            '\u{0B}' => {
                field.push('\n');
                started = true;
            }
            c => {
                if quoted {
                    return Err(CsvError { record: records.len() + 1, message: "text after a closing quote" });
                }
                field.push(c);
                started = true;
            }
        }
    }
    if in_quotes {
        return Err(CsvError { record: records.len() + 1, message: "a quoted field is not closed" });
    }
    if started || !field.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<Vec<String>> {
        parse(s).unwrap()
    }

    #[test]
    fn filemaker_csv() {
        assert_eq!(p("\"a\",\"b\"\r\n\"1\",\"\"\r\n"), [vec!["a", "b"], vec!["1", ""]]);
        assert_eq!(p("a,b\r1,2\r"), [vec!["a", "b"], vec!["1", "2"]]);
        assert_eq!(p("a,b\n1,2"), [vec!["a", "b"], vec!["1", "2"]]);
        assert_eq!(p("\"say \"\"hi\"\", ok\"\n"), [vec!["say \"hi\", ok"]]);
        assert_eq!(p("\"line 1\u{0B}line 2\"\n"), [vec!["line 1\nline 2"]]);
        assert_eq!(p("\"real\nbreak\"\n"), [vec!["real\nbreak"]]);
        assert_eq!(p("\u{FEFF}\"x\"\n"), [vec!["x"]]);
        assert_eq!(p(",\n"), [vec!["", ""]]);
        assert!(p("").is_empty());
    }

    #[test]
    fn broken_quotes_are_errors() {
        assert_eq!(parse("\"a\n").unwrap_err().message, "a quoted field is not closed");
        assert_eq!(parse("\"a\"b\n").unwrap_err().message, "text after a closing quote");
    }
}
