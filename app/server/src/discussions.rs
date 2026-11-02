//! Discussions, comments and notifications (05 OP-ACTIVITY-06, 07, 09; 07 BR-ACTIVITY-06, 07, 11). Sources:
//! kontroller 421 (create), 739 (edit), 422 (comment), 670 (notification), kontacts 313 / 844-846 / 861 (the
//! subscribers card), 373 (the unread recount), layout 101 (mark read). Plain SQL, one transaction per write.

use crate::activities::{self, PartyInput, PartyRow, Target};
use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::reference::{ActivityPartyRole, ActivityType, EntityType};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

fn field_error(field: &'static str, message: impl Into<String>) -> CommandError {
    CommandError::Validation(vec![FieldError { field, message: message.into() }])
}

/// What a discussion is about and who follows it (421 inputs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewDiscussion {
    pub entity_type_id: i64,
    pub entity_id: String,
    /// The first comment, when given (421 s157-169).
    pub notes: String,
    /// Subscriber (64) parties; exactly the ones passed, the creator is not added (421 s175-189).
    pub subscribers: Vec<String>,
}

/// BR-ACTIVITY-07 (421 s112-139): the title comes from the entity. A party: its name; an activity: its subject; a
/// sales transaction: "#<doc number> - <customer name>"; an opportunity: its party's name; any other type: empty.
/// An entity that does not exist is a field error (421 s104-107 for the type; the id must name a row).
pub fn title_for(conn: &Connection, entity_type_id: i64, entity_id: &str) -> Result<String, CommandError> {
    let missing = || field_error("entity_id", "The record this discussion is about does not exist.");
    let one = |sql: &str| -> Result<Option<String>, CommandError> {
        conn.query_row(sql, [entity_id], |r| r.get::<_, Option<String>>(0)).optional()?.ok_or_else(missing)
    };
    let title = match EntityType::from_id(entity_type_id) {
        Some(EntityType::Party) => one("SELECT name_cached FROM party WHERE id = ?1")?,
        Some(EntityType::Activity) => one("SELECT subject FROM activity WHERE id = ?1")?,
        Some(EntityType::SalesTransaction) => {
            one("SELECT '#' || COALESCE(st.doc_number, '') || ' - ' || COALESCE(p.name_cached, '')
             FROM sales_transaction st
             LEFT JOIN customer c ON c.id = st.id_customer
             LEFT JOIN party_role pr ON pr.id = c.id
             LEFT JOIN party p ON p.id = pr.party_id
             WHERE st.id = ?1")?
        }
        Some(EntityType::Opportunity) => {
            one("SELECT p.name_cached FROM opportunity o LEFT JOIN party p ON p.id = o.party_id WHERE o.id = ?1")?
        }
        _ => return Err(field_error("entity_type_id", "Not a valid entity type.")),
    };
    Ok(title.unwrap_or_default())
}

/// OP-ACTIVITY-06 `discussion_create` (421): the activity (type 10, discussion_ind), the discussion row (date and
/// time now, the title from the entity; active_ind left NULL as in the source, field 20 has no auto-enter), the
/// activity_entity link, the Creator (50) = the current user's party, one Subscriber (64) row per party passed, then
/// Notes as the first comment. 421 writes the comment before the activity exists and sends `CreatorId` where it reads
/// `CreatorPartyId` on two of its callers; the target does it in order, as intended.
pub fn discussion_create(conn: &mut Connection, new: &NewDiscussion, actor: &Actor) -> Result<String, CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let title = title_for(&tx, new.entity_type_id, &new.entity_id)?;
    let me = activities::current_party(&tx, actor)?.ok_or_else(|| {
        field_error("creator", "Your user is not linked to a person, so the discussion cannot have a creator.")
    })?;
    let id = crate::ids::new_id();
    let (date, time) = (&stamp.at[..10], &stamp.at[11..]);
    tx.execute(
        "INSERT INTO activity (id, activity_type_id, discussion_ind, subject, date_display, time_display, last_updated,
                               comment_count_cached)
         VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, 0)",
        params![id, ActivityType::Discussion.id(), title, date, time, stamp.at],
    )?;
    audit::stamp_created(&tx, "activity", &id, &stamp)?;
    tx.execute(
        "INSERT INTO discussion (id, title, date, time) VALUES (?1, ?2, ?3, ?4)",
        params![id, title, date, time],
    )?;
    audit::stamp_created(&tx, "discussion", &id, &stamp)?;
    let link = crate::ids::new_id();
    tx.execute(
        "INSERT INTO activity_entity (id, activity_id, entity_type_id, entity_id) VALUES (?1, ?2, ?3, ?4)",
        params![link, id, new.entity_type_id, new.entity_id],
    )?;
    audit::stamp_created(&tx, "activity_entity", &link, &stamp)?;
    let target = Target::of(&tx, &id)?;
    activities::add_party_in(&tx, &target, &PartyInput::new(ActivityPartyRole::Creator, &me), &stamp)?;
    for party in dedup(&new.subscribers) {
        activities::add_party_in(&tx, &target, &PartyInput::new(ActivityPartyRole::Subscriber, &party), &stamp)?;
    }
    activities::refresh_caches(&tx, &id)?;
    if !new.notes.trim().is_empty() {
        comment_in(&tx, &id, &new.notes, &me, &stamp)?;
    }
    tx.commit()?;
    Ok(id)
}

fn dedup(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_owned());
        }
    }
    out
}

fn discussion_target(conn: &Connection, id: &str) -> Result<Target, CommandError> {
    let is_discussion: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM discussion WHERE id = ?1)", [id], |r| r.get(0))?;
    if !is_discussion {
        return Err(CommandError::NotFound);
    }
    Target::of(conn, id)
}

/// The edit of OP-ACTIVITY-06 (739): `active` sets active_ind when given; `subscribers`, when given, replaces the
/// Subscriber (64) rows (the card saves the whole list, kontacts 861). The Creator and every other row stay: 739's
/// rebuild through 542 deleted them, and 739 never wrote active_ind (an unset `$Discussion`, s105-110); both built as
/// intended.
pub fn discussion_edit(
    conn: &mut Connection,
    id: &str,
    active: Option<bool>,
    subscribers: Option<&[String]>,
    actor: &Actor,
) -> Result<(), CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let target = discussion_target(&tx, id)?;
    if let Some(active) = active {
        tx.execute("UPDATE discussion SET active_ind = ?2 WHERE id = ?1", params![id, i64::from(active)])?;
        audit::stamp_modified(&tx, "discussion", &id, &stamp)?;
    }
    if let Some(list) = subscribers {
        let wanted = dedup(list);
        let current: Vec<(String, String)> = {
            let mut stmt = tx.prepare(
                "SELECT id, COALESCE(party_id, '') FROM activity_party
                 WHERE activity_id = ?1 AND CAST(activity_party_role_id AS INTEGER) = 64",
            )?;
            let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            rows
        };
        for (row, party) in &current {
            if !wanted.contains(party) {
                tx.execute("DELETE FROM activity_party WHERE id = ?1", [row])?;
            }
        }
        for party in wanted.iter().filter(|p| !current.iter().any(|(_, c)| c == *p)) {
            activities::add_party_in(&tx, &target, &PartyInput::new(ActivityPartyRole::Subscriber, party), &stamp)?;
        }
        activities::refresh_caches(&tx, id)?;
    }
    tx.execute("UPDATE activity SET last_updated = ?2 WHERE id = ?1", params![id, stamp.at])?;
    audit::stamp_modified(&tx, "activity", &id, &stamp)?;
    tx.commit()?;
    Ok(())
}

/// OP-ACTIVITY-07 `discussion_comment_add` (422): the comment by the current user's party, date and time now; the
/// comment count recounted (s116); a notification for each subscriber (BR-ACTIVITY-06).
pub fn comment_add(
    conn: &mut Connection,
    discussion_id: &str,
    comment: &str,
    actor: &Actor,
) -> Result<String, CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    discussion_target(&tx, discussion_id)?;
    let me = activities::current_party(&tx, actor)?
        .ok_or_else(|| field_error("comment", "Your user is not linked to a person, so it cannot comment."))?;
    let id = comment_in(&tx, discussion_id, comment, &me, &stamp)?;
    tx.commit()?;
    Ok(id)
}

/// The body of a comment add, inside the caller's transaction.
///
/// Notifications (422 s162-187, 670): one for each Subscriber (64) of the discussion (the UI path, kontacts 694,
/// passes the subscriber list), or, when it has none, one for each party on the activity in any role. The commenter
/// is not left out (the source does not). The text is the discussion title, the entity the discussion (type 11).
/// 422 also rewrote activity_party with the subscribers only, which deleted the Creator (s128-155): not built.
fn comment_in(
    tx: &Connection,
    discussion_id: &str,
    comment: &str,
    creator: &str,
    stamp: &Stamp,
) -> Result<String, CommandError> {
    let text = crate::parties::clean_text(comment);
    if text.trim().is_empty() {
        return Err(field_error("comment", "Write a comment first."));
    }
    let id = crate::ids::new_id();
    tx.execute(
        "INSERT INTO activity_comment (id, activity_id, creator_id, comment, date, time) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, discussion_id, creator, text.trim(), &stamp.at[..10], &stamp.at[11..]],
    )?;
    audit::stamp_created(tx, "activity_comment", &id, stamp)?;
    tx.execute(
        "UPDATE activity SET comment_count_cached = (SELECT COUNT(*) FROM activity_comment WHERE activity_id = ?1),
                             last_updated = ?2
         WHERE id = ?1",
        params![discussion_id, stamp.at],
    )?;
    audit::stamp_modified(tx, "activity", &discussion_id, stamp)?;
    let title: String = tx.query_row(
        "SELECT COALESCE(d.title, a.subject, '') FROM discussion d JOIN activity a ON a.id = d.id WHERE d.id = ?1",
        [discussion_id],
        |r| r.get(0),
    )?;
    let parties = |sql: &str| -> rusqlite::Result<Vec<String>> {
        let mut stmt = tx.prepare(sql)?;
        let rows = stmt.query_map([discussion_id], |r| r.get(0))?.collect();
        rows
    };
    let mut notify = parties(
        "SELECT DISTINCT party_id FROM activity_party
         WHERE activity_id = ?1 AND CAST(activity_party_role_id AS INTEGER) = 64 AND party_id IS NOT NULL",
    )?;
    if notify.is_empty() {
        notify =
            parties("SELECT DISTINCT party_id FROM activity_party WHERE activity_id = ?1 AND party_id IS NOT NULL")?;
    }
    for party in notify {
        crate::notifications::create(tx, &party, &title, discussion_id, EntityType::Activity.id(), stamp)?;
    }
    Ok(id)
}

// ---------------------------------------------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub party_id: String,
    pub who: String,
    pub organization: bool,
    pub date: String,
    pub time: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discussion {
    pub activity: activities::Activity,
    pub title: String,
    /// active_ind: NULL counts as active (421 never sets it).
    pub active: bool,
    pub date: String,
    pub time: String,
    pub comments: Vec<Comment>,
}

impl Discussion {
    pub fn subscribers(&self) -> Vec<&PartyRow> {
        self.activity.parties.iter().filter(|p| p.role_id == ActivityPartyRole::Subscriber.id()).collect()
    }
}

/// One discussion with its comments, oldest first (NotFound for an id that is not a discussion).
pub fn discussion_get(conn: &Connection, id: &str) -> Result<Discussion, CommandError> {
    let (title, active, date, time): (Option<String>, Option<i64>, Option<String>, Option<String>) = conn
        .query_row("SELECT title, active_ind, date, time FROM discussion WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let activity = activities::activity_get(conn, id)?;
    let mut stmt = conn.prepare(
        "SELECT c.id, COALESCE(c.creator_id, ''), COALESCE(p.name_cached, ''), COALESCE(p.organization_ind, 0) = 1,
                COALESCE(c.date, ''), COALESCE(c.time, ''), COALESCE(c.comment, '')
         FROM activity_comment c LEFT JOIN party p ON p.id = c.creator_id
         WHERE c.activity_id = ?1 ORDER BY c.date, c.time, c.rowid",
    )?;
    let comments = stmt
        .query_map([id], |r| {
            Ok(Comment {
                id: r.get(0)?,
                party_id: r.get(1)?,
                who: r.get(2)?,
                organization: r.get(3)?,
                date: r.get(4)?,
                time: r.get(5)?,
                text: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Discussion {
        title: title.filter(|t| !t.is_empty()).unwrap_or_else(|| activity.subject.clone()),
        active: active != Some(0),
        date: date.unwrap_or_default(),
        time: time.unwrap_or_default(),
        activity,
        comments,
    })
}

/// The people the subscribers card offers (kontacts 313 s100 pre-fills it with every party on the discussion):
/// every party on the discussion, the party it is about and that party's current relations. (id, name, subscribed).
pub fn subscriber_choices(conn: &Connection, id: &str) -> Result<Vec<(String, String, bool)>, CommandError> {
    let d = discussion_get(conn, id)?;
    let mut out: Vec<(String, String, bool)> = Vec::new();
    let mut push = |pid: &str, name: &str| {
        if !pid.is_empty() && !out.iter().any(|(p, _, _)| p == pid) {
            let on = d.subscribers().iter().any(|s| s.party_id == pid);
            out.push((pid.to_owned(), name.to_owned(), on));
        }
    };
    for p in &d.activity.parties {
        push(&p.party_id, &p.name);
    }
    for e in d.activity.entities.iter().filter(|e| e.type_id == EntityType::Party.id()) {
        push(&e.id, &e.label);
        for r in crate::parties::relations_of(conn, &e.id)?.into_iter().filter(|r| r.current) {
            push(&r.other_id, &r.other_name);
        }
    }
    Ok(out)
}

/// The people a new discussion on a party may be followed by: the party and its current relations, none chosen
/// (a new discussion has no subscribers unless some are passed, 421; 313's pre-fill is empty for a new one).
pub fn new_subscriber_choices(
    conn: &Connection,
    entity_type_id: i64,
    entity_id: &str,
) -> rusqlite::Result<Vec<(String, String)>> {
    if entity_type_id != EntityType::Party.id() {
        return Ok(Vec::new());
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let name: Option<String> =
        conn.query_row("SELECT name_cached FROM party WHERE id = ?1", [entity_id], |r| r.get(0)).optional()?.flatten();
    if let Some(n) = name {
        out.push((entity_id.to_owned(), n));
    }
    for r in crate::parties::relations_of(conn, entity_id)?.into_iter().filter(|r| r.current) {
        if !out.iter().any(|(p, _)| *p == r.other_id) {
            out.push((r.other_id, r.other_name));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ACTIVITY-09 notification_list_mark_read (BR-ACTIVITY-11)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notification {
    pub id: String,
    pub date: String,
    pub description: String,
    pub read: bool,
    /// The page of what it is about, when that still exists.
    pub href: String,
}

/// The user's notifications, newest first, with the unread count (the user's cached count, kontacts 373).
pub fn notification_list(
    conn: &Connection,
    actor: &Actor,
    limit: i64,
) -> Result<(Vec<Notification>, i64), CommandError> {
    let Some(me) = activities::current_party(conn, actor)? else { return Ok((Vec::new(), 0)) };
    let mut stmt = conn.prepare(
        "SELECT n.id, COALESCE(n.date, ''), COALESCE(n.description, ''), COALESCE(n.read_ind, 0) = 1, n.entity_id,
                CAST(n.entity_type_id AS INTEGER), CAST(a.activity_type_id AS INTEGER)
         FROM notification n LEFT JOIN activity a ON a.id = n.entity_id AND CAST(n.entity_type_id AS INTEGER) = 11
         WHERE n.party_id = ?1
         ORDER BY COALESCE(n.date, '') DESC, n.creation_timestamp DESC, n.rowid DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![me, limit], |r| {
            let entity: Option<String> = r.get(4)?;
            let etype: Option<i64> = r.get(5)?;
            let atype: Option<i64> = r.get(6)?;
            let href = match (entity, etype, atype) {
                (Some(e), Some(11), Some(t)) => activities::page_href(&e, t),
                (Some(e), Some(10), _) => format!("/parties/{}", crate::web::urlencode(&e)),
                _ => String::new(),
            };
            Ok(Notification { id: r.get(0)?, date: r.get(1)?, description: r.get(2)?, read: r.get(3)?, href })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let unread: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notification WHERE party_id = ?1 AND COALESCE(read_ind, 0) <> 1",
        [&me],
        |r| r.get(0),
    )?;
    Ok((rows, unread))
}

/// Marks one notification of the user read (layout 101's row button: `ReadInd = 1`), then recounts (373 s6). A
/// notification of another party is NotFound.
pub fn notification_mark_read(conn: &mut Connection, id: &str, actor: &Actor) -> Result<(), CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let me = activities::current_party(&tx, actor)?.ok_or(CommandError::NotFound)?;
    let n = tx.execute("UPDATE notification SET read_ind = 1 WHERE id = ?1 AND party_id = ?2", params![id, me])?;
    if n == 0 {
        return Err(CommandError::NotFound);
    }
    audit::stamp_modified(&tx, "notification", &id, &stamp)?;
    crate::notifications::recount(&tx, &me)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_in_memory, Seed};

    fn demo() -> (Connection, Actor) {
        let (conn, _) = open_in_memory(Seed::Demo).unwrap();
        let actor = crate::admin::current_user(&conn).unwrap().unwrap();
        (conn, actor)
    }

    fn on(entity_type_id: i64, entity_id: &str) -> NewDiscussion {
        NewDiscussion { entity_type_id, entity_id: entity_id.into(), ..Default::default() }
    }

    #[test]
    fn the_title_comes_from_the_entity() {
        let (conn, _) = demo();
        assert_eq!(title_for(&conn, 10, "p-hp").unwrap(), "Halden & Pryce Architects");
        assert_eq!(title_for(&conn, 11, "a-task-quote").unwrap(), "Send revised fit-out quote");
        let st: String =
            conn.query_row("SELECT id FROM sales_transaction WHERE doc_number = 'INV-1048'", [], |r| r.get(0)).unwrap();
        assert_eq!(title_for(&conn, 12, &st).unwrap(), "#INV-1048 - Halden & Pryce Architects");
        let opp: Option<String> =
            conn.query_row("SELECT id FROM opportunity LIMIT 1", [], |r| r.get(0)).optional().unwrap();
        if let Some(o) = opp {
            assert!(!title_for(&conn, 13, &o).unwrap().is_empty());
        }
        assert!(matches!(title_for(&conn, 14, "x"), Err(CommandError::Validation(_))));
        assert!(matches!(title_for(&conn, 10, "nobody"), Err(CommandError::Validation(_))));
    }

    #[test]
    fn create_writes_the_rows_and_notes_become_the_first_comment() {
        let (mut conn, actor) = demo();
        let mut new = on(10, "p-hp");
        new.notes = "Kick-off agenda attached.".into();
        new.subscribers = vec!["p-hp-eh".into(), "p-hp-eh".into()];
        let id = discussion_create(&mut conn, &new, &actor).unwrap();
        let d = discussion_get(&conn, &id).unwrap();
        assert_eq!(d.title, "Halden & Pryce Architects");
        assert_eq!(d.activity.type_id, 10);
        assert!(d.activity.discussion && d.active);
        let roles: Vec<(i64, &str)> = d.activity.parties.iter().map(|p| (p.role_id, p.party_id.as_str())).collect();
        assert_eq!(roles, [(50, "p-mara"), (64, "p-hp-eh")], "creator, one subscriber; the creator is not subscribed");
        assert_eq!(d.comments.len(), 1);
        assert_eq!((d.comments[0].who.as_str(), d.activity.comment_count), ("Mara Ellison", 1));
        assert_eq!(d.activity.entities[0].id, "p-hp");
        let active: Option<i64> =
            conn.query_row("SELECT active_ind FROM discussion WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(active, None, "421 leaves active_ind empty");
    }

    #[test]
    fn comments_count_and_notify_the_subscribers() {
        let (mut conn, actor) = demo();
        for n in 1..=3 {
            comment_add(&mut conn, "a-disc-light", &format!("Comment {n}"), &actor).unwrap();
        }
        let d = discussion_get(&conn, "a-disc-light").unwrap();
        assert_eq!(d.activity.comment_count, 5, "2 in the seed + 3");
        assert_eq!(d.comments.len(), 5);
        let to: Vec<String> = {
            let mut s = conn.prepare("SELECT party_id FROM notification WHERE entity_id = 'a-disc-light'").unwrap();
            s.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(to, ["p-hp-eh", "p-hp-eh", "p-hp-eh"], "the one subscriber, once per comment");
        let creator = d.activity.parties.iter().any(|p| p.role_id == 50);
        assert!(creator, "the Creator survives comments (422's rewrite is not built)");
        assert!(matches!(comment_add(&mut conn, "a-disc-light", "  ", &actor), Err(CommandError::Validation(_))));
        assert!(matches!(comment_add(&mut conn, "a-task-quote", "x", &actor), Err(CommandError::NotFound)));
    }

    #[test]
    fn with_no_subscribers_everyone_on_it_is_notified_the_commenter_too() {
        let (mut conn, actor) = demo();
        let id = discussion_create(&mut conn, &on(10, "p-bm"), &actor).unwrap();
        comment_add(&mut conn, &id, "First", &actor).unwrap();
        let (n, unread): (i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM notification WHERE entity_id = ?1 AND party_id = 'p-mara'),
                        (SELECT unread_notification_count_cached FROM user WHERE id = 'u-mara')",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(n, 1, "the creator is on it and is not left out");
        assert!(unread >= 1);
    }

    #[test]
    fn edit_replaces_the_subscribers_keeps_the_creator_and_sets_active() {
        let (mut conn, actor) = demo();
        let list = vec!["p-hp-mp".to_owned(), "p-hp-jw".to_owned()];
        discussion_edit(&mut conn, "a-disc-light", Some(false), Some(&list), &actor).unwrap();
        let d = discussion_get(&conn, "a-disc-light").unwrap();
        let mut subs: Vec<&str> = d.subscribers().iter().map(|p| p.party_id.as_str()).collect();
        subs.sort();
        assert_eq!(subs, ["p-hp-jw", "p-hp-mp"]);
        assert!(
            d.activity.parties.iter().any(|p| p.role_id == 50) && d.activity.parties.iter().any(|p| p.role_id == 63)
        );
        assert!(!d.active);
        discussion_edit(&mut conn, "a-disc-light", Some(true), Some(&[]), &actor).unwrap();
        let d = discussion_get(&conn, "a-disc-light").unwrap();
        assert!(d.subscribers().is_empty() && d.active);
    }

    #[test]
    fn notifications_are_listed_and_marked_read_one_at_a_time() {
        let (mut conn, actor) = demo();
        let id = discussion_create(&mut conn, &on(10, "p-hp"), &actor).unwrap();
        comment_add(&mut conn, &id, "one", &actor).unwrap();
        comment_add(&mut conn, &id, "two", &actor).unwrap();
        let (list, unread) = notification_list(&conn, &actor, 20).unwrap();
        assert_eq!((list.len(), unread), (2, 2));
        assert_eq!(list[0].href, format!("/discussions/{id}"));
        assert_eq!(list[0].description, "Halden & Pryce Architects");
        notification_mark_read(&mut conn, &list[0].id, &actor).unwrap();
        let (_, unread) = notification_list(&conn, &actor, 20).unwrap();
        assert_eq!(unread, 1);
        let cached: i64 = conn
            .query_row("SELECT unread_notification_count_cached FROM user WHERE id = 'u-mara'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cached, 1, "the recount of 373");
        assert!(matches!(notification_mark_read(&mut conn, "nope", &actor), Err(CommandError::NotFound)));
    }
}
