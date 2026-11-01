//! The DDR schema audit (migration/10_ddr_schema_audit.md) as a test: every field of every DDR table has one row in
//! migration/10_ddr_schema_audit.tsv, every migrated row names a column the migrated database has (type, NOT NULL,
//! foreign key), every other row names the decision that drops it, the seeded value lists equal the DDR, and schema 04
//! builds the same database as the refinery migrations.

use karbon_server::db::{self, Seed};
use regex::Regex;
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn migration_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migration")
}

/// One row of 10_ddr_schema_audit.tsv.
#[derive(Debug, Clone)]
struct Row {
    file: String,
    table_id: u32,
    table: String,
    field_id: u32,
    field: String,
    data_type: String,
    field_type: String,
    repetitions: u32,
    global: bool,
    status: String,
    target: String,
    target_type: String,
    target_notnull: String,
    target_fk: String,
    decision: String,
}

fn register() -> Vec<Row> {
    let text = std::fs::read_to_string(migration_dir().join("10_ddr_schema_audit.tsv")).unwrap();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
    assert_eq!(header.len(), 16, "{header:?}");
    lines
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 16, "{l}");
            Row {
                file: c[0].into(),
                table_id: c[1].parse().unwrap(),
                table: c[2].into(),
                field_id: c[3].parse().unwrap(),
                field: c[4].into(),
                data_type: c[5].into(),
                field_type: c[6].into(),
                repetitions: c[7].parse().unwrap(),
                global: c[8] == "1",
                status: c[10].into(),
                target: c[11].into(),
                target_type: c[12].into(),
                target_notnull: c[13].into(),
                target_fk: c[14].into(),
                decision: c[15].into(),
            }
        })
        .collect()
}

/// A field as the DDR object files describe it: (file, table id, table, field id, field, type, kind, reps, global).
type DdrField = (String, u32, String, u32, String, String, String, u32, bool);

fn ddr_fields() -> Vec<DdrField> {
    let header = Regex::new(r"^# table (\d+): (.*)$").unwrap();
    let field = Regex::new(r"^Field id=(\d+) dataType=(\w+) fieldType=(\w+) name=(.*)$").unwrap();
    let reps = Regex::new(r"maxRepetition=(\d+)").unwrap();
    let mut out = Vec::new();
    let objects = migration_dir().join("objects");
    let mut files: Vec<_> = std::fs::read_dir(&objects).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    for dir in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "fmp12")) {
        let file = dir.file_name().unwrap().to_string_lossy().to_string();
        let Ok(entries) = std::fs::read_dir(dir.join("tables")) else { continue };
        for path in entries.map(|e| e.unwrap().path()) {
            let text = std::fs::read_to_string(&path).unwrap();
            let mut lines = text.lines();
            let h = header.captures(lines.next().unwrap()).unwrap_or_else(|| panic!("{}", path.display()));
            let (tid, tname): (u32, String) = (h[1].parse().unwrap(), h[2].to_string());
            let mut current: Option<DdrField> = None;
            for line in lines {
                if let Some(c) = field.captures(line) {
                    out.extend(current.take());
                    let name = c[4].trim_matches('"').to_string();
                    current = Some((
                        file.clone(),
                        tid,
                        tname.clone(),
                        c[1].parse().unwrap(),
                        name,
                        c[2].into(),
                        c[3].into(),
                        1,
                        false,
                    ));
                } else if let (Some(f), true) = (current.as_mut(), line.trim_start().starts_with("Storage ")) {
                    f.7 = reps.captures(line).map_or(1, |c| c[1].parse().unwrap());
                    f.8 = line.contains("global=True");
                }
            }
            out.extend(current);
        }
    }
    out
}

/// (type, notnull, fk "table.column" or "") of every column of the database.
fn columns(conn: &Connection) -> BTreeMap<String, (String, String, String)> {
    let mut out = BTreeMap::new();
    for t in tables(conn) {
        let mut fks = BTreeMap::new();
        let mut stmt = conn.prepare(&format!("PRAGMA foreign_key_list('{t}')")).unwrap();
        for r in stmt
            .query_map([], |r| Ok((r.get::<_, String>(3)?, r.get::<_, String>(2)?, r.get::<_, String>(4)?)))
            .unwrap()
        {
            let (from, to_table, to_col) = r.unwrap();
            fks.insert(from, format!("{to_table}.{to_col}"));
        }
        let mut stmt = conn.prepare(&format!("PRAGMA table_info('{t}')")).unwrap();
        for r in
            stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))).unwrap()
        {
            let (name, ty, notnull) = r.unwrap();
            let fk = fks.get(&name).cloned().unwrap_or_default();
            out.insert(format!("{t}.{name}"), (ty, notnull.to_string(), fk));
        }
    }
    out
}

fn tables(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT IN ('refinery_schema_history') \
             AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

/// Columns the app adds that have no FileMaker field (04 conventions and OD-07-06; the import lineage of D-40).
const APP_ONLY_TABLES: &[&str] =
    &["app_meta", "import_batch", "import_rejected_row", "import_file", "import_changed_value", "value_list_item"];
const APP_ONLY_COLUMNS: &[&str] =
    &["system_setting.singleton", "system_setting.tax_rate_bp", "sales_transaction.tax_rate_bp"];

#[test]
fn every_ddr_field_has_exactly_one_register_row() {
    let ddr: BTreeSet<DdrField> = ddr_fields().into_iter().collect();
    let rows = register();
    let reg: BTreeSet<DdrField> = rows
        .iter()
        .map(|r| {
            (
                r.file.clone(),
                r.table_id,
                r.table.clone(),
                r.field_id,
                r.field.clone(),
                r.data_type.clone(),
                r.field_type.clone(),
                r.repetitions,
                r.global,
            )
        })
        .collect();
    assert_eq!(reg.len(), rows.len(), "duplicate register rows");
    let missing: Vec<_> = ddr.difference(&reg).collect();
    let extra: Vec<_> = reg.difference(&ddr).collect();
    assert!(missing.is_empty() && extra.is_empty(), "not in the register: {missing:#?}\nnot in the DDR: {extra:#?}");
    // The audit's counts (10_ddr_schema_audit.md, Summary).
    assert_eq!(ddr.len(), 3071);
    // 195 tables; karbon_data Globals has no field, so 194 tables have rows.
    assert_eq!(ddr.iter().map(|f| (&f.0, f.1)).collect::<BTreeSet<_>>().len(), 194);
}

#[test]
fn every_migrated_field_names_a_column_of_the_migrated_database() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    let cols = columns(&conn);
    let mut checked = 0;
    for r in register().iter().filter(|r| r.status == "column") {
        let (ty, notnull, fk) =
            cols.get(&r.target).unwrap_or_else(|| panic!("{}::{} -> {}: no such column", r.table, r.field, r.target));
        assert_eq!(
            (ty.as_str(), notnull.as_str(), fk.as_str()),
            (r.target_type.as_str(), r.target_notnull.as_str(), r.target_fk.as_str()),
            "{}::{} -> {}",
            r.table,
            r.field,
            r.target
        );
        // The FileMaker type and the column type agree (04 conventions): text, number, dates as ISO text, containers as
        // a path; money is INTEGER cents, and vendor.credit_limit is Text in the source (BR-IMPORT-01).
        let ok = match r.data_type.as_str() {
            "Text" => ty == "TEXT" || (ty == "NUMERIC" && r.field.ends_with("Id")) || r.target == "vendor.credit_limit",
            "Number" => ty == "NUMERIC" || ty == "INTEGER",
            "Date" | "Time" | "TimeStamp" | "Binary" => ty == "TEXT",
            other => panic!("unknown FileMaker type {other}"),
        };
        assert!(ok, "{}::{} ({}) -> {} {ty}", r.table, r.field, r.data_type, r.target);
        assert_eq!(r.file, "karbon_data.fmp12", "only karbon_data holds stored data: {r:?}");
        assert!(!r.global, "a global has no column: {r:?}");
        checked += 1;
    }
    assert_eq!(checked, 689);
}

#[test]
fn every_column_has_a_source_field_or_is_app_only() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    let targets: BTreeSet<String> = register().into_iter().filter(|r| r.status == "column").map(|r| r.target).collect();
    let orphans: Vec<String> = columns(&conn)
        .into_keys()
        .filter(|c| !targets.contains(c))
        .filter(|c| !APP_ONLY_TABLES.contains(&c.split('.').next().unwrap()))
        .filter(|c| !APP_ONLY_COLUMNS.contains(&c.as_str()))
        .collect();
    assert!(orphans.is_empty(), "columns with no source field: {orphans:?}");
}

#[test]
fn every_field_without_a_column_names_its_decision() {
    let id = Regex::new(r"\b(OD-\d+-\d+|D-\d+|BR-[A-Z]+-\d+)\b").unwrap();
    for r in register().iter().filter(|r| r.status != "column") {
        assert!(matches!(r.status.as_str(), "dropped" | "computed"), "{r:?}");
        assert!(id.is_match(&r.decision), "no decision id: {r:?}");
        if r.status == "computed" {
            assert!(r.decision.contains("D-39"), "{r:?}");
        }
        assert!(r.target_type.is_empty() && r.target_fk.is_empty(), "{r:?}");
    }
    // Repeating fields would become child rows (OD-04-03); every one is in a dropped table.
    for r in register().iter().filter(|r| r.repetitions > 1) {
        assert_eq!(r.status, "dropped", "{r:?}");
    }
}

/// The custom values of each DDR value list, by name.
fn ddr_value_lists() -> BTreeMap<String, Vec<String>> {
    let name = Regex::new(r#"^ValueList id=\d+ name=(.*)$"#).unwrap();
    let mut out = BTreeMap::new();
    for dir in std::fs::read_dir(migration_dir().join("objects")).unwrap().map(|e| e.unwrap().path()) {
        let Ok(entries) = std::fs::read_dir(dir.join("value-lists")) else { continue };
        for path in entries.map(|e| e.unwrap().path()) {
            let text = std::fs::read_to_string(&path).unwrap();
            let Some(n) = text.lines().find_map(|l| name.captures(l).map(|c| c[1].trim_matches('"').to_string()))
            else {
                continue;
            };
            let mut values = Vec::new();
            let mut in_custom = false;
            for line in text.lines() {
                match line.trim() {
                    "CustomValues" => in_custom = true,
                    "Text" => {}
                    t if in_custom && t.starts_with("| ") => values.push(t[2..].to_string()),
                    _ if in_custom && !line.starts_with("    ") => in_custom = false,
                    _ => {}
                }
            }
            out.entry(n).or_insert(values);
        }
    }
    out
}

#[test]
fn seeded_value_lists_equal_the_ddr() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    let ddr = ddr_value_lists();
    let mut stmt = conn.prepare("SELECT list, value FROM value_list_item ORDER BY list, sort_order").unwrap();
    let mut seeded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap() {
        let (list, value) = r.unwrap();
        seeded.entry(list).or_default().push(value);
    }
    assert_eq!(seeded.len(), 9, "{seeded:?}");
    for (list, values) in &seeded {
        assert_eq!(Some(values), ddr.get(list), "{list}");
    }
    // Lists the audit leaves out: no target column stores their values (10_ddr_schema_audit.md, GAP-01).
    for skipped in ["TwilioFormatOptions", "AddressServiceProvider", "Code Location", "Task Status"] {
        assert!(ddr.contains_key(skipped) && !seeded.contains_key(skipped), "{skipped}");
    }
}

/// Everything about a database's schema that matters, without the comments sqlite_master keeps in CREATE TABLE.
fn shape(conn: &Connection) -> Vec<String> {
    let mut out: Vec<String> = columns(conn).into_iter().map(|(k, v)| format!("column {k} {v:?}")).collect();
    for t in tables(conn) {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info('{t}')")).unwrap();
        let pk: Vec<String> = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(5)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .filter(|(k, _)| *k > 0)
            .map(|(k, n)| format!("{k}:{n}"))
            .collect();
        out.push(format!("pk {t} {pk:?}"));
        let mut stmt = conn.prepare(&format!("PRAGMA index_list('{t}')")).unwrap();
        let idx: Vec<(String, i64)> =
            stmt.query_map([], |r| Ok((r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        for (name, unique) in idx {
            let mut stmt = conn.prepare(&format!("PRAGMA index_info('{name}')")).unwrap();
            let cols: Vec<String> = stmt.query_map([], |r| r.get(2)).unwrap().map(Result::unwrap).collect();
            let name = if name.starts_with("sqlite_autoindex") { "auto".to_string() } else { name };
            out.push(format!("index {t} {name} unique={unique} {cols:?}"));
        }
    }
    let mut stmt = conn.prepare("SELECT name, sql FROM sqlite_master WHERE type IN ('view', 'trigger')").unwrap();
    out.extend(
        stmt.query_map([], |r| Ok(format!("{}: {}", r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(Result::unwrap),
    );
    let mut stmt = conn.prepare("SELECT list, value, sort_order FROM value_list_item").unwrap();
    out.extend(
        stmt.query_map([], |r| {
            Ok(format!("vl {}|{}|{}", r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .unwrap()
        .map(Result::unwrap),
    );
    out.sort();
    out
}

#[test]
fn schema_04_equals_the_migrations() {
    let from_04 = Connection::open_in_memory().unwrap();
    from_04.execute_batch(&std::fs::read_to_string(migration_dir().join("04_database_schema.sql")).unwrap()).unwrap();
    let mut migrated = Connection::open_in_memory().unwrap();
    migrated.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    db::migrate(&mut migrated).unwrap();
    let (a, b) = (shape(&from_04), shape(&migrated));
    let only_04: Vec<_> = a.iter().filter(|x| !b.contains(x)).collect();
    let only_migrations: Vec<_> = b.iter().filter(|x| !a.contains(x)).collect();
    assert!(
        only_04.is_empty() && only_migrations.is_empty(),
        "only in 04: {only_04:#?}\nonly in the migrations: {only_migrations:#?}"
    );
    assert_eq!(tables(&migrated).len(), 59);
}

#[test]
fn v1_is_the_first_schema_04_and_v2_is_inside_04() {
    // V1 must stay byte for byte as applied (refinery checks it); V2's statements appear in 04 word for word.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let v1 = std::fs::read_to_string(root.join("migrations/V1__schema_04.sql")).unwrap();
    assert_eq!(v1.matches("CREATE TABLE").count(), 57);
    let v2 = std::fs::read_to_string(root.join("migrations/V2__ddr_audit.sql")).unwrap();
    let schema = std::fs::read_to_string(migration_dir().join("04_database_schema.sql")).unwrap();
    let body: String = v2.lines().skip(3).collect::<Vec<_>>().join("\n");
    assert!(schema.contains(body.trim()), "04 does not hold V2 word for word");
}
