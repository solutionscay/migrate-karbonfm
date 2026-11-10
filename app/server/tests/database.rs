//! The database foundation of I1: refinery migrations of schema 04, pragmas, the reference seed (OP-ADMIN-02), the
//! demo seed rule and the audit stamps (BR-PARTY-07, D-05).

use karbon_server::audit::{self, Actor, Stamp};
use karbon_server::db::{self, DbError, Seed};
use karbon_server::reference::{
    ActivityPartyRole, ActivityType, ContactMethodPurposeType, EntityType, OpportunityStatus, PartyRoleType,
};
use rusqlite::Connection;

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn fresh_database_gets_schema_04_and_the_reference_seed_only() {
    let (conn, report) = db::open_in_memory(Seed::Reference).unwrap();
    assert_eq!(
        report.migrations_applied,
        ["V1__schema_04", "V2__ddr_audit", "V3__sales_summary_view", "V4__integration_settings"]
    );
    assert_eq!(report.reference_rows_inserted, 44);
    assert!(!report.demo_seed_loaded);
    // The 57 tables of V1 and the 2 import lineage tables of V2, plus refinery's history.
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name <> 'refinery_schema_history'"),
        59
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM refinery_schema_history"), 4);
    // V4 (I6): the integration switches start off (AP-12).
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM system_setting WHERE address_suggestion_provider IS NOT NULL \
             OR COALESCE(fullcontact_enabled_ind, 0) <> 0"
        ),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM value_list_item"), 24);
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view' AND name = 'sales_summary_by_month'"),
        1
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM party"), 0);
    assert_eq!(count(&conn, "PRAGMA foreign_keys"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM app_meta WHERE key = 'reference_seed'"), 1);
    assert!(db::foreign_key_check(&conn).unwrap().is_empty());
}

#[test]
fn reference_seed_has_the_source_ids_and_names_with_stamps() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    let name = |sql: &str, id: i64| -> String { conn.query_row(sql, [id], |r| r.get(0)).unwrap() };
    for v in ActivityType::ALL {
        assert_eq!(name("SELECT description FROM activity_type WHERE id = ?1", v.id()), v.name());
    }
    for v in ActivityPartyRole::ALL {
        assert_eq!(name("SELECT name FROM activity_party_role WHERE id = ?1", v.id()), v.name());
    }
    for v in OpportunityStatus::ALL {
        assert_eq!(name("SELECT name FROM opportunity_status WHERE id = ?1", v.id()), v.name());
        let active: i64 =
            conn.query_row("SELECT active_ind FROM opportunity_status WHERE id = ?1", [v.id()], |r| r.get(0)).unwrap();
        assert_eq!(active == 1, v.is_active());
    }
    for v in EntityType::ALL {
        assert_eq!(name("SELECT name FROM entity_type WHERE id = ?1", v.id()), v.name());
    }
    for v in PartyRoleType::ALL {
        assert_eq!(name("SELECT name FROM party_role_type WHERE id = ?1", v.id()), v.name());
    }
    for v in ContactMethodPurposeType::ALL {
        assert_eq!(name("SELECT name FROM contact_method_purpose_type WHERE id = ?1", v.id()), v.name());
    }
    // Stamps where the table has them: the app itself, before setup has a user.
    let (account, created, modified): (String, String, String) = conn
        .query_row(
            "SELECT creation_account_name, creation_timestamp, modification_timestamp FROM activity_type WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(account, audit::SYSTEM_ACCOUNT);
    assert!(audit::is_timestamp(&created), "{created}");
    assert_eq!(created, modified);
}

#[test]
fn demo_seed_reference_rows_equal_the_rust_enums() {
    // The demo seed run on its own (as `sqlite3 demo.db < 04_demo_seed.sql`) inserts the same rows as OP-ADMIN-02;
    // in the app they are INSERT OR IGNORE after the reference seed, so a difference would go unnoticed without this.
    let conn = Connection::open_in_memory().unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migration");
    conn.execute_batch(&std::fs::read_to_string(dir.join("04_database_schema.sql")).unwrap()).unwrap();
    conn.execute_batch(&std::fs::read_to_string(dir.join("04_demo_seed.sql")).unwrap()).unwrap();
    let pairs = |sql: &str| -> Vec<(i64, String)> {
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    };
    let expect = |all: Vec<(i64, &str)>| all.into_iter().map(|(i, n)| (i, n.to_string())).collect::<Vec<_>>();
    assert_eq!(
        pairs("SELECT id, description FROM activity_type ORDER BY id"),
        expect(ActivityType::ALL.iter().map(|v| (v.id(), v.name())).collect())
    );
    assert_eq!(
        pairs("SELECT id, name FROM activity_party_role ORDER BY id"),
        expect(ActivityPartyRole::ALL.iter().map(|v| (v.id(), v.name())).collect())
    );
    assert_eq!(
        pairs("SELECT id, name FROM opportunity_status ORDER BY id"),
        expect(OpportunityStatus::ALL.iter().map(|v| (v.id(), v.name())).collect())
    );
    assert_eq!(
        pairs("SELECT id, name FROM entity_type ORDER BY id"),
        expect(EntityType::ALL.iter().map(|v| (v.id(), v.name())).collect())
    );
    assert_eq!(
        pairs("SELECT id, name FROM party_role_type ORDER BY id"),
        expect(PartyRoleType::ALL.iter().map(|v| (v.id(), v.name())).collect())
    );
    assert_eq!(
        pairs("SELECT id, name FROM contact_method_purpose_type ORDER BY id"),
        expect(ContactMethodPurposeType::ALL.iter().map(|v| (v.id(), v.name())).collect())
    );
    for v in OpportunityStatus::ALL {
        let a: i64 =
            conn.query_row("SELECT active_ind FROM opportunity_status WHERE id = ?1", [v.id()], |r| r.get(0)).unwrap();
        assert_eq!(a == 1, v.is_active(), "{v:?}");
    }
}

#[test]
fn file_database_uses_wal_and_seeds_the_demo_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = db::database_path(&dir.path().join("app-data"));
    let (conn, report) = db::open(&path, Seed::Demo).unwrap();
    assert!(path.exists());
    assert_eq!(
        report.migrations_applied,
        ["V1__schema_04", "V2__ddr_audit", "V3__sales_summary_view", "V4__integration_settings"]
    );
    assert_eq!(report.reference_rows_inserted, 44);
    assert!(report.demo_seed_loaded);
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    assert_eq!(count(&conn, "PRAGMA foreign_keys"), 1);
    assert_eq!(count(&conn, "PRAGMA synchronous"), 1, "NORMAL");
    assert!(db::foreign_key_check(&conn).unwrap().is_empty());
    let parties = count(&conn, "SELECT COUNT(*) FROM party");
    assert!(parties > 14);
    // Source ids kept: the seed's own keys and the reference ids.
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM sales_transaction WHERE id = 't-inv-1048' AND doc_number = 'INV-1048'"),
        1
    );
    drop(conn);

    let (conn, report) = db::open(&path, Seed::Demo).unwrap();
    assert!(report.migrations_applied.is_empty());
    assert_eq!(report.reference_rows_inserted, 0);
    assert!(!report.demo_seed_loaded);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM party"), parties);
}

#[test]
fn demo_seed_never_loads_into_a_database_with_a_party() {
    let dir = tempfile::tempdir().unwrap();
    let path = db::database_path(dir.path());
    let (conn, _) = db::open(&path, Seed::Reference).unwrap();
    conn.execute("INSERT INTO party (id, name_cached, organization_ind) VALUES ('p-own', 'My Company', 1)", [])
        .unwrap();
    drop(conn);
    let (conn, report) = db::open(&path, Seed::Demo).unwrap();
    assert!(!report.demo_seed_loaded);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM party"), 1);
}

#[test]
fn demo_seed_loads_into_a_reference_only_database() {
    // A database first opened by a non-demo build (reference rows, no party yet) still takes the demo seed.
    let dir = tempfile::tempdir().unwrap();
    let path = db::database_path(dir.path());
    drop(db::open(&path, Seed::Reference).unwrap());
    let (conn, report) = db::open(&path, Seed::Demo).unwrap();
    assert!(report.demo_seed_loaded);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM activity_type"), 10);
    assert!(db::foreign_key_check(&conn).unwrap().is_empty());
}

#[test]
fn foreign_keys_are_enforced() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let err = conn
        .execute("INSERT INTO sales_line (id, sales_transaction_id, amount) VALUES ('sl-x', 't-missing', 100)", [])
        .unwrap_err();
    assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
}

#[test]
fn an_edited_applied_migration_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = db::database_path(dir.path());
    let (conn, _) = db::open(&path, Seed::Reference).unwrap();
    conn.execute("UPDATE refinery_schema_history SET checksum = '1' WHERE version = 1", []).unwrap();
    drop(conn);
    match db::open(&path, Seed::Reference) {
        Err(DbError::Migration(e)) => {
            assert!(e.to_string().contains("V1__schema_04 is different"), "{e}")
        }
        other => panic!("expected a divergent migration error, got {other:?}"),
    }
}

#[test]
fn an_s01_database_without_migration_history_is_refused() {
    // S-01 built karbonfm-demo.db with execute_batch and no history. The app now uses karbonfm.db, so the old file is
    // never opened; if one is, refinery refuses to re-create the tables and the file is left as it was.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let conn = Connection::open(&path).unwrap();
    let schema = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migration/04_database_schema.sql"),
    )
    .unwrap();
    conn.execute_batch(&schema).unwrap();
    drop(conn);
    assert!(matches!(db::open(&path, Seed::Demo), Err(DbError::Migration(_))));
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM party"), 0);
}

#[test]
fn timestamps_of_the_demo_database_are_local_civil_time() {
    // D-05: every *_timestamp value is `YYYY-MM-DD HH:MM:SS`, no offset.
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT m.name, c.name FROM sqlite_master m JOIN pragma_table_info(m.name) c
             WHERE m.type = 'table' AND c.name LIKE '%\\_timestamp' ESCAPE '\\'",
        )
        .unwrap();
    let cols: Vec<(String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert!(cols.len() > 50, "{}", cols.len());
    let mut checked = 0;
    for (table, col) in cols {
        let mut stmt = conn.prepare(&format!("SELECT {col} FROM \"{table}\" WHERE {col} IS NOT NULL")).unwrap();
        for v in stmt.query_map([], |r| r.get::<_, String>(0)).unwrap() {
            let v = v.unwrap();
            assert!(audit::is_timestamp(&v), "{table}.{col} = {v}");
            checked += 1;
        }
    }
    assert!(checked > 40, "{checked}");
}

// ---------------------------------------------------------------------------------------------------------------
// Audit stamps (BR-PARTY-07)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn stamp_created_sets_creation_and_modification() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    conn.execute("INSERT INTO party (id, name_cached) VALUES ('p-1', 'Ada')", []).unwrap();
    let mara = Actor::user("u-mara", "mara");
    let n = audit::stamp_created(&conn, "party", &"p-1", &Stamp::at(&mara, "2026-10-31 09:15:00")).unwrap();
    assert!(n >= 4, "{n}");
    let row: (String, String, String, String, String, String) = conn
        .query_row(
            "SELECT creation_user_id, creation_account_name, creation_timestamp,
                    modification_user_id, modification_account_name, modification_timestamp FROM party WHERE id = 'p-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "u-mara".into(),
            "mara".into(),
            "2026-10-31 09:15:00".into(),
            "u-mara".into(),
            "mara".into(),
            "2026-10-31 09:15:00".into()
        )
    );
}

#[test]
fn stamp_modified_keeps_the_creation_stamps() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    conn.execute("INSERT INTO party (id, name_cached) VALUES ('p-1', 'Ada')", []).unwrap();
    audit::stamp_created(&conn, "party", &"p-1", &Stamp::at(&Actor::system(), "2026-10-30 08:00:00")).unwrap();
    audit::stamp_modified(&conn, "party", &"p-1", &Stamp::at(&Actor::user("u-mara", "mara"), "2026-10-31 10:00:00"))
        .unwrap();
    let (cu, ct, mu, mt): (Option<String>, String, String, String) = conn
        .query_row(
            "SELECT creation_user_id, creation_timestamp, modification_user_id, modification_timestamp FROM party WHERE id = 'p-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((cu, ct.as_str()), (None, "2026-10-30 08:00:00"));
    assert_eq!((mu.as_str(), mt.as_str()), ("u-mara", "2026-10-31 10:00:00"));
}

#[test]
fn stamps_only_the_columns_a_table_has() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    let stamp = Stamp::at(&Actor::user("u-mara", "mara"), "2026-10-31 10:00:00");
    // contact_method_purpose_type has only the user ids; party_role_type has no stamp column.
    conn.execute("INSERT INTO contact_method_purpose_type (id, name) VALUES (9, 'Other')", []).unwrap();
    assert_eq!(audit::stamp_created(&conn, "contact_method_purpose_type", &9, &stamp).unwrap(), 2);
    conn.execute("INSERT INTO party_role_type (id, name) VALUES (9, 'Other')", []).unwrap();
    assert_eq!(audit::stamp_created(&conn, "party_role_type", &9, &stamp).unwrap(), 0);
}

#[test]
fn stamp_refuses_an_unknown_table_or_row() {
    let (conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    let stamp = Stamp::now(&Actor::system());
    assert!(audit::stamp_created(&conn, "party; DROP TABLE party", &"x", &stamp).is_err());
    assert!(audit::stamp_created(&conn, "party", &"no-such-id", &stamp).is_err());
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE name = 'party'"), 1);
}

#[test]
fn stamps_roll_back_with_the_command() {
    // D-08: a command and its stamps are one transaction.
    let (mut conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    {
        let tx = conn.transaction().unwrap();
        tx.execute("INSERT INTO party (id, name_cached) VALUES ('p-1', 'Ada')", []).unwrap();
        audit::stamp_created(&tx, "party", &"p-1", &Stamp::now(&Actor::system())).unwrap();
        // dropped without commit
    }
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM party"), 0);
}

/// BR-PARTY-03 and BR-PARTY-05 on the data the app starts with: every relation of the demo seed and of first-run
/// setup is a pair of rows, and every customer role of the demo seed has its customer row.
#[test]
fn demo_seed_and_setup_write_relation_pairs_and_customer_rows() {
    let (conn, _) = db::open_in_memory(Seed::Demo).unwrap();
    assert_eq!(karbon_server::parties::relations_without_reciprocal(&conn).unwrap(), Vec::<String>::new());
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM party_relation"), 30);
    assert_eq!(karbon_server::parties::customer_roles_without_customer_row(&conn).unwrap(), Vec::<String>::new());

    let (mut conn, _) = db::open_in_memory(Seed::Reference).unwrap();
    let input = karbon_server::admin::SetupInput {
        organization_name: "Harbor & Pine Design".into(),
        first_name: "Rosa".into(),
        last_name: "Lindqvist".into(),
        country_code: "1".into(),
        account_name: "rosa".into(),
        ..Default::default()
    };
    karbon_server::admin::setup_welcome(&mut conn, &input).unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM party_relation"), 2);
    assert_eq!(karbon_server::parties::relations_without_reciprocal(&conn).unwrap(), Vec::<String>::new());
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM party_relation WHERE relationship = 'Employer' AND creation_user_id IS NOT NULL"
        ),
        1
    );
}
