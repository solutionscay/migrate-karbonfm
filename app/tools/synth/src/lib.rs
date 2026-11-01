//! A deterministic synthetic FileMaker export of KarbonFM, shaped like the export the runbook asks the user for
//! (03 "Data migration runbook"; OD-03-01): one UTF-8 CSV per karbon_data base table, named `<Table>.csv`, with the
//! DDR field names as the header row, plus the external container folder `Container/<yyyy>/<mm>/<dd>/`.
//!
//! FileMaker formats, as an export writes them:
//! - dates `M/D/YYYY`, times `H:MM:SS AM`, timestamps `M/D/YYYY H:MM:SS PM` (no leading zeros on month, day, hour);
//! - numbers without trailing zeros ("1200", "32.5"), unit prices back-computed to 7 decimals ("33.3333333");
//! - an empty field is an empty string; a return inside a field is a vertical tab (ASCII 11);
//! - booleans are "1", "0" or empty; container fields hold the path of the file in the export's Container folder.
//!
//! The story continues 04_demo_seed.sql: Ellison Studio (an interiors and wayfinding studio in Portland, Maine), its
//! owner Mara Ellison, Halden & Pryce Architects and invoice INV-1048 keep their demo ids and values. Around them: about
//! 450 parties, 4,000 invoices and sales receipts over 46 months with seasonality, payments, opportunities, activities,
//! templates and file metadata. A small documented set of dirty rows ([`dirty`]) exercises BR-IMPORT-01.
//!
//! The same seed and scale always give the same bytes: the generator has its own PRNG (SplitMix64), never the clock.
//! The column list of every table comes from `migration/10_ddr_schema_audit.tsv`, so a field name the DDR does not
//! have is a panic, not a silent extra column.

pub mod dirty;
mod story;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

pub use story::{ANCHOR_INVOICE, ANCHOR_OWNER_ORG, ANCHOR_OWNER_PERSON};

const REGISTER: &str = include_str!("../../../../migration/10_ddr_schema_audit.tsv");

/// What to generate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub seed: u64,
    /// 1.0 is the "small real business": about 450 parties and 4,000 sales transactions. Tests use less.
    pub scale: f64,
    /// Add the dirty rows of [`dirty`] (BR-IMPORT-01 rejects and reports).
    pub dirty: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { seed: 20261101, scale: 1.0, dirty: true }
    }
}

/// What was written: rows per table and the container files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub rows: BTreeMap<String, usize>,
    pub container_files: usize,
    pub bytes: u64,
}

impl Summary {
    pub fn total_rows(&self) -> usize {
        self.rows.values().sum()
    }
}

/// Writes the export into `out` (created if needed; existing `<Table>.csv` files are replaced).
pub fn generate(config: Config, out: &Path) -> std::io::Result<Summary> {
    let mut export = Export::new();
    story::build(&mut export, config);
    if config.dirty {
        dirty::add(&mut export);
    }
    export.write(out)
}

/// The tables of the export: header from the register, rows in insertion order.
pub struct Export {
    tables: BTreeMap<&'static str, Table>,
    containers: Vec<(String, Vec<u8>)>,
}

pub struct Table {
    fields: Vec<&'static str>,
    rows: Vec<BTreeMap<&'static str, String>>,
}

/// A row being built: field name -> FileMaker text.
pub type Row = BTreeMap<&'static str, String>;

impl Export {
    fn new() -> Self {
        let mut fields: BTreeMap<&'static str, Vec<(u32, &'static str)>> = BTreeMap::new();
        for line in REGISTER.lines().skip(1) {
            let c: Vec<&'static str> = line.split('\t').collect();
            // A FileMaker export of "all fields" of a karbon_data table: the stored, non-global fields.
            if c[0] == "karbon_data.fmp12" && c[6] == "Normal" && c[8] == "0" {
                fields.entry(c[2]).or_default().push((c[3].parse().unwrap(), c[4]));
            }
        }
        let tables = fields
            .into_iter()
            .map(|(t, mut f)| {
                f.sort();
                (t, Table { fields: f.into_iter().map(|(_, n)| n).collect(), rows: Vec::new() })
            })
            .collect();
        Export { tables, containers: Vec::new() }
    }

    /// Adds a row to `table`. Panics on a table or field the DDR does not have.
    pub fn push(&mut self, table: &str, row: Row) {
        let t = self.tables.get_mut(table).unwrap_or_else(|| panic!("no DDR table {table}"));
        for k in row.keys() {
            assert!(t.fields.contains(k), "no field {table}::{k} in the DDR");
        }
        t.rows.push(row);
    }

    /// Adds a file to the export's Container folder (`path` relative to it).
    pub fn container(&mut self, path: String, bytes: Vec<u8>) {
        self.containers.push((path, bytes));
    }

    /// Whether `table` has `field` (stamps are filled only where the table has them).
    pub fn has_field(&self, table: &str, field: &str) -> bool {
        self.tables.get(table).is_some_and(|t| t.fields.contains(&field))
    }

    /// The first row of `table` whose `field` is `value`.
    pub fn find_mut(&mut self, table: &str, field: &str, value: &str) -> Option<&mut Row> {
        self.tables.get_mut(table)?.rows.iter_mut().find(|r| r.get(field).is_some_and(|v| v == value))
    }

    pub fn rows_mut(&mut self, table: &str) -> &mut Vec<Row> {
        &mut self.tables.get_mut(table).unwrap_or_else(|| panic!("no DDR table {table}")).rows
    }

    pub fn rows(&self, table: &str) -> usize {
        self.tables.get(table).map_or(0, |t| t.rows.len())
    }

    /// The tables written: every karbon_data table the import loads (header only when empty), and the chat tables
    /// the import must skip. Not written: tables the plan drops and no install exports (meta, Globals, ...).
    fn exported(name: &str) -> bool {
        !matches!(name, "meta" | "Globals" | "HTTPRequestLog" | "ChartingLibrary" | "PartyImport" | "MergeFieldConfig")
    }

    fn write(&self, out: &Path) -> std::io::Result<Summary> {
        std::fs::create_dir_all(out)?;
        let mut summary = Summary::default();
        for (name, t) in self.tables.iter().filter(|(n, _)| Export::exported(n)) {
            let mut text = String::new();
            write_record(&mut text, t.fields.iter().copied());
            for row in &t.rows {
                write_record(&mut text, t.fields.iter().map(|f| row.get(f).map_or("", String::as_str)));
            }
            std::fs::write(out.join(format!("{name}.csv")), &text)?;
            summary.bytes += text.len() as u64;
            summary.rows.insert(name.to_string(), t.rows.len());
        }
        for (path, bytes) in &self.containers {
            let p = out.join("Container").join(path);
            std::fs::create_dir_all(p.parent().unwrap())?;
            std::fs::write(&p, bytes)?;
            summary.bytes += bytes.len() as u64;
        }
        summary.container_files = self.containers.len();
        Ok(summary)
    }
}

/// One CSV record: every field quoted (as FileMaker writes CSV), quotes doubled, CRLF at the end.
fn write_record<'a>(out: &mut String, fields: impl Iterator<Item = &'a str>) {
    for (i, f) in fields.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&f.replace('"', "\"\""));
        out.push('"');
    }
    out.push_str("\r\n");
}

/// SplitMix64: small, fast and fixed forever, so a seed always gives the same export.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(lo <= hi);
        lo + (self.next_u64() % ((hi - lo + 1) as u64)) as i64
    }

    /// A float in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.range(0, items.len() as i64 - 1) as usize]
    }

    /// An index chosen with the given weights.
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut x = self.unit() * total;
        for (i, w) in weights.iter().enumerate() {
            if x < *w {
                return i;
            }
            x -= w;
        }
        weights.len() - 1
    }

    /// A FileMaker `Get(UUID)` value: an RFC 4122 version 4 UUID in upper case.
    pub fn uuid(&mut self) -> String {
        let (a, b) = (self.next_u64(), self.next_u64());
        format!(
            "{:08X}-{:04X}-4{:03X}-{:04X}-{:012X}",
            a >> 32,
            (a >> 16) & 0xFFFF,
            a & 0xFFF,
            0x8000 | ((b >> 48) & 0x3FFF),
            b & 0xFFFF_FFFF_FFFF
        )
    }
}

/// FileMaker date text: `M/D/YYYY`.
pub fn fm_date(d: NaiveDate) -> String {
    format!("{}/{}/{}", d.month(), d.day(), d.year())
}

/// FileMaker time text: `H:MM:SS AM`.
pub fn fm_time(t: NaiveDateTime) -> String {
    let (pm, h) = t.hour12();
    format!("{h}:{:02}:{:02} {}", t.minute(), t.second(), if pm { "PM" } else { "AM" })
}

/// FileMaker timestamp text: `M/D/YYYY H:MM:SS PM`.
pub fn fm_timestamp(t: NaiveDateTime) -> String {
    format!("{} {}", fm_date(t.date()), fm_time(t))
}

/// A money amount as FileMaker writes the number: no trailing zeros, no separators ("1200", "32.5", "-0.05").
pub fn fm_money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let (whole, frac) = (cents.unsigned_abs() / 100, cents.unsigned_abs() % 100);
    match frac {
        0 => format!("{sign}{whole}"),
        f if f % 10 == 0 => format!("{sign}{whole}.{}", f / 10),
        f => format!("{sign}{whole}.{f:02}"),
    }
}

/// A quantity in units of 0.0001 as FileMaker writes it ("2", "1.5", "0.25").
pub fn fm_qty(units: i64) -> String {
    let mut s = format!("{}.{:04}", units / 10_000, (units % 10_000).abs());
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// A text value with paragraph returns as FileMaker exports them (vertical tab).
pub fn fm_text(s: &str) -> String {
    s.replace('\n', "\u{0B}")
}

/// `1` or `0`.
pub fn fm_bool(b: bool) -> String {
    if b { "1" } else { "0" }.to_string()
}

/// A small, valid-looking placeholder for a container file of the given extension.
fn placeholder(name: &str, ext: &str) -> Vec<u8> {
    let mut s = String::new();
    let _ = match ext {
        "pdf" => writeln!(s, "%PDF-1.4\n% KarbonFM synthetic export: {name}\n%%EOF"),
        _ => writeln!(s, "KarbonFM synthetic export: {name}"),
    };
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filemaker_formats() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
        assert_eq!(fm_date(d), "3/7/2026");
        let t = d.and_hms_opt(14, 5, 9).unwrap();
        assert_eq!(fm_timestamp(t), "3/7/2026 2:05:09 PM");
        assert_eq!(fm_time(d.and_hms_opt(0, 30, 0).unwrap()), "12:30:00 AM");
        assert_eq!(fm_money(120000), "1200");
        assert_eq!(fm_money(3250), "32.5");
        assert_eq!(fm_money(-5), "-0.05");
        assert_eq!(fm_money(123456), "1234.56");
        assert_eq!(fm_qty(15000), "1.5");
        assert_eq!(fm_qty(20000), "2");
        assert_eq!(fm_qty(2500), "0.25");
        assert_eq!(fm_text("a\nb"), "a\u{0B}b");
    }

    #[test]
    fn csv_quotes_every_field() {
        let mut s = String::new();
        write_record(&mut s, ["a", "say \"hi\"", ""].into_iter());
        assert_eq!(s, "\"a\",\"say \"\"hi\"\"\",\"\"\r\n");
    }

    #[test]
    fn rng_is_fixed() {
        let mut r = Rng::new(1);
        let a: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
        let mut r = Rng::new(1);
        assert_eq!(a, (0..3).map(|_| r.next_u64()).collect::<Vec<_>>());
        let u = Rng::new(7).uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
    }

    #[test]
    fn the_same_seed_writes_the_same_bytes() {
        let small = Config { scale: 0.05, ..Config::default() };
        let (a, b) = (tempdir("a"), tempdir("b"));
        let sa = generate(small, &a).unwrap();
        let sb = generate(small, &b).unwrap();
        assert_eq!(sa, sb);
        for name in ["Party.csv", "SalesTransaction.csv", "SalesLine.csv", "Activity.csv"] {
            assert_eq!(std::fs::read(a.join(name)).unwrap(), std::fs::read(b.join(name)).unwrap(), "{name}");
        }
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("karbon-synth-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }
}
