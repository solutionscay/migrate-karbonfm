//! Audit stamps (BR-PARTY-07): every durable row carries the creation and modification user id, account name and
//! timestamp of the single local user (BR-ADMIN-04). Timestamps are local civil time `YYYY-MM-DD HH:MM:SS`, no offset
//! (D-05). There is no separate audit trail (D-30).
//!
//! Commands write their row with plain SQL, then call [`stamp_created`] or [`stamp_modified`] in the same
//! transaction. The helpers set only the stamp columns the table has (schema 04 keeps the source's columns, so some
//! tables have all six, some only the user ids, some none).

use rusqlite::types::ToSql;
use rusqlite::Connection;

/// Who writes: the single local user (BR-ADMIN-04), or the app itself for seeds and migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: Option<String>,
    pub account_name: String,
}

impl Actor {
    /// The local user (`user.id`, `user.account_name`). OP-ADMIN-04 `current_user` builds it for each request.
    pub fn user(user_id: impl Into<String>, account_name: impl Into<String>) -> Self {
        Self { user_id: Some(user_id.into()), account_name: account_name.into() }
    }

    /// The app itself, before setup has created a user: reference seed and demo seed.
    pub fn system() -> Self {
        Self { user_id: None, account_name: SYSTEM_ACCOUNT.into() }
    }
}

/// Account name of rows the app writes without a user.
pub const SYSTEM_ACCOUNT: &str = "karbonfm";

/// One actor at one instant. A command takes one stamp and uses it for every row it writes, so the rows of one
/// operation carry the same time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub actor: Actor,
    /// Local civil time, `YYYY-MM-DD HH:MM:SS` (D-05).
    pub at: String,
}

impl Stamp {
    pub fn now(actor: &Actor) -> Self {
        Self { actor: actor.clone(), at: local_now() }
    }

    /// A fixed time, for tests and for the import (which keeps the source stamps, D-39).
    pub fn at(actor: &Actor, at: impl Into<String>) -> Self {
        Self { actor: actor.clone(), at: at.into() }
    }
}

/// The machine's local civil time, `YYYY-MM-DD HH:MM:SS`, as stored in every timestamp column (D-05).
pub fn local_now() -> String {
    chrono::Local::now().format(TIMESTAMP_FORMAT).to_string()
}

/// strftime format of the D-05 timestamp text.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// True when `s` is a D-05 timestamp: a valid local date and time `YYYY-MM-DD HH:MM:SS`, no offset.
pub fn is_timestamp(s: &str) -> bool {
    s.len() == 19 && chrono::NaiveDateTime::parse_from_str(s, TIMESTAMP_FORMAT).is_ok()
}

#[derive(Clone, Copy)]
enum Part {
    Creation,
    Modification,
}

/// Sets the creation and modification stamps of the new row `id` of `table` (FileMaker sets both on create).
/// Returns the number of stamp columns set (0 for a table without stamp columns).
pub fn stamp_created(conn: &Connection, table: &str, id: &dyn ToSql, stamp: &Stamp) -> rusqlite::Result<usize> {
    stamp_row(conn, table, id, stamp, &[Part::Creation, Part::Modification])
}

/// Sets the modification stamps of row `id` of `table`; the creation stamps stay.
pub fn stamp_modified(conn: &Connection, table: &str, id: &dyn ToSql, stamp: &Stamp) -> rusqlite::Result<usize> {
    stamp_row(conn, table, id, stamp, &[Part::Modification])
}

fn stamp_row(conn: &Connection, table: &str, id: &dyn ToSql, stamp: &Stamp, parts: &[Part]) -> rusqlite::Result<usize> {
    let columns = table_columns(conn, table)?;
    let mut sets: Vec<String> = Vec::new();
    let mut values: Vec<&dyn ToSql> = Vec::new();
    for part in parts {
        let prefix = match part {
            Part::Creation => "creation",
            Part::Modification => "modification",
        };
        let candidates: [(&str, &dyn ToSql); 3] =
            [("user_id", &stamp.actor.user_id), ("account_name", &stamp.actor.account_name), ("timestamp", &stamp.at)];
        for (suffix, value) in candidates {
            let col = format!("{prefix}_{suffix}");
            if columns.contains(&col) {
                values.push(value);
                sets.push(format!("{col} = ?{}", values.len()));
            }
        }
    }
    if sets.is_empty() {
        return Ok(0);
    }
    values.push(id);
    // `table` is safe to splice: table_columns found it in sqlite_master.
    let sql = format!("UPDATE \"{table}\" SET {} WHERE id = ?{}", sets.join(", "), values.len());
    let changed = conn.execute(&sql, values.as_slice())?;
    if changed != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(sets.len())
}

/// The columns of a table of the schema. An unknown table is an error, so no other name reaches the SQL.
fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let known: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |r| r.get(0),
    )?;
    if !known {
        return Err(rusqlite::Error::InvalidParameterName(format!("unknown table {table}")));
    }
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1)")?;
    let cols = stmt.query_map([table], |r| r.get(0))?.collect();
    cols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_now_is_a_d05_timestamp() {
        let now = local_now();
        assert!(is_timestamp(&now), "{now}");
        assert!(!now.contains('T') && !now.contains('Z') && !now.contains('+'));
        assert!(is_timestamp("2026-10-31 23:59:59"));
        assert!(!is_timestamp("2026-10-31T23:59:59"));
        assert!(!is_timestamp("2026-10-31 23:59:59+01:00"));
        assert!(!is_timestamp("2026-02-30 10:00:00"));
    }
}
