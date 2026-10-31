//! Reference tables with fixed ids (BR-ADMIN-02, D-19) and their seed on a fresh database (OP-ADMIN-02
//! `reference_seed`). Each seeded table is a Rust enum with the source ids; commands use the enum, never a literal.
//!
//! Sources: kontroller 660 (activity_type), 710 (activity_party_role), 607 (opportunity_status, its missing comma
//! before id 8 fixed), 732 (entity_type); the ids the scripts hard-code for party_role_type (705, 610, 595) and
//! contact_method_purpose_type (185). The other rows of those two tables come from the user's export (D-19).

use crate::audit::{self, Actor, Stamp};
use rusqlite::{params, Connection};

/// Defines a reference enum with its ids and names, and `ALL` in id order.
macro_rules! reference_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $id:literal, $label:literal;)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(i64)]
        pub enum $name { $($variant = $id),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub const fn id(self) -> i64 {
                self as i64
            }

            pub const fn name(self) -> &'static str {
                match self { $($name::$variant => $label),+ }
            }

            pub fn from_id(id: i64) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.id() == id)
            }
        }
    };
}

reference_enum! {
    /// activity_type (BR-ACTIVITY-01; kontroller 660). Column `description`.
    ActivityType {
        Email = 1, "Email";
        Note = 2, "Note";
        Task = 3, "Task";
        Event = 4, "Event";
        Comment = 5, "Comment";
        Meeting = 6, "Meeting";
        Letter = 7, "Letter";
        Call = 8, "Call";
        Sms = 9, "SMS";
        Discussion = 10, "Discussion";
    }
}

reference_enum! {
    /// activity_party_role (BR-ACTIVITY-02; kontroller 710).
    ActivityPartyRole {
        Creator = 50, "Creator";
        CompletedBy = 51, "Completed By";
        AssignedBy = 52, "Assigned By";
        AssignedTo = 53, "Assigned To";
        From = 54, "From";
        To = 55, "To";
        Cc = 56, "Cc";
        Bcc = 57, "Bcc";
        NotificationFrom = 58, "Notification From";
        NotificationTo = 59, "Notification To";
        Invited = 60, "Invited";
        Unused = 61, "Unused";
        Organizer = 62, "Organizer";
        Regarding = 63, "Regarding";
        Subscriber = 64, "Subscriber";
        SpokenWith = 65, "Spoken With";
    }
}

reference_enum! {
    /// opportunity_status (kontroller 607). `is_active` is the active_ind column.
    OpportunityStatus {
        New = 1, "New";
        InProcess = 2, "In process";
        ProposalSent = 3, "Proposal sent";
        Won = 4, "Won";
        Lost = 5, "Lost";
        Unqualified = 6, "Unqualified";
        ClosedOther = 7, "Closed (other)";
        OnHold = 8, "On Hold";
    }
}

impl OpportunityStatus {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::New | Self::InProcess | Self::ProposalSent | Self::OnHold)
    }
}

reference_enum! {
    /// entity_type (kontroller 732).
    EntityType {
        Party = 10, "Party";
        Activity = 11, "Activity";
        SalesTransaction = 12, "Sales Transaction";
        Opportunity = 13, "Opportunity";
        ChatMessage = 14, "Chat Message";
    }
}

reference_enum! {
    /// party_role_type: only the ids the scripts hard-code (kontroller 705, 610, 595).
    PartyRoleType {
        Customer = 2, "Customer";
        Lead = 4, "Lead";
    }
}

reference_enum! {
    /// contact_method_purpose_type: only the ids the scripts hard-code (kontroller 185).
    ContactMethodPurposeType {
        Billing = 1, "Billing";
        Shipping = 2, "Shipping";
        Mailing = 3, "Mailing";
    }
}

/// The app_meta key that marks a database whose reference tables were seeded.
pub const SEEDED_KEY: &str = "reference_seed";

/// One reference row: table, id, name column, name, and active_ind where the table has it.
struct Row {
    table: &'static str,
    name_col: &'static str,
    id: i64,
    name: &'static str,
    active: Option<bool>,
}

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let simple = |table, name_col, id, name| Row { table, name_col, id, name, active: None };
    rows.extend(ActivityType::ALL.iter().map(|v| simple("activity_type", "description", v.id(), v.name())));
    rows.extend(ActivityPartyRole::ALL.iter().map(|v| simple("activity_party_role", "name", v.id(), v.name())));
    rows.extend(OpportunityStatus::ALL.iter().map(|v| Row {
        table: "opportunity_status",
        name_col: "name",
        id: v.id(),
        name: v.name(),
        active: Some(v.is_active()),
    }));
    rows.extend(EntityType::ALL.iter().map(|v| simple("entity_type", "name", v.id(), v.name())));
    rows.extend(PartyRoleType::ALL.iter().map(|v| simple("party_role_type", "name", v.id(), v.name())));
    rows.extend(
        ContactMethodPurposeType::ALL.iter().map(|v| simple("contact_method_purpose_type", "name", v.id(), v.name())),
    );
    rows
}

/// OP-ADMIN-02 `reference_seed`: inserts the reference rows on a database that was never seeded, in one transaction,
/// with creation stamps (BR-PARTY-07) where the table has them. A row whose id exists is left as it is (an import
/// keeps the user's own reference rows). Returns the number of rows inserted; 0 when the database was seeded before.
pub fn reference_seed(conn: &mut Connection, actor: &Actor) -> rusqlite::Result<usize> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let seeded: bool =
        tx.query_row("SELECT EXISTS (SELECT 1 FROM app_meta WHERE key = ?1)", [SEEDED_KEY], |r| r.get(0))?;
    if seeded {
        return Ok(0);
    }
    let stamp = Stamp::now(actor);
    let mut inserted = 0;
    for row in rows() {
        let n = match row.active {
            Some(active) => tx.execute(
                &format!(
                    "INSERT INTO {} (id, {}, active_ind) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
                    row.table, row.name_col
                ),
                params![row.id, row.name, i64::from(active)],
            )?,
            None => tx.execute(
                &format!("INSERT INTO {} (id, {}) VALUES (?1, ?2) ON CONFLICT DO NOTHING", row.table, row.name_col),
                params![row.id, row.name],
            )?,
        };
        if n == 1 {
            audit::stamp_created(&tx, row.table, &row.id, &stamp)?;
            inserted += 1;
        }
    }
    tx.execute("INSERT INTO app_meta (key, value) VALUES (?1, ?2)", params![SEEDED_KEY, stamp.at])?;
    tx.commit()?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_are_unique() {
        for v in ActivityType::ALL {
            assert_eq!(ActivityType::from_id(v.id()), Some(*v));
        }
        assert_eq!(ActivityType::ALL.len(), 10);
        assert_eq!(ActivityPartyRole::ALL.len(), 16);
        assert_eq!(OpportunityStatus::ALL.len(), 8);
        assert_eq!(ActivityPartyRole::Creator.id(), 50);
        assert_eq!(ActivityPartyRole::SpokenWith.id(), 65);
        assert_eq!(PartyRoleType::Lead.id(), 4);
        assert_eq!(ActivityType::from_id(11), None);
        let mut keys: Vec<_> = rows().iter().map(|r| (r.table, r.id)).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n);
        assert_eq!(n, 44, "03 lists 44 reference rows");
    }
}
