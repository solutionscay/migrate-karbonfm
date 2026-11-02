//! Notifications (BR-ACTIVITY-11): rows for a party, read by the user one at a time, with the unread count cached on
//! the user. Sources: kontroller 670 (create), kontacts 373 step 6 (the unread recount), layout 101 (the row button
//! that sets ReadInd). Script 445 (mark all of a discussion read) has no caller and is not built.

use crate::audit::{self, Stamp};
use rusqlite::{params, Connection};

/// Creates one notification for `party_id` (kontroller 670 s106-110): date today, the description, the entity it is
/// about. read_ind and sent_ind stay NULL, as in the source (nothing sets SentInd; 670 s114 is a comment only).
pub fn create(
    conn: &Connection,
    party_id: &str,
    description: &str,
    entity_id: &str,
    entity_type_id: i64,
    stamp: &Stamp,
) -> rusqlite::Result<String> {
    let id = crate::ids::new_id();
    conn.execute(
        "INSERT INTO notification (id, party_id, date, description, entity_id, entity_type_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, party_id, &stamp.at[..10], description, entity_id, entity_type_id],
    )?;
    audit::stamp_created(conn, "notification", &id, stamp)?;
    recount(conn, party_id)?;
    Ok(id)
}

/// The unread recount of kontacts 373 s6: notifications of the user's party whose read_ind is not 1. The source
/// recounts (on User Home load and on a timer); it never increments.
pub fn recount(conn: &Connection, party_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE user SET unread_notification_count_cached =
            (SELECT COUNT(*) FROM notification n WHERE n.party_id = user.party_id AND COALESCE(n.read_ind, 0) <> 1)
         WHERE party_id = ?1",
        [party_id],
    )?;
    Ok(())
}
