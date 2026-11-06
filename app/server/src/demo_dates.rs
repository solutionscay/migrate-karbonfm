//! B11: demo data stays current. The seeds' dates are fixed (the small seed on 2026-10-28, the big demo on 2026-10-31),
//! so "Revenue this month" reads $0.00 a month later and the overdue figures drift. When demo data is created, every
//! date and timestamp moves by `today - anchor` days: all gaps stay exact, so the overdue figures stay as designed, and
//! no amount changes. `period` is then rewritten from the new date (BR-SALES-03).
//!
//! It runs only when demo data is created (`Seed::DemoAsOf`, `big_demo::create_as_of`), never on an existing database
//! (03, owner 2026-11-01), never on `Seed::Demo` (the tests' fixed dates) and never in `import_export` (a real export
//! keeps its dates).

use chrono::NaiveDate;
use rusqlite::Connection;

/// The day the small seed's figures are true on (04_demo_seed.sql header: "oldest 41 days on 2026-10-28").
pub const SMALL_SEED_ANCHOR: &str = "2026-10-28";
/// The last day of the synthetic export (`karbon_synth`'s `end()`).
pub const BIG_DEMO_ANCHOR: &str = "2026-10-31";

/// A date-like column that keeps its value: a birthday is not relative to today.
pub const KEPT_COLUMNS: &[(&str, &str)] = &[("person", "date_of_birth")];
/// Tables that keep their dates: the import's own record, the app's bookkeeping, the migrations' history.
pub const KEPT_TABLES: &[&str] = &[
    "import_batch",
    "import_file",
    "import_changed_value",
    "import_rejected_row",
    "app_meta",
    "refinery_schema_history",
];

const DATE_SHAPE: &str = "[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]";

/// True for a column name that holds a date or a timestamp.
fn date_like(column: &str) -> bool {
    column.contains("date") || column.ends_with("_timestamp") || column.ends_with("_at")
}

/// Every (table, column) a shift moves.
pub fn shifted_columns(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p
         WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' ORDER BY 1, 2",
    )?;
    let all = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for pair in all {
        let (table, column) = pair?;
        let kept =
            KEPT_TABLES.contains(&table.as_str()) || KEPT_COLUMNS.iter().any(|(t, c)| *t == table && *c == column);
        if !kept && date_like(&column) {
            out.push((table, column));
        }
    }
    Ok(out)
}

/// Moves every date of the demo data by `today - anchor` days and rewrites `period`. Call it inside the transaction that
/// creates the data. Values that are not a date or a timestamp are left alone. Returns the days moved.
pub fn shift(conn: &Connection, anchor: NaiveDate, today: NaiveDate) -> rusqlite::Result<i64> {
    let days = (today - anchor).num_days();
    if days == 0 {
        return Ok(0);
    }
    let by = format!("{days:+} days");
    for (table, column) in shifted_columns(conn)? {
        // COALESCE: a value SQLite cannot read as a date (a dirty row) stays as it is.
        conn.execute(
            &format!(
                r#"UPDATE "{table}" SET "{column}" = CASE
                     WHEN "{column}" GLOB '{DATE_SHAPE}' THEN COALESCE(date("{column}", ?1), "{column}")
                     WHEN "{column}" GLOB '{DATE_SHAPE} [0-9][0-9]:[0-9][0-9]:[0-9][0-9]'
                       THEN COALESCE(datetime("{column}", ?1), "{column}")
                     ELSE "{column}" END
                   WHERE typeof("{column}") = 'text'"#
            ),
            [&by],
        )?;
    }
    conn.execute(
        "UPDATE sales_transaction SET period = CAST(strftime('%Y%m', date) AS INTEGER) WHERE date IS NOT NULL",
        [],
    )?;
    Ok(days)
}

/// Parses `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// The machine's local date.
pub fn local_today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_like_names() {
        assert!(date_like("due_date") && date_like("date") && date_like("date_display"));
        assert!(date_like("creation_timestamp") && date_like("completed_at") && date_like("last_updated"));
        assert!(!date_like("doc_number") && !date_like("time_display") && !date_like("period"));
    }
}
