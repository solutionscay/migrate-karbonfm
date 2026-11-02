//! ACTIVITY commands of I4 (05 OP-ACTIVITY-01..05 and 08, 07 BR-ACTIVITY-*): create, edit and delete a note, task,
//! event, meeting or call; the parties on an activity; complete a task; list a party's activities. Plain SQL; each
//! write is one `BEGIN IMMEDIATE` transaction with one stamp (BR-PARTY-07). Routes never run SQL (AP-01).
//!
//! The source scripts are kontroller 377 (the shared create), 390 note, 391 task, 392 event, 641 call, 376/631/638
//! (activity parties), 542/541 (edit), 515 (delete), 690/691 (complete), 497 (list) and kontacts 668 (closing the
//! window). Where a script is broken the command builds what it clearly intends (03 "Business rules", D-07); each
//! such place says so.

use crate::admin::{CommandError, FieldError, ALREADY_SENT};
use crate::audit::{self, Actor, Stamp};
use crate::reference::{ActivityPartyRole, ActivityType, EntityType};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

/// What can be created through OP-ACTIVITY-01 here. Email is created by `email.rs` (S5) and Discussion by
/// `discussions.rs`; both share the party and cache helpers below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Note,
    Task,
    Event,
    Meeting,
    Call,
    Email,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "note" => Kind::Note,
            "task" => Kind::Task,
            "event" => Kind::Event,
            "meeting" => Kind::Meeting,
            "call" => Kind::Call,
            "email" => Kind::Email,
            _ => return None,
        })
    }
    pub fn key(self) -> &'static str {
        match self {
            Kind::Note => "note",
            Kind::Task => "task",
            Kind::Event => "event",
            Kind::Meeting => "meeting",
            Kind::Call => "call",
            Kind::Email => "email",
        }
    }
    /// BR-ACTIVITY-01: the fixed type id (390 s126, 391 s129, 392 s137, 641 s134, 389 s176).
    pub fn activity_type(self) -> ActivityType {
        match self {
            Kind::Note => ActivityType::Note,
            Kind::Task => ActivityType::Task,
            Kind::Event => ActivityType::Event,
            Kind::Meeting => ActivityType::Meeting,
            Kind::Call => ActivityType::Call,
            Kind::Email => ActivityType::Email,
        }
    }
    /// The one *_ind flag each type sets (BR-ACTIVITY-01). A meeting sets event_ind (392 s101) and event.meeting_ind.
    fn flag(self) -> &'static str {
        match self {
            Kind::Note => "note_ind",
            Kind::Task => "task_ind",
            Kind::Event | Kind::Meeting => "event_ind",
            Kind::Call => "call_ind",
            Kind::Email => "email_message_ind",
        }
    }
    pub fn label(self) -> &'static str {
        self.activity_type().name()
    }
}

/// The editable fields of an activity. Each kind reads the ones it has; the others are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub subject: String,
    pub message: String,
    /// Note, call, email: the activity date and time (377 s96-100: today and now when empty).
    pub date: String,
    pub time: String,
    /// Task (391): due date (default today, s85) and time; stored, which the source meant and missed (s72, s80).
    pub due_date: String,
    pub due_time: String,
    /// Event and meeting (392 s109-116).
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub location: String,
    pub all_day: bool,
    /// Call (641 s150-152).
    pub caller_name: String,
    pub callback_number: String,
    pub result: String,
}

/// One party to put on a new activity: role, party (optional, 376 s64) and contact method (optional).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyInput {
    pub role: ActivityPartyRole,
    pub party_id: String,
    pub contact_method_id: String,
}

impl PartyInput {
    pub fn new(role: ActivityPartyRole, party_id: &str) -> Self {
        Self { role, party_id: party_id.into(), contact_method_id: String::new() }
    }
}

/// OP-ACTIVITY-01 input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewActivity {
    pub kind: Kind,
    pub fields: Fields,
    /// The record the activity is about (activity_entity, 377 s165-169): entity type id and entity id.
    pub entity: Option<(i64, String)>,
    pub parties: Vec<PartyInput>,
}

fn field_error(field: &'static str, message: impl Into<String>) -> CommandError {
    CommandError::Validation(vec![FieldError { field, message: message.into() }])
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// An ISO date `YYYY-MM-DD`, or None when empty; anything else is a field error.
fn date_of(field: &'static str, s: &str) -> Result<Option<String>, CommandError> {
    match opt(s) {
        None => Ok(None),
        Some(t) => chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d")
            .map(|d| Some(d.format("%Y-%m-%d").to_string()))
            .map_err(|_| field_error(field, "Enter a date as YYYY-MM-DD.")),
    }
}

/// A time `HH:MM` or `HH:MM:SS`, stored as `HH:MM:SS`, or None when empty.
fn time_of(field: &'static str, s: &str) -> Result<Option<String>, CommandError> {
    match opt(s) {
        None => Ok(None),
        Some(t) => chrono::NaiveTime::parse_from_str(t, "%H:%M:%S")
            .or_else(|_| chrono::NaiveTime::parse_from_str(t, "%H:%M"))
            .map(|v| Some(v.format("%H:%M:%S").to_string()))
            .map_err(|_| field_error(field, "Enter a time as HH:MM.")),
    }
}

/// The current user's party (user.party_id), the default Creator (BR-ACTIVITY-02).
pub fn current_party(conn: &Connection, actor: &Actor) -> rusqlite::Result<Option<String>> {
    let Some(user) = &actor.user_id else { return Ok(None) };
    Ok(conn
        .query_row("SELECT party_id FROM user WHERE id = ?1", [user], |r| r.get::<_, Option<String>>(0))
        .optional()?
        .flatten())
}

/// 377 s127-129: "user profile not associated with a party".
fn no_party() -> CommandError {
    field_error("creator", "Your user is not linked to a person, so the activity cannot have a creator.")
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ACTIVITY-01 activity_create
// ---------------------------------------------------------------------------------------------------------------

/// The dates and times a kind stores, checked. Event and meeting need a subject and a start date (392 s61).
struct Checked {
    subject: Option<String>,
    message: Option<String>,
    date_display: String,
    time_display: Option<String>,
    due_date: Option<String>,
    due_time: Option<String>,
    start_date: Option<String>,
    start_time: Option<String>,
    end_date: Option<String>,
    end_time: Option<String>,
}

fn check_fields(kind: Kind, f: &Fields, today: &str, now: &str) -> Result<Checked, CommandError> {
    let mut errors = Vec::new();
    let mut keep = |r: Result<Option<String>, CommandError>| match r {
        Ok(v) => v,
        Err(CommandError::Validation(mut e)) => {
            errors.append(&mut e);
            None
        }
        Err(_) => None,
    };
    let date = keep(date_of("date", &f.date));
    let time = keep(time_of("time", &f.time));
    let due_date = keep(date_of("due_date", &f.due_date));
    let due_time = keep(time_of("due_time", &f.due_time));
    let start_date = keep(date_of("start_date", &f.start_date));
    let start_time = keep(time_of("start_time", &f.start_time));
    let end_date = keep(date_of("end_date", &f.end_date));
    let end_time = keep(time_of("end_time", &f.end_time));
    let subject = opt(&f.subject).map(str::to_owned);
    if matches!(kind, Kind::Event | Kind::Meeting) {
        if subject.is_none() {
            errors.push(FieldError { field: "subject", message: "Enter a subject.".into() });
        }
        if start_date.is_none() && !errors.iter().any(|e| e.field == "start_date") {
            errors.push(FieldError { field: "start_date", message: "Enter the start date.".into() });
        }
    }
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }
    let (date_display, time_display) = match kind {
        // 391 s85, s140: the due date (default today) is the date shown.
        Kind::Task => (due_date.clone().unwrap_or_else(|| today.into()), due_time.clone()),
        // 668 s59-63: an event shows its start.
        Kind::Event | Kind::Meeting => (start_date.clone().unwrap_or_else(|| today.into()), start_time.clone()),
        _ => (date.unwrap_or_else(|| today.into()), Some(time.unwrap_or_else(|| now.into()))),
    };
    Ok(Checked {
        subject,
        message: opt(&f.message).map(str::to_owned),
        date_display,
        time_display,
        // 391 s85: DueDate defaults to today.
        due_date: if kind == Kind::Task { Some(due_date.unwrap_or_else(|| today.into())) } else { due_date },
        due_time,
        start_date,
        start_time,
        end_date,
        end_time,
    })
}

/// OP-ACTIVITY-01 `activity_create` (BR-ACTIVITY-01, 02, 03; A1-A9 of the I4 plan). Returns the new activity id.
pub fn activity_create(conn: &mut Connection, new: &NewActivity, actor: &Actor) -> Result<String, CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = create_in(&tx, new, &stamp)?;
    tx.commit()?;
    Ok(id)
}

/// The body of [`activity_create`], inside the caller's transaction (the email create of S5 and the discussion
/// create use it too).
pub fn create_in(tx: &Connection, new: &NewActivity, stamp: &Stamp) -> Result<String, CommandError> {
    let (today, now) = (&stamp.at[..10], &stamp.at[11..]);
    let c = check_fields(new.kind, &new.fields, today, now)?;
    // 668 s10-17: a subject is required once the activity has a message or is about a record.
    if c.subject.is_none() && (c.message.is_some() || new.entity.is_some()) {
        return Err(field_error("subject", "The subject is required."));
    }
    if let Some((type_id, entity_id)) = &new.entity {
        check_entity(tx, *type_id, entity_id)?;
    }
    // BR-ACTIVITY-02 (377 s120-135): the current user is the Creator unless the request names one.
    let mut parties = new.parties.clone();
    if !parties.iter().any(|p| p.role == ActivityPartyRole::Creator) {
        let me = current_party(tx, &stamp.actor)?.ok_or_else(no_party)?;
        parties.insert(0, PartyInput::new(ActivityPartyRole::Creator, &me));
    }

    let id = crate::ids::new_id();
    let kind = new.kind;
    tx.execute(
        &format!(
            "INSERT INTO activity (id, activity_type_id, {flag}, subject, message, date_display, time_display, last_updated)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
            flag = kind.flag()
        ),
        params![id, kind.activity_type().id(), c.subject, c.message, c.date_display, c.time_display, stamp.at],
    )?;
    audit::stamp_created(tx, "activity", &id, stamp)?;
    match kind {
        Kind::Task => {
            tx.execute(
                "INSERT INTO task (id, due_date, due_time) VALUES (?1, ?2, ?3)",
                params![id, c.due_date, c.due_time],
            )?;
        }
        Kind::Event | Kind::Meeting => {
            tx.execute(
                "INSERT INTO event (id, start_date, start_time, end_date, end_time, location, all_day_event_ind, meeting_ind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    c.start_date,
                    c.start_time,
                    c.end_date,
                    c.end_time,
                    opt(&new.fields.location),
                    i64::from(new.fields.all_day),
                    // 392 s120-121 makes a Meeting row; the target keeps it as event.meeting_ind (04, D-15). The
                    // source never set MeetingInd (Event field 197): built as intended.
                    (kind == Kind::Meeting).then_some(1)
                ],
            )?;
            audit::stamp_created(tx, "event", &id, stamp)?;
        }
        Kind::Call => {
            tx.execute(
                "INSERT INTO call (id, caller_name, callback_number, result) VALUES (?1, ?2, ?3, ?4)",
                params![id, opt(&new.fields.caller_name), opt(&new.fields.callback_number), opt(&new.fields.result)],
            )?;
        }
        Kind::Note | Kind::Email => {}
    }
    if let Some((type_id, entity_id)) = &new.entity {
        let link = crate::ids::new_id();
        tx.execute(
            "INSERT INTO activity_entity (id, activity_id, entity_type_id, entity_id) VALUES (?1, ?2, ?3, ?4)",
            params![link, id, type_id, entity_id],
        )?;
        audit::stamp_created(tx, "activity_entity", &link, stamp)?;
    }
    let target = Target::of(tx, &id)?;
    for p in &parties {
        add_party_in(tx, &target, p, stamp)?;
    }
    refresh_caches(tx, &id)?;

    // 391 s159-170: a task assigned to someone else notifies the assignee. The source refuses a task with no assignee
    // and 670 s100 drops the text; both are bugs, built as intended (a notification only when there is an assignee).
    if kind == Kind::Task {
        let creator = parties.iter().find(|p| p.role == ActivityPartyRole::Creator).map(|p| p.party_id.clone());
        let from = parties
            .iter()
            .find(|p| p.role == ActivityPartyRole::AssignedBy && !p.party_id.is_empty())
            .or_else(|| parties.iter().find(|p| p.role == ActivityPartyRole::Creator))
            .map(|p| party_name(tx, &p.party_id))
            .transpose()?
            .unwrap_or_default();
        for p in parties.iter().filter(|p| p.role == ActivityPartyRole::AssignedTo && !p.party_id.is_empty()) {
            if Some(&p.party_id) != creator.as_ref() {
                let text = format!("Task from {from} - {}", c.subject.as_deref().unwrap_or(""));
                crate::notifications::create(tx, &p.party_id, &text, &id, EntityType::Activity.id(), stamp)?;
            }
        }
    }
    Ok(id)
}

/// The entity an activity is about must exist (377 s166: the type must be one of entity_type).
fn check_entity(conn: &Connection, type_id: i64, entity_id: &str) -> Result<(), CommandError> {
    let table = match EntityType::from_id(type_id) {
        Some(EntityType::Party) => "party",
        Some(EntityType::Activity) => "activity",
        Some(EntityType::SalesTransaction) => "sales_transaction",
        Some(EntityType::Opportunity) => "opportunity",
        _ => return Err(field_error("entity_type_id", "Not a valid entity type.")),
    };
    let found: bool =
        conn.query_row(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"), [entity_id], |r| r.get(0))?;
    if found {
        Ok(())
    } else {
        Err(field_error("entity_id", "The record this is about does not exist."))
    }
}

fn party_name(conn: &Connection, party_id: &str) -> rusqlite::Result<String> {
    Ok(conn
        .query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [party_id], |r| r.get(0))
        .optional()?
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ACTIVITY-04 activity_party_set (BR-ACTIVITY-02, 03)
// ---------------------------------------------------------------------------------------------------------------

/// What the party commands need to know about the activity.
pub struct Target {
    pub id: String,
    pub type_id: i64,
    /// The duplicate contact method check applies to email messages and tasks (376 s155-162).
    pub email_or_task: bool,
    pub sent: bool,
}

impl Target {
    pub fn of(conn: &Connection, id: &str) -> Result<Self, CommandError> {
        conn.query_row(
            "SELECT a.id, CAST(a.activity_type_id AS INTEGER),
                    COALESCE(a.email_message_ind, 0) = 1 OR COALESCE(a.task_ind, 0) = 1,
                    COALESCE(e.sent_ind, 0) = 1
             FROM activity a LEFT JOIN email_message e ON e.id = a.id WHERE a.id = ?1",
            [id],
            |r| {
                Ok(Target {
                    id: r.get(0)?,
                    type_id: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    email_or_task: r.get(2)?,
                    sent: r.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or(CommandError::NotFound)
    }
}

/// Initials as 376 s181-185 caches them: an organization's first letter, a person's capitals (at most 3).
pub fn party_initials(name: &str, organization: bool) -> String {
    if organization {
        name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default()
    } else {
        name.chars().filter(|c| c.is_uppercase()).take(3).collect()
    }
}

/// The party's name and initials, or a field error when the party does not exist (376 s112-113).
fn party_cache(conn: &Connection, party_id: &str) -> Result<(String, String), CommandError> {
    let row: Option<(String, bool)> = conn
        .query_row(
            "SELECT COALESCE(name_cached, ''), COALESCE(organization_ind, 0) = 1 FROM party WHERE id = ?1",
            [party_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (name, org) = row.ok_or_else(|| field_error("other_id", "That person or organization does not exist."))?;
    let initials = party_initials(&name, org);
    Ok((name, initials))
}

/// The email address of a contact method (its network address), for the activity_party_email row.
fn method_address(conn: &Connection, contact_method_id: &str) -> rusqlite::Result<Option<String>> {
    Ok(conn
        .query_row("SELECT address FROM network_address WHERE id = ?1", [contact_method_id], |r| r.get(0))
        .optional()?
        .flatten())
}

fn check_role(target: &Target, role: ActivityPartyRole) -> Result<(), CommandError> {
    if role.eligible_for(target.type_id) {
        Ok(())
    } else {
        let what = ActivityType::from_id(target.type_id).map(|t| t.name().to_lowercase()).unwrap_or("activity".into());
        Err(field_error("role", format!("“{}” does not apply to this {what}.", role.name())))
    }
}

/// Adds one activity party (376): checks, row, name and initials caches, the email or event subtype row.
pub fn add_party_in(conn: &Connection, target: &Target, p: &PartyInput, stamp: &Stamp) -> Result<String, CommandError> {
    let cache = match opt(&p.party_id) {
        Some(pid) => Some(party_cache(conn, pid)?),
        None => None,
    };
    check_role(target, p.role)?;
    let cm = opt(&p.contact_method_id);
    if let (true, Some(cm)) = (target.email_or_task, cm) {
        let dup: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM activity_party WHERE activity_id = ?1 AND activity_party_role_id = ?2
                                                         AND contact_method_id = ?3)",
            params![target.id, p.role.id(), cm],
            |r| r.get(0),
        )?;
        if dup {
            return Err(field_error("contact_method_id", "This contact is on the activity in that role already."));
        }
    }
    let id = crate::ids::new_id();
    let (name, initials) = cache.unzip();
    conn.execute(
        "INSERT INTO activity_party (id, activity_id, party_id, activity_party_role_id, contact_method_id,
                                     party_name_cached, party_initials)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, target.id, opt(&p.party_id), p.role.id(), cm, name, initials],
    )?;
    audit::stamp_created(conn, "activity_party", &id, stamp)?;
    write_subtypes(conn, &id, p.role, cm)?;
    Ok(id)
}

/// The email and event subtype rows a role makes (710 s34, 376 s190-215); a row the role no longer makes is removed.
fn write_subtypes(conn: &Connection, id: &str, role: ActivityPartyRole, cm: Option<&str>) -> rusqlite::Result<()> {
    if role.creates_email_row() {
        let address = match cm {
            Some(cm) => method_address(conn, cm)?,
            None => None,
        };
        conn.execute(
            "INSERT INTO activity_party_email (id, email_address, email_address_formatted) VALUES (?1, ?2, ?2)
             ON CONFLICT (id) DO UPDATE SET email_address = excluded.email_address,
                                            email_address_formatted = excluded.email_address_formatted",
            params![id, address],
        )?;
    } else {
        conn.execute("DELETE FROM activity_party_email WHERE id = ?1", [id])?;
    }
    if role.creates_event_row() {
        conn.execute("INSERT INTO activity_party_event (id) VALUES (?1) ON CONFLICT (id) DO NOTHING", [id])?;
    } else {
        conn.execute("DELETE FROM activity_party_event WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// The activity caches 376 keeps (creator_cached s249-251, parties_cached s222-224, task from/to s234-240),
/// recomputed from the rows. The source appends a name on every add and edit, so names piled up: built as intended.
pub fn refresh_caches(conn: &Connection, activity_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE activity SET
            parties_cached = (SELECT group_concat(n, ', ') FROM (
                SELECT party_name_cached AS n FROM activity_party
                WHERE activity_id = ?1 AND CAST(activity_party_role_id AS INTEGER) NOT IN (50, 51)
                  AND COALESCE(party_name_cached, '') <> '' ORDER BY rowid)),
            creator_cached = (SELECT party_name_cached FROM activity_party
                              WHERE activity_id = ?1 AND CAST(activity_party_role_id AS INTEGER) = 50 ORDER BY rowid LIMIT 1)
         WHERE id = ?1",
        [activity_id],
    )?;
    conn.execute(
        "UPDATE task SET
            from_cached = (SELECT party_name_cached FROM activity_party
                           WHERE activity_id = ?1 AND CAST(activity_party_role_id AS INTEGER) = 52 ORDER BY rowid LIMIT 1),
            to_cached = (SELECT party_name_cached FROM activity_party
                         WHERE activity_id = ?1 AND CAST(activity_party_role_id AS INTEGER) = 53 ORDER BY rowid LIMIT 1)
         WHERE id = ?1",
        [activity_id],
    )?;
    Ok(())
}

fn touch(conn: &Connection, activity_id: &str, stamp: &Stamp) -> rusqlite::Result<()> {
    conn.execute("UPDATE activity SET last_updated = ?2 WHERE id = ?1", params![activity_id, stamp.at])?;
    audit::stamp_modified(conn, "activity", &activity_id, stamp)?;
    Ok(())
}

fn writable(conn: &Connection, activity_id: &str) -> Result<Target, CommandError> {
    let target = Target::of(conn, activity_id)?;
    if target.sent {
        return Err(CommandError::AlreadySent);
    }
    Ok(target)
}

/// OP-ACTIVITY-04, add: one party on the activity (376). Refused on a sent email (AP-08).
pub fn activity_party_add(
    conn: &mut Connection,
    activity_id: &str,
    p: &PartyInput,
    actor: &Actor,
) -> Result<String, CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let target = writable(&tx, activity_id)?;
    let id = add_party_in(&tx, &target, p, &stamp)?;
    refresh_caches(&tx, activity_id)?;
    touch(&tx, activity_id, &stamp)?;
    tx.commit()?;
    Ok(id)
}

/// The activity of an activity_party row.
pub fn activity_of_party(conn: &Connection, activity_party_id: &str) -> Result<String, CommandError> {
    conn.query_row("SELECT activity_id FROM activity_party WHERE id = ?1", [activity_party_id], |r| r.get(0))
        .optional()?
        .ok_or(CommandError::NotFound)
}

/// OP-ACTIVITY-04, edit: party, role and contact method of one row (631). The same party, activity and role checks
/// as an add; 631 has no duplicate check, so there is none. 631's field writes test an unset variable, so the source
/// never changed the party or role: built as intended.
pub fn activity_party_edit(
    conn: &mut Connection,
    activity_party_id: &str,
    p: &PartyInput,
    actor: &Actor,
) -> Result<(), CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let activity_id = activity_of_party(&tx, activity_party_id)?;
    let target = writable(&tx, &activity_id)?;
    // 631 s67: the party is required on an edit.
    let pid = opt(&p.party_id).ok_or_else(|| field_error("other_id", "Choose a person or organization."))?;
    let (name, initials) = party_cache(&tx, pid)?;
    check_role(&target, p.role)?;
    let cm = opt(&p.contact_method_id);
    tx.execute(
        "UPDATE activity_party SET party_id = ?2, activity_party_role_id = ?3, contact_method_id = ?4,
                                   party_name_cached = ?5, party_initials = ?6
         WHERE id = ?1",
        params![activity_party_id, pid, p.role.id(), cm, name, initials],
    )?;
    audit::stamp_modified(&tx, "activity_party", &activity_party_id, &stamp)?;
    write_subtypes(&tx, activity_party_id, p.role, cm)?;
    refresh_caches(&tx, &activity_id)?;
    touch(&tx, &activity_id, &stamp)?;
    tx.commit()?;
    Ok(())
}

/// OP-ACTIVITY-04, remove: deletes the row and its subtype rows, then the caches (638 leaves both: built as
/// intended). There is no role guard (638), so the Creator can be removed.
pub fn activity_party_remove(
    conn: &mut Connection,
    activity_party_id: &str,
    actor: &Actor,
) -> Result<String, CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let activity_id = activity_of_party(&tx, activity_party_id)?;
    writable(&tx, &activity_id)?;
    tx.execute("DELETE FROM activity_party_email WHERE id = ?1", [activity_party_id])?;
    tx.execute("DELETE FROM activity_party_event WHERE id = ?1", [activity_party_id])?;
    tx.execute("DELETE FROM activity_party WHERE id = ?1", [activity_party_id])?;
    refresh_caches(&tx, &activity_id)?;
    touch(&tx, &activity_id, &stamp)?;
    tx.commit()?;
    Ok(activity_id)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ACTIVITY-02 activity_update (BR-ACTIVITY-05, 08)
// ---------------------------------------------------------------------------------------------------------------

/// The kind of an existing activity, from its type id; None for a type without an edit form (comment, letter, SMS,
/// discussion: the discussion has its own commands).
pub fn kind_of(type_id: i64) -> Option<Kind> {
    Some(match ActivityType::from_id(type_id)? {
        ActivityType::Note => Kind::Note,
        ActivityType::Task => Kind::Task,
        ActivityType::Event => Kind::Event,
        ActivityType::Meeting => Kind::Meeting,
        ActivityType::Call => Kind::Call,
        ActivityType::Email => Kind::Email,
        _ => return None,
    })
}

/// The 668 discard question for an activity with no subject and no content.
pub const DISCARD: &str = "This activity has no subject and no content, so it is deleted.";

/// OP-ACTIVITY-02 `activity_update`: writes the fields of the kind's form; parties change only through OP-ACTIVITY-04,
/// so 542's delete-all of the parties cannot happen; the type never changes (541 s159 set it to 1: a bug).
///
/// BR-ACTIVITY-05 (kontacts 668 s10-26): an empty subject is a field error when the activity has a message or is
/// about a record; with neither, the answer is ConfirmationRequired(DISCARD) and the route offers the delete.
/// BR-ACTIVITY-08 / AP-08: a sent email is refused with AlreadySent.
pub fn activity_update(conn: &mut Connection, id: &str, f: &Fields, actor: &Actor) -> Result<(), CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let target = writable(&tx, id)?;
    let kind =
        kind_of(target.type_id).ok_or_else(|| CommandError::Refused("This activity has no edit form.".into()))?;
    let (today, now) = (&stamp.at[..10], &stamp.at[11..]);
    let created: String = tx.query_row(
        "SELECT COALESCE(substr(creation_timestamp, 1, 10), ?2) FROM activity WHERE id = ?1",
        params![id, today],
        |r| r.get(0),
    )?;
    let mut c = check_fields(kind, f, today, now)?;
    if c.subject.is_none() {
        let linked: bool =
            tx.query_row("SELECT EXISTS (SELECT 1 FROM activity_entity WHERE activity_id = ?1)", [id], |r| r.get(0))?;
        if c.message.is_some() || linked {
            return Err(field_error("subject", "The subject is required."));
        }
        return Err(CommandError::ConfirmationRequired(DISCARD.into()));
    }
    if kind == Kind::Task {
        // 668 s33-39: on a task the date shown is the due date, or the creation date when there is none. An edit
        // keeps an empty due date empty (no default after create).
        let due = date_of("due_date", &f.due_date)?;
        c.due_date = due.clone();
        c.date_display = due.unwrap_or(created);
    }
    tx.execute(
        "UPDATE activity SET subject = ?2, message = ?3, date_display = ?4, time_display = ?5 WHERE id = ?1",
        params![id, c.subject, c.message, c.date_display, c.time_display],
    )?;
    match kind {
        Kind::Task => {
            tx.execute(
                "UPDATE task SET due_date = ?2, due_time = ?3 WHERE id = ?1",
                params![id, c.due_date, c.due_time],
            )?;
        }
        Kind::Event | Kind::Meeting => {
            tx.execute(
                "UPDATE event SET start_date = ?2, start_time = ?3, end_date = ?4, end_time = ?5, location = ?6,
                                  all_day_event_ind = ?7
                 WHERE id = ?1",
                params![id, c.start_date, c.start_time, c.end_date, c.end_time, opt(&f.location), i64::from(f.all_day)],
            )?;
            audit::stamp_modified(&tx, "event", &id, &stamp)?;
        }
        Kind::Call => {
            tx.execute(
                "UPDATE call SET caller_name = ?2, callback_number = ?3, result = ?4 WHERE id = ?1",
                params![id, opt(&f.caller_name), opt(&f.callback_number), opt(&f.result)],
            )?;
        }
        Kind::Note | Kind::Email => {}
    }
    touch(&tx, id, &stamp)?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ACTIVITY-03 activity_delete (BR-ACTIVITY-05, AP-07)
// ---------------------------------------------------------------------------------------------------------------

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The confirmation text of a delete: what goes with the activity.
pub fn delete_summary(conn: &Connection, id: &str) -> Result<String, CommandError> {
    let (type_id, subject): (Option<i64>, Option<String>) = conn
        .query_row("SELECT CAST(activity_type_id AS INTEGER), subject FROM activity WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let count = |sql: &str| -> rusqlite::Result<i64> { conn.query_row(sql, [id], |r| r.get(0)) };
    let parties = count("SELECT COUNT(*) FROM activity_party WHERE activity_id = ?1")?;
    let comments = count("SELECT COUNT(*) FROM activity_comment WHERE activity_id = ?1")?;
    let files = count("SELECT COUNT(*) FROM file_attachment WHERE entity_type_id = 11 AND entity_id = ?1")?;
    let what = type_id.and_then(ActivityType::from_id).map(|t| t.name().to_lowercase()).unwrap_or("activity".into());
    let mut parts = vec![plural(parties, "person on it", "people on it")];
    if comments > 0 {
        parts.push(plural(comments, "comment", "comments"));
    }
    if files > 0 {
        parts.push(plural(files, "file link", "file links"));
    }
    let title = subject.filter(|s| !s.is_empty()).map(|s| format!(" “{s}”")).unwrap_or_default();
    Ok(format!(
        "Deletes the {what}{title} with its {}. Attached files stay in Files.",
        match parts.len() {
            1 => parts.remove(0),
            _ => {
                let last = parts.pop().unwrap_or_default();
                format!("{} and {last}", parts.join(", "))
            }
        }
    ))
}

/// OP-ACTIVITY-03 `activity_delete`. Without `confirm`, ConfirmationRequired with the summary (AP-07). With it, the
/// activity goes with its subtype row, its parties and their email/event rows, its entity links, purposes, comments
/// and the file links that point at it (entity 11); files stay (BR-FILE-03). The karbon_data graph cascades most of
/// these (rels 19, 21, 24, 25, 46, 47, 51, 56, 88, 89); the rest are needed with foreign keys on. It does not delete
/// another activity named by activity_entity.entity_id (rel 95), and does not repeat 515 s79 (Id set to 1).
pub fn activity_delete(conn: &mut Connection, id: &str, confirm: bool, _actor: &Actor) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let summary = delete_summary(&tx, id)?;
    if !confirm {
        return Err(CommandError::ConfirmationRequired(summary));
    }
    delete_in(&tx, id)?;
    tx.commit()?;
    Ok(())
}

/// The rows of one activity, children first (foreign keys are on).
pub fn delete_in(tx: &Connection, id: &str) -> rusqlite::Result<()> {
    for sql in [
        "DELETE FROM activity_party_email WHERE id IN (SELECT id FROM activity_party WHERE activity_id = ?1)",
        "DELETE FROM activity_party_event WHERE id IN (SELECT id FROM activity_party WHERE activity_id = ?1)",
        "DELETE FROM activity_party WHERE activity_id = ?1",
        "DELETE FROM activity_entity WHERE activity_id = ?1",
        "DELETE FROM activity_comment WHERE activity_id = ?1",
        "DELETE FROM file_attachment WHERE entity_type_id = 11 AND entity_id = ?1",
        "UPDATE activity SET activity_purpose_id = NULL WHERE id = ?1",
        "DELETE FROM activity_purpose WHERE activity_id = ?1",
        "DELETE FROM task WHERE id = ?1",
        "DELETE FROM event WHERE id = ?1",
        "DELETE FROM call WHERE id = ?1",
        "DELETE FROM email_message WHERE id = ?1",
        "DELETE FROM discussion WHERE id = ?1",
        "DELETE FROM activity WHERE id = ?1",
    ] {
        tx.execute(sql, [id])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Read: one activity (05 "entity get commands")
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartyRow {
    pub id: String,
    pub role_id: i64,
    pub role: String,
    pub party_id: String,
    pub name: String,
    pub initials: String,
    pub organization: bool,
    pub contact_method_id: String,
    pub email_address: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskInfo {
    pub due_date: String,
    pub due_time: String,
    pub completed_at: String,
    pub completed_by: String,
    pub priority: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventInfo {
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub location: String,
    pub all_day: bool,
    pub meeting: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallInfo {
    pub caller_name: String,
    pub callback_number: String,
    pub result: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmailInfo {
    pub sent: bool,
    pub smtp_timestamp: String,
    pub smtp_result: String,
    pub use_system_settings: bool,
}

/// What an activity is about (activity_entity), with a label and a link where the app has a page for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntityLink {
    pub type_id: i64,
    pub id: String,
    pub label: String,
    pub href: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Activity {
    pub id: String,
    pub type_id: i64,
    pub type_name: String,
    pub subject: String,
    pub message: String,
    pub date_display: String,
    pub time_display: String,
    pub creator: String,
    pub parties_cached: String,
    pub created: String,
    pub comment_count: i64,
    pub task: Option<TaskInfo>,
    pub event: Option<EventInfo>,
    pub call: Option<CallInfo>,
    pub email: Option<EmailInfo>,
    pub discussion: bool,
    pub parties: Vec<PartyRow>,
    pub entities: Vec<EntityLink>,
}

impl Activity {
    pub fn kind(&self) -> Option<Kind> {
        kind_of(self.type_id)
    }
    pub fn open_task(&self) -> bool {
        self.task.as_ref().is_some_and(|t| t.completed_at.is_empty())
    }
    pub fn sent(&self) -> bool {
        self.email.as_ref().is_some_and(|e| e.sent)
    }
}

fn s(v: Option<String>) -> String {
    v.unwrap_or_default()
}

/// The parties of an activity in the order they were added.
pub fn parties_of(conn: &Connection, activity_id: &str) -> rusqlite::Result<Vec<PartyRow>> {
    let mut stmt = conn.prepare(
        "SELECT ap.id, CAST(ap.activity_party_role_id AS INTEGER), COALESCE(r.name, ''), ap.party_id,
                COALESCE(ap.party_name_cached, p.name_cached), ap.party_initials, COALESCE(p.organization_ind, 0) = 1,
                ap.contact_method_id, e.email_address
         FROM activity_party ap
         LEFT JOIN activity_party_role r ON r.id = ap.activity_party_role_id
         LEFT JOIN party p ON p.id = ap.party_id
         LEFT JOIN activity_party_email e ON e.id = ap.id
         WHERE ap.activity_id = ?1 ORDER BY ap.rowid",
    )?;
    let rows = stmt
        .query_map([activity_id], |r| {
            Ok(PartyRow {
                id: r.get(0)?,
                role_id: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                role: r.get(2)?,
                party_id: s(r.get(3)?),
                name: s(r.get(4)?),
                initials: s(r.get(5)?),
                organization: r.get(6)?,
                contact_method_id: s(r.get(7)?),
                email_address: s(r.get(8)?),
            })
        })?
        .collect();
    rows
}

/// The records an activity is about, with a label: a party's name, a sales transaction's doc number, an activity's
/// subject, an opportunity's name.
pub fn entities_of(conn: &Connection, activity_id: &str) -> rusqlite::Result<Vec<EntityLink>> {
    let mut stmt = conn.prepare(
        "SELECT CAST(ae.entity_type_id AS INTEGER), ae.entity_id,
                CASE CAST(ae.entity_type_id AS INTEGER)
                  WHEN 10 THEN (SELECT name_cached FROM party WHERE id = ae.entity_id)
                  WHEN 11 THEN (SELECT subject FROM activity WHERE id = ae.entity_id)
                  WHEN 12 THEN (SELECT doc_number FROM sales_transaction WHERE id = ae.entity_id)
                  WHEN 13 THEN (SELECT name FROM opportunity WHERE id = ae.entity_id)
                END
         FROM activity_entity ae WHERE ae.activity_id = ?1 AND ae.entity_id IS NOT NULL ORDER BY ae.rowid",
    )?;
    let rows = stmt
        .query_map([activity_id], |r| {
            let type_id: i64 = r.get::<_, Option<i64>>(0)?.unwrap_or(0);
            let id: String = r.get(1)?;
            let label: Option<String> = r.get(2)?;
            let href = match type_id {
                10 => format!("/parties/{}", crate::web::urlencode(&id)),
                11 => format!("/activities/{}", crate::web::urlencode(&id)),
                12 => label.as_deref().map(|d| format!("/sales/{}", crate::web::urlencode(d))).unwrap_or_default(),
                _ => String::new(),
            };
            Ok(EntityLink { type_id, id, label: label.unwrap_or_else(|| "(removed)".into()), href })
        })?
        .collect();
    rows
}

/// One activity with its subtype, parties and links (NotFound for an unknown id).
pub fn activity_get(conn: &Connection, id: &str) -> Result<Activity, CommandError> {
    let mut a = conn
        .query_row(
            "SELECT a.id, CAST(a.activity_type_id AS INTEGER), COALESCE(t.description, ''), a.subject, a.message,
                    a.date_display, a.time_display, a.creator_cached, a.parties_cached, a.creation_timestamp,
                    CAST(COALESCE(a.comment_count_cached, 0) AS INTEGER), COALESCE(a.discussion_ind, 0) = 1
             FROM activity a LEFT JOIN activity_type t ON t.id = a.activity_type_id WHERE a.id = ?1",
            [id],
            |r| {
                Ok(Activity {
                    id: r.get(0)?,
                    type_id: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    type_name: r.get(2)?,
                    subject: s(r.get(3)?),
                    message: s(r.get(4)?),
                    date_display: s(r.get(5)?),
                    time_display: s(r.get(6)?),
                    creator: s(r.get(7)?),
                    parties_cached: s(r.get(8)?),
                    created: s(r.get(9)?),
                    comment_count: r.get(10)?,
                    discussion: r.get(11)?,
                    ..Default::default()
                })
            },
        )
        .optional()?
        .ok_or(CommandError::NotFound)?;
    a.task = conn
        .query_row(
            "SELECT due_date, due_time, completed_at, completed_by_cached, priority FROM task WHERE id = ?1",
            [id],
            |r| {
                Ok(TaskInfo {
                    due_date: s(r.get(0)?),
                    due_time: s(r.get(1)?),
                    completed_at: s(r.get(2)?),
                    completed_by: s(r.get(3)?),
                    priority: s(r.get(4)?),
                })
            },
        )
        .optional()?;
    a.event = conn
        .query_row(
            "SELECT start_date, start_time, end_date, end_time, location, COALESCE(all_day_event_ind, 0) = 1,
                    COALESCE(meeting_ind, 0) = 1
             FROM event WHERE id = ?1",
            [id],
            |r| {
                Ok(EventInfo {
                    start_date: s(r.get(0)?),
                    start_time: s(r.get(1)?),
                    end_date: s(r.get(2)?),
                    end_time: s(r.get(3)?),
                    location: s(r.get(4)?),
                    all_day: r.get(5)?,
                    meeting: r.get(6)?,
                })
            },
        )
        .optional()?;
    a.call = conn
        .query_row("SELECT caller_name, callback_number, result FROM call WHERE id = ?1", [id], |r| {
            Ok(CallInfo { caller_name: s(r.get(0)?), callback_number: s(r.get(1)?), result: s(r.get(2)?) })
        })
        .optional()?;
    a.email = conn
        .query_row(
            "SELECT COALESCE(sent_ind, 0) = 1, smtp_timestamp, smtp_result, COALESCE(use_system_settings_ind, 0) = 1
             FROM email_message WHERE id = ?1",
            [id],
            |r| {
                Ok(EmailInfo {
                    sent: r.get(0)?,
                    smtp_timestamp: s(r.get(1)?),
                    smtp_result: s(r.get(2)?),
                    use_system_settings: r.get(3)?,
                })
            },
        )
        .optional()?;
    a.parties = parties_of(conn, id)?;
    a.entities = entities_of(conn, id)?;
    Ok(a)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ACTIVITY-05 task_set_complete (BR-ACTIVITY-04, 09)
// ---------------------------------------------------------------------------------------------------------------

/// OP-ACTIVITY-05 `task_set_complete`. Complete (690 s85-108): completed_at = now (D-29: replaces CompletedInd and
/// AlertInd), the completer's name cached, date_display = today, and a Completed By (51) row for the current user's
/// party (kontacts 758 s3 always passes it). Incomplete (691 s72, s102-109): completed_at and the cache cleared,
/// date_display = the due date or the creation date, the Completed By rows deleted (759 fails when there is none:
/// built as intended). Completing a complete task, or reopening an open one, is refused: layout 91 shows only the
/// button that applies, and a second complete would add a second role-51 row (690 has no guard).
pub fn task_set_complete(conn: &mut Connection, id: &str, complete: bool, actor: &Actor) -> Result<(), CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let state: Option<(Option<String>, Option<String>, String)> = tx
        .query_row(
            "SELECT t.completed_at, t.due_date, COALESCE(substr(a.creation_timestamp, 1, 10), ?2)
             FROM task t JOIN activity a ON a.id = t.id WHERE t.id = ?1",
            params![id, &stamp.at[..10]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (completed_at, due, created) = state.ok_or(CommandError::NotFound)?;
    if complete {
        if completed_at.is_some() {
            return Err(CommandError::Refused("This task is complete already.".into()));
        }
        let me = current_party(&tx, actor)?.ok_or_else(no_party)?;
        let name = party_name(&tx, &me)?;
        tx.execute(
            "UPDATE task SET completed_at = ?2, completed_by_cached = ?3 WHERE id = ?1",
            params![id, stamp.at, name],
        )?;
        tx.execute("UPDATE activity SET date_display = ?2 WHERE id = ?1", params![id, &stamp.at[..10]])?;
        let target = Target::of(&tx, id)?;
        add_party_in(&tx, &target, &PartyInput::new(ActivityPartyRole::CompletedBy, &me), &stamp)?;
    } else {
        if completed_at.is_none() {
            return Err(CommandError::Refused("This task is open already.".into()));
        }
        tx.execute("UPDATE task SET completed_at = NULL, completed_by_cached = NULL WHERE id = ?1", [id])?;
        tx.execute("UPDATE activity SET date_display = ?2 WHERE id = ?1", params![id, due.unwrap_or(created)])?;
        tx.execute(
            "DELETE FROM activity_party WHERE activity_id = ?1 AND CAST(activity_party_role_id AS INTEGER) = 51",
            [id],
        )?;
    }
    refresh_caches(&tx, id)?;
    touch(&tx, id, &stamp)?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ACTIVITY-08 activity_list (BR-ACTIVITY-10)
// ---------------------------------------------------------------------------------------------------------------

/// Rows per page of the activity list (05 query contract: page 100).
pub const LIST_PAGE: i64 = 100;

/// The status filter of 497 (s83-98).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// No status filter (any other value, 497).
    #[default]
    Any,
    /// Omits completed tasks; every other activity counts as incomplete (s83-87).
    Incomplete,
    /// Completed tasks only (s89-91).
    Complete,
    /// date_display on or before today, completed tasks omitted (s93-98).
    Overdue,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s {
            "incomplete" => Status::Incomplete,
            "complete" => Status::Complete,
            "overdue" => Status::Overdue,
            _ => Status::Any,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Status::Any => "",
            Status::Incomplete => "incomplete",
            Status::Complete => "complete",
            Status::Overdue => "overdue",
        }
    }
}

/// The kind chips of the timeline (design/activity.html). 497 has no type filter; this is a screen filter on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    All,
    Discussions,
    Tasks,
    Notes,
    MeetingsAndCalls,
    Email,
}

impl KindFilter {
    pub const ALL: [KindFilter; 6] = [
        KindFilter::All,
        KindFilter::Discussions,
        KindFilter::Tasks,
        KindFilter::Notes,
        KindFilter::MeetingsAndCalls,
        KindFilter::Email,
    ];
    pub fn parse(s: &str) -> Self {
        Self::ALL.into_iter().find(|k| k.key() == s).unwrap_or_default()
    }
    pub fn key(self) -> &'static str {
        match self {
            KindFilter::All => "all",
            KindFilter::Discussions => "disc",
            KindFilter::Tasks => "task",
            KindFilter::Notes => "note",
            KindFilter::MeetingsAndCalls => "meet",
            KindFilter::Email => "email",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            KindFilter::All => "All",
            KindFilter::Discussions => "Discussions",
            KindFilter::Tasks => "Tasks",
            KindFilter::Notes => "Notes",
            KindFilter::MeetingsAndCalls => "Meetings & calls",
            KindFilter::Email => "Email",
        }
    }
    fn type_ids(self) -> &'static [i64] {
        match self {
            KindFilter::All => &[],
            KindFilter::Discussions => &[10],
            KindFilter::Tasks => &[3],
            KindFilter::Notes => &[2],
            KindFilter::MeetingsAndCalls => &[4, 6, 8],
            KindFilter::Email => &[1],
        }
    }
}

/// The activity list query: a party (optional: the timeline shows everyone's), a role, a status, a kind, a page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListQuery {
    pub party_id: Option<String>,
    pub role_id: Option<i64>,
    pub status: Status,
    pub kind: KindFilter,
    /// 1-based.
    pub page: i64,
    pub page_size: i64,
    /// Oldest first (the home task list, layout 101 sorts by date ascending); newest first otherwise (05).
    pub oldest_first: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListRow {
    pub id: String,
    pub type_id: i64,
    pub type_name: String,
    pub subject: String,
    pub message: String,
    pub date_display: String,
    pub time_display: String,
    pub creator: String,
    pub parties: String,
    pub comment_count: i64,
    pub due_date: String,
    pub completed_at: String,
    pub completed_by: String,
    pub is_task: bool,
    pub sent: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListPage {
    pub rows: Vec<ListRow>,
    pub matched: i64,
    pub page: i64,
    pub page_size: i64,
}

/// OP-ACTIVITY-08 `activity_list` (497). A party matches through its activity_party rows (in the given role, or any
/// role when none is given, s78-79), and, with no role given, through an activity_entity link to it, so a
/// discussion or meeting started on the party shows there too (I4 plan Q3: 497 finds by ActivityParty only and the
/// source has no list of activities linked by entity). Distinct activities, newest first, 100 a page (05).
pub fn activity_list(conn: &Connection, q: &ListQuery) -> Result<ListPage, CommandError> {
    let mut wh: Vec<String> = Vec::new();
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    let mut arg = |v: rusqlite::types::Value| {
        args.push(v);
        format!("?{}", args.len())
    };
    if let Some(party) = &q.party_id {
        let p = arg(party.clone().into());
        match q.role_id {
            Some(role) => {
                let r = arg(role.into());
                wh.push(format!(
                    "EXISTS (SELECT 1 FROM activity_party ap WHERE ap.activity_id = a.id AND ap.party_id = {p}
                                                              AND CAST(ap.activity_party_role_id AS INTEGER) = {r})"
                ));
            }
            None => wh.push(format!(
                "(EXISTS (SELECT 1 FROM activity_party ap WHERE ap.activity_id = a.id AND ap.party_id = {p})
                  OR EXISTS (SELECT 1 FROM activity_entity ae WHERE ae.activity_id = a.id
                                AND CAST(ae.entity_type_id AS INTEGER) = 10 AND ae.entity_id = {p}))"
            )),
        }
    } else if let Some(role) = q.role_id {
        let r = arg(role.into());
        wh.push(format!(
            "EXISTS (SELECT 1 FROM activity_party ap WHERE ap.activity_id = a.id
                                                      AND CAST(ap.activity_party_role_id AS INTEGER) = {r})"
        ));
    }
    let done = "EXISTS (SELECT 1 FROM task t WHERE t.id = a.id AND t.completed_at IS NOT NULL)";
    match q.status {
        Status::Any => {}
        Status::Incomplete => wh.push(format!("NOT {done}")),
        Status::Complete => wh.push(done.to_owned()),
        Status::Overdue => {
            let today = arg(crate::audit::local_now()[..10].to_owned().into());
            wh.push(format!("a.date_display <= {today} AND NOT {done}"));
        }
    }
    let types = q.kind.type_ids();
    if !types.is_empty() {
        let list = types.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
        wh.push(format!("CAST(a.activity_type_id AS INTEGER) IN ({list})"));
    }
    let filter = if wh.is_empty() { String::new() } else { format!("WHERE {}", wh.join(" AND ")) };
    let matched: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM activity a {filter}"), rusqlite::params_from_iter(&args), |r| {
            r.get(0)
        })?;
    let size = if q.page_size > 0 { q.page_size } else { LIST_PAGE };
    let page = q.page.max(1);
    let order = if q.oldest_first { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT a.id, CAST(a.activity_type_id AS INTEGER), COALESCE(ty.description, ''), a.subject, a.message,
                a.date_display, a.time_display, a.creator_cached, a.parties_cached,
                CAST(COALESCE(a.comment_count_cached, 0) AS INTEGER), t.due_date, t.completed_at, t.completed_by_cached,
                t.id IS NOT NULL, CASE WHEN e.id IS NULL THEN NULL ELSE COALESCE(e.sent_ind, 0) = 1 END
         FROM activity a
         LEFT JOIN activity_type ty ON ty.id = a.activity_type_id
         LEFT JOIN task t ON t.id = a.id
         LEFT JOIN email_message e ON e.id = a.id
         {filter}
         ORDER BY COALESCE(a.date_display, '') {order}, COALESCE(a.time_display, '') {order}, a.rowid {order}
         LIMIT {size} OFFSET {}",
        (page - 1) * size
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(&args), |r| {
            Ok(ListRow {
                id: r.get(0)?,
                type_id: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                type_name: r.get(2)?,
                subject: s(r.get(3)?),
                message: s(r.get(4)?),
                date_display: s(r.get(5)?),
                time_display: s(r.get(6)?),
                creator: s(r.get(7)?),
                parties: s(r.get(8)?),
                comment_count: r.get(9)?,
                due_date: s(r.get(10)?),
                completed_at: s(r.get(11)?),
                completed_by: s(r.get(12)?),
                is_task: r.get(13)?,
                sent: r.get(14)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ListPage { rows, matched, page, page_size: size })
}

/// Home "Tasks due" (UI-02; layout 101 portal Selected_Task, kontacts 418/365): the open tasks of the current user's
/// party in any role, oldest date first. The status global has no default in the source (365 s14 empties the list);
/// the plan's default is Incomplete (I4 plan Q1).
pub fn tasks_due(conn: &Connection, actor: &Actor, limit: i64) -> Result<ListPage, CommandError> {
    let Some(me) = current_party(conn, actor)? else { return Ok(ListPage::default()) };
    activity_list(
        conn,
        &ListQuery {
            party_id: Some(me),
            status: Status::Incomplete,
            kind: KindFilter::Tasks,
            page_size: limit,
            oldest_first: true,
            ..Default::default()
        },
    )
}

/// The page of an activity: a discussion opens its own (UI-09), every other type the activity page (UI-07).
pub fn page_href(id: &str, type_id: i64) -> String {
    let id = crate::web::urlencode(id);
    match ActivityType::from_id(type_id) {
        Some(ActivityType::Discussion) => format!("/discussions/{id}"),
        _ => format!("/activities/{id}"),
    }
}

/// Whether a refusal text belongs to AlreadySent (used by the routes' 409 answers).
pub fn already_sent_text() -> &'static str {
    ALREADY_SENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_in_memory, Seed};

    fn demo() -> (Connection, Actor) {
        let (conn, _) = open_in_memory(Seed::Demo).unwrap();
        let actor = crate::admin::current_user(&conn).unwrap().expect("the demo has a user");
        (conn, actor)
    }

    fn note(subject: &str) -> NewActivity {
        NewActivity {
            kind: Kind::Note,
            fields: Fields { subject: subject.into(), message: "Body".into(), ..Default::default() },
            entity: None,
            parties: vec![],
        }
    }

    fn flags(conn: &Connection, id: &str) -> (i64, Vec<&'static str>) {
        let cols = ["note_ind", "task_ind", "event_ind", "call_ind", "email_message_ind", "discussion_ind"];
        let type_id: i64 =
            conn.query_row("SELECT activity_type_id FROM activity WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        let set = cols
            .into_iter()
            .filter(|c| {
                conn.query_row(&format!("SELECT COALESCE({c}, 0) FROM activity WHERE id = ?1"), [id], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
                    == 1
            })
            .collect();
        (type_id, set)
    }

    #[test]
    fn each_kind_sets_one_flag_and_its_type() {
        let (mut conn, actor) = demo();
        for (kind, type_id, flag) in [
            (Kind::Note, 2, "note_ind"),
            (Kind::Task, 3, "task_ind"),
            (Kind::Event, 4, "event_ind"),
            (Kind::Meeting, 6, "event_ind"),
            (Kind::Call, 8, "call_ind"),
        ] {
            let mut new = note("Subject");
            new.kind = kind;
            new.fields.start_date = "2026-11-02".into();
            let id = activity_create(&mut conn, &new, &actor).unwrap();
            assert_eq!(flags(&conn, &id), (type_id, vec![flag]), "{kind:?}");
        }
    }

    #[test]
    fn a_meeting_sets_meeting_ind_and_an_event_does_not() {
        let (mut conn, actor) = demo();
        let mut new = note("Walkthrough");
        new.kind = Kind::Meeting;
        new.fields.start_date = "2026-11-02".into();
        new.fields.start_time = "10:00".into();
        let id = activity_create(&mut conn, &new, &actor).unwrap();
        let (meeting, start, shown): (Option<i64>, String, String) = conn
            .query_row(
                "SELECT e.meeting_ind, e.start_time, a.date_display || ' ' || a.time_display
                 FROM event e JOIN activity a ON a.id = e.id WHERE e.id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((meeting, start.as_str(), shown.as_str()), (Some(1), "10:00:00", "2026-11-02 10:00:00"));
        new.kind = Kind::Event;
        let id = activity_create(&mut conn, &new, &actor).unwrap();
        let meeting: Option<i64> =
            conn.query_row("SELECT meeting_ind FROM event WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(meeting, None);
    }

    #[test]
    fn an_event_needs_a_subject_and_a_start_date() {
        let (mut conn, actor) = demo();
        let mut new = note("");
        new.kind = Kind::Event;
        let Err(CommandError::Validation(e)) = activity_create(&mut conn, &new, &actor) else { panic!() };
        let fields: Vec<_> = e.iter().map(|e| e.field).collect();
        assert_eq!(fields, ["subject", "start_date"]);
    }

    #[test]
    fn the_creator_defaults_to_the_current_user_and_a_given_one_is_kept() {
        let (mut conn, actor) = demo();
        let id = activity_create(&mut conn, &note("Mine"), &actor).unwrap();
        let a = activity_get(&conn, &id).unwrap();
        assert_eq!(a.creator, "Mara Ellison");
        assert_eq!(a.parties.len(), 1);
        assert_eq!((a.parties[0].role_id, a.parties[0].initials.as_str()), (50, "ME"));

        let mut given = note("Theirs");
        given.parties = vec![PartyInput::new(ActivityPartyRole::Creator, "p-hp-eh")];
        let id = activity_create(&mut conn, &given, &actor).unwrap();
        assert_eq!(activity_get(&conn, &id).unwrap().creator, "Elena Halden");
    }

    #[test]
    fn a_user_with_no_party_cannot_create() {
        let (mut conn, actor) = demo();
        conn.execute("UPDATE user SET party_id = NULL", []).unwrap();
        let Err(CommandError::Validation(e)) = activity_create(&mut conn, &note("x"), &actor) else { panic!() };
        assert_eq!(e[0].field, "creator");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM activity WHERE subject = 'x'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn defaults_are_today_and_now() {
        let (mut conn, actor) = demo();
        let id = activity_create(&mut conn, &note("Now"), &actor).unwrap();
        let a = activity_get(&conn, &id).unwrap();
        assert_eq!(a.date_display, crate::audit::local_now()[..10]);
        assert_eq!(a.time_display.len(), 8);
    }

    #[test]
    fn every_role_type_pair_follows_the_eligibility_list() {
        let (mut conn, actor) = demo();
        let kinds = [Kind::Note, Kind::Task, Kind::Event, Kind::Meeting, Kind::Call];
        for kind in kinds {
            let mut new = note("Eligibility");
            new.kind = kind;
            new.fields.start_date = "2026-11-02".into();
            let id = activity_create(&mut conn, &new, &actor).unwrap();
            for role in ActivityPartyRole::ALL {
                let r = activity_party_add(&mut conn, &id, &PartyInput::new(*role, "p-hp"), &actor);
                let eligible = role.eligible_for(kind.activity_type().id());
                match r {
                    Ok(_) => assert!(eligible, "{role:?} on {kind:?} should be refused"),
                    Err(CommandError::Validation(e)) => {
                        assert!(!eligible, "{role:?} on {kind:?} should be allowed");
                        assert_eq!(e[0].field, "role");
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        }
    }

    #[test]
    fn a_duplicate_contact_method_in_one_role_is_refused_on_a_task_only() {
        let (mut conn, actor) = demo();
        let cm: String = conn
            .query_row("SELECT id FROM contact_method WHERE party_id = 'p-hp-eh' LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let mut task = note("Task");
        task.kind = Kind::Task;
        let t = activity_create(&mut conn, &task, &actor).unwrap();
        let p = PartyInput {
            role: ActivityPartyRole::Regarding,
            party_id: "p-hp-eh".into(),
            contact_method_id: cm.clone(),
        };
        activity_party_add(&mut conn, &t, &p, &actor).unwrap();
        let Err(CommandError::Validation(e)) = activity_party_add(&mut conn, &t, &p, &actor) else { panic!() };
        assert_eq!(e[0].field, "contact_method_id");
        // A blank contact method is never checked (FileMaker never matches an empty key, 376 s159).
        let blank = PartyInput::new(ActivityPartyRole::Regarding, "p-hp-eh");
        activity_party_add(&mut conn, &t, &blank, &actor).unwrap();
        activity_party_add(&mut conn, &t, &blank, &actor).unwrap();
        // On a note the check does not apply.
        let n = activity_create(&mut conn, &note("Note"), &actor).unwrap();
        activity_party_add(&mut conn, &n, &p, &actor).unwrap();
        activity_party_add(&mut conn, &n, &p, &actor).unwrap();
    }

    #[test]
    fn parties_cached_is_recomputed_without_duplicates() {
        let (mut conn, actor) = demo();
        let id = activity_create(&mut conn, &note("Caches"), &actor).unwrap();
        let a =
            activity_party_add(&mut conn, &id, &PartyInput::new(ActivityPartyRole::Regarding, "p-hp"), &actor).unwrap();
        activity_party_add(&mut conn, &id, &PartyInput::new(ActivityPartyRole::Regarding, "p-bm"), &actor).unwrap();
        let cached = |c: &Connection| activity_get(c, &id).unwrap().parties_cached;
        assert_eq!(cached(&conn), "Halden & Pryce Architects, Bluefin Marine Supply");
        activity_party_edit(&mut conn, &a, &PartyInput::new(ActivityPartyRole::Regarding, "p-hp-eh"), &actor).unwrap();
        assert_eq!(cached(&conn), "Elena Halden, Bluefin Marine Supply");
        activity_party_remove(&mut conn, &a, &actor).unwrap();
        assert_eq!(cached(&conn), "Bluefin Marine Supply");
    }

    #[test]
    fn organization_initials_are_one_letter() {
        assert_eq!(party_initials("Halden & Pryce Architects", true), "H");
        assert_eq!(party_initials("Mara Ellison", false), "ME");
        assert_eq!(party_initials("Ana De La Cruz", false), "ADL");
    }

    #[test]
    fn a_task_stores_its_due_date_and_notifies_the_assignee() {
        let (mut conn, actor) = demo();
        let mut task = note("Send the quote");
        task.kind = Kind::Task;
        task.fields.due_date = "2026-11-05".into();
        task.parties = vec![PartyInput::new(ActivityPartyRole::AssignedTo, "p-hp-eh")];
        let id = activity_create(&mut conn, &task, &actor).unwrap();
        let a = activity_get(&conn, &id).unwrap();
        assert_eq!(a.date_display, "2026-11-05");
        assert_eq!(a.task.as_ref().unwrap().due_date, "2026-11-05");
        let to: String = conn.query_row("SELECT to_cached FROM task WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(to, "Elena Halden");
        let (party, text, etype): (String, String, i64) = conn
            .query_row(
                "SELECT party_id, description, entity_type_id FROM notification WHERE entity_id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((party.as_str(), text.as_str(), etype), ("p-hp-eh", "Task from Mara Ellison - Send the quote", 11));
    }

    #[test]
    fn a_task_for_myself_or_nobody_notifies_nobody_and_is_due_today() {
        let (mut conn, actor) = demo();
        let before: i64 = conn.query_row("SELECT COUNT(*) FROM notification", [], |r| r.get(0)).unwrap();
        let mut task = note("Mine");
        task.kind = Kind::Task;
        let id = activity_create(&mut conn, &task, &actor).unwrap();
        task.parties = vec![PartyInput::new(ActivityPartyRole::AssignedTo, "p-mara")];
        activity_create(&mut conn, &task, &actor).unwrap();
        let after: i64 = conn.query_row("SELECT COUNT(*) FROM notification", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
        assert_eq!(activity_get(&conn, &id).unwrap().task.unwrap().due_date, crate::audit::local_now()[..10]);
    }

    #[test]
    fn the_subject_is_required_once_there_is_content_and_an_empty_one_is_discarded() {
        let (mut conn, actor) = demo();
        let Err(CommandError::Validation(e)) = activity_create(&mut conn, &note(""), &actor) else { panic!() };
        assert_eq!(e[0].field, "subject");
        let id = activity_create(&mut conn, &note("Keep"), &actor).unwrap();
        let empty = Fields::default();
        let with_message = Fields { message: "text".into(), ..Default::default() };
        let Err(CommandError::Validation(e)) = activity_update(&mut conn, &id, &with_message, &actor) else { panic!() };
        assert_eq!(e[0].field, "subject");
        let Err(CommandError::ConfirmationRequired(t)) = activity_update(&mut conn, &id, &empty, &actor) else {
            panic!()
        };
        assert_eq!(t, DISCARD);
        activity_update(&mut conn, &id, &Fields { subject: "New".into(), ..Default::default() }, &actor).unwrap();
        let a = activity_get(&conn, &id).unwrap();
        assert_eq!((a.subject.as_str(), a.message.as_str()), ("New", ""));
    }

    #[test]
    fn a_task_edit_shows_the_due_date_or_the_creation_date() {
        let (mut conn, actor) = demo();
        let f = Fields { subject: "Send revised fit-out quote".into(), ..Default::default() };
        activity_update(&mut conn, "a-task-quote", &f, &actor).unwrap();
        let a = activity_get(&conn, "a-task-quote").unwrap();
        assert_eq!((a.date_display.as_str(), a.task.unwrap().due_date.as_str()), ("2026-10-28", ""));
        let f = Fields { due_date: "2026-11-09".into(), ..f };
        activity_update(&mut conn, "a-task-quote", &f, &actor).unwrap();
        assert_eq!(activity_get(&conn, "a-task-quote").unwrap().date_display, "2026-11-09");
    }

    #[test]
    fn t_08_08_a_sent_email_refuses_every_change() {
        let (mut conn, actor) = demo();
        let before = activity_get(&conn, "a-mail-1048").unwrap();
        assert!(before.sent());
        let f = Fields { subject: "Changed".into(), ..Default::default() };
        assert!(matches!(activity_update(&mut conn, "a-mail-1048", &f, &actor), Err(CommandError::AlreadySent)));
        let p = PartyInput::new(ActivityPartyRole::Cc, "p-hp-eh");
        assert!(matches!(activity_party_add(&mut conn, "a-mail-1048", &p, &actor), Err(CommandError::AlreadySent)));
        let row = before.parties[0].id.clone();
        assert!(matches!(activity_party_remove(&mut conn, &row, &actor), Err(CommandError::AlreadySent)));
        assert_eq!(activity_get(&conn, "a-mail-1048").unwrap(), before);
    }

    #[test]
    fn delete_asks_first_then_removes_every_child_row() {
        let (mut conn, actor) = demo();
        let Err(CommandError::ConfirmationRequired(summary)) =
            activity_delete(&mut conn, "a-disc-light", false, &actor)
        else {
            panic!()
        };
        assert_eq!(
            summary,
            "Deletes the discussion “Lighting spec for the reception area” with its 3 people on it and 2 comments. \
             Attached files stay in Files."
        );
        let mut meeting = note("Walk");
        meeting.kind = Kind::Meeting;
        meeting.fields.start_date = "2026-11-02".into();
        meeting.entity = Some((10, "p-hp".into()));
        meeting.parties = vec![PartyInput::new(ActivityPartyRole::Invited, "p-hp-eh")];
        let m = activity_create(&mut conn, &meeting, &actor).unwrap();
        conn.execute(
            "INSERT INTO file_attachment (id, attachment_id, entity_type_id, entity_id) VALUES ('fa-x', 'f-001', 11, ?1)",
            [&m],
        )
        .unwrap();
        for id in ["a-disc-light", m.as_str()] {
            activity_delete(&mut conn, id, true, &actor).unwrap();
        }
        for (table, col) in [
            ("activity", "id"),
            ("activity_party", "activity_id"),
            ("activity_comment", "activity_id"),
            ("activity_entity", "activity_id"),
            ("discussion", "id"),
            ("event", "id"),
            ("file_attachment", "entity_id"),
        ] {
            let n: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE {col} IN ('a-disc-light', ?1)"), [&m], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(n, 0, "{table}");
        }
        let files: i64 = conn.query_row("SELECT COUNT(*) FROM file WHERE id = 'f-001'", [], |r| r.get(0)).unwrap();
        assert_eq!(files, 1, "files stay");
        let broken: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0)).unwrap();
        assert_eq!(broken, 0);
    }

    #[test]
    fn an_unknown_activity_is_not_found() {
        let (mut conn, actor) = demo();
        assert!(matches!(activity_get(&conn, "nope"), Err(CommandError::NotFound)));
        assert!(matches!(activity_delete(&mut conn, "nope", true, &actor), Err(CommandError::NotFound)));
        let p = PartyInput::new(ActivityPartyRole::Regarding, "p-hp");
        assert!(matches!(activity_party_add(&mut conn, "nope", &p, &actor), Err(CommandError::NotFound)));
    }

    #[test]
    fn a_failing_party_rolls_the_whole_create_back() {
        let (mut conn, actor) = demo();
        let mut new = note("Rollback");
        new.parties = vec![PartyInput::new(ActivityPartyRole::Regarding, "no-such-party")];
        assert!(matches!(activity_create(&mut conn, &new, &actor), Err(CommandError::Validation(_))));
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM activity WHERE subject = 'Rollback'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn complete_and_reopen_round_trip() {
        let (mut conn, actor) = demo();
        task_set_complete(&mut conn, "a-task-quote", true, &actor).unwrap();
        let a = activity_get(&conn, "a-task-quote").unwrap();
        let t = a.task.clone().unwrap();
        assert!(crate::audit::is_timestamp(&t.completed_at));
        assert_eq!(t.completed_by, "Mara Ellison");
        assert_eq!(a.date_display, crate::audit::local_now()[..10]);
        assert_eq!(a.parties.iter().filter(|p| p.role_id == 51).count(), 1);
        assert!(matches!(task_set_complete(&mut conn, "a-task-quote", true, &actor), Err(CommandError::Refused(_))));

        task_set_complete(&mut conn, "a-task-quote", false, &actor).unwrap();
        let a = activity_get(&conn, "a-task-quote").unwrap();
        assert_eq!(a.task.clone().unwrap().completed_at, "");
        assert_eq!(a.date_display, "2026-10-28", "back to the due date");
        assert_eq!(a.parties.iter().filter(|p| p.role_id == 51).count(), 0);
        assert!(matches!(task_set_complete(&mut conn, "a-task-quote", false, &actor), Err(CommandError::Refused(_))));
        assert!(matches!(task_set_complete(&mut conn, "a-meet-walk", true, &actor), Err(CommandError::NotFound)));
    }

    #[test]
    fn reopening_a_task_without_a_due_date_shows_its_creation_date() {
        let (mut conn, actor) = demo();
        conn.execute("UPDATE task SET due_date = NULL WHERE id = 'a-task-quote'", []).unwrap();
        task_set_complete(&mut conn, "a-task-quote", true, &actor).unwrap();
        task_set_complete(&mut conn, "a-task-quote", false, &actor).unwrap();
        assert_eq!(activity_get(&conn, "a-task-quote").unwrap().date_display, "2026-10-28");
    }

    fn ids(page: &ListPage) -> Vec<&str> {
        page.rows.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn the_list_filters_by_party_role_and_status() {
        let (mut conn, actor) = demo();
        let q = |party: &str, status| ListQuery { party_id: Some(party.into()), status, ..Default::default() };
        let all = activity_list(&conn, &q("p-hp", Status::Any)).unwrap();
        assert_eq!(ids(&all), ["a-task-quote", "a-disc-light", "a-meet-walk", "a-mail-1048"], "newest first");
        // Incomplete keeps every activity that is not a completed task (497 s83-87).
        assert_eq!(activity_list(&conn, &q("p-hp", Status::Incomplete)).unwrap().matched, 4);
        assert_eq!(activity_list(&conn, &q("p-hp", Status::Complete)).unwrap().matched, 0);
        task_set_complete(&mut conn, "a-task-quote", true, &actor).unwrap();
        assert_eq!(ids(&activity_list(&conn, &q("p-hp", Status::Complete)).unwrap()), ["a-task-quote"]);
        assert_eq!(activity_list(&conn, &q("p-hp", Status::Incomplete)).unwrap().matched, 3);
        // Overdue: on or before today and not a completed task; the seed's dates are all in the past.
        assert_eq!(activity_list(&conn, &q("p-hp", Status::Overdue)).unwrap().matched, 3);
        let role = ListQuery { role_id: Some(64), ..q("p-hp-eh", Status::Any) };
        assert_eq!(ids(&activity_list(&conn, &role).unwrap()), ["a-disc-light"]);
        let kind = ListQuery { kind: KindFilter::MeetingsAndCalls, ..q("p-hp", Status::Any) };
        assert_eq!(ids(&activity_list(&conn, &kind).unwrap()), ["a-meet-walk"]);
    }

    #[test]
    fn a_meeting_linked_to_a_party_shows_on_it_and_paging_works() {
        let (mut conn, actor) = demo();
        let mut m = note("On site");
        m.kind = Kind::Meeting;
        m.fields.start_date = "2026-11-03".into();
        m.entity = Some((10, "p-bm".into()));
        let id = activity_create(&mut conn, &m, &actor).unwrap();
        let q = ListQuery { party_id: Some("p-bm".into()), ..Default::default() };
        assert!(ids(&activity_list(&conn, &q).unwrap()).contains(&id.as_str()));
        let q = ListQuery { page_size: 2, page: 2, ..Default::default() };
        let page = activity_list(&conn, &q).unwrap();
        assert_eq!((page.rows.len(), page.matched), (2, 5));
    }

    #[test]
    fn tasks_due_are_my_open_tasks_oldest_first() {
        let (mut conn, actor) = demo();
        let mut t = note("Later task");
        t.kind = Kind::Task;
        t.fields.due_date = "2026-12-01".into();
        let later = activity_create(&mut conn, &t, &actor).unwrap();
        let due = tasks_due(&conn, &actor, 10).unwrap();
        assert_eq!(ids(&due), ["a-task-quote", later.as_str()]);
        task_set_complete(&mut conn, "a-task-quote", true, &actor).unwrap();
        assert_eq!(ids(&tasks_due(&conn, &actor, 10).unwrap()), [later.as_str()]);
    }
}
