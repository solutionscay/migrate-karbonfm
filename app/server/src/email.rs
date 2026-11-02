//! Email messages (05 OP-ACTIVITY-01/02 on an email, OP-INTEG-01 `email_send`; 07 BR-ACTIVITY-08, BR-INTEG-01; 08
//! AP-02, AP-03, AP-08). Sources: kontroller 389 (create), 522 (edit), 521 and 337 (send), kontacts 333 / 775 (the
//! send buttons), 343 / 346 (apply a template).
//!
//! The send reads what it needs under the database lock, releases it for the SMTP session, then takes it again to
//! mark the message sent, so the window stays usable while the server talks to the mail host. A failure writes
//! nothing: the message stays unsent (521 s119-121 and End Transaction 31 s85-86 revert the record).

use crate::activities::{self, Fields, Kind, NewActivity, PartyInput};
use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::net::OutboundPolicy;
use crate::reference::ActivityPartyRole;
use crate::secrets::{Secret, SecretKey, SecretStore};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::fmt;
use std::sync::Mutex;

fn field_error(field: &'static str, message: impl Into<String>) -> CommandError {
    CommandError::Validation(vec![FieldError { field, message: message.into() }])
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

// ---------------------------------------------------------------------------------------------------------------
// Create, edit, apply a template
// ---------------------------------------------------------------------------------------------------------------

/// A new email message (389).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewEmail {
    pub subject: String,
    pub message: String,
    /// The record it is about (activity_entity), e.g. an invoice (12) or a party (10).
    pub entity: Option<(i64, String)>,
    /// The To (55) parties: (party id, contact method id of the email address, may be empty).
    pub to: Vec<(String, String)>,
    /// use_system_settings_ind: send through the system account instead of the user's (521 s86).
    pub use_system_settings: bool,
}

/// OP-ACTIVITY-01 for an email (389): the activity (type 1, email_message_ind), the email_message row (sent_ind 0:
/// "New messages default to Send = false", s65; use_system_settings_ind), the Creator and, when no From is given,
/// the current user's party as From (54) with `user.smtp_email_address` as its address (s134-153). A user with no
/// party is refused (s142-144).
pub fn email_create(conn: &mut Connection, new: &NewEmail, actor: &Actor) -> Result<String, CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let me = activities::current_party(&tx, actor)?
        .ok_or_else(|| field_error("creator", "Your user is not linked to a person, so the email has no sender."))?;
    let my_address: Option<String> = match &actor.user_id {
        Some(u) => {
            tx.query_row("SELECT smtp_email_address FROM user WHERE id = ?1", [u], |r| r.get(0)).optional()?.flatten()
        }
        None => None,
    };
    let mut parties = vec![PartyInput {
        role: ActivityPartyRole::From,
        party_id: me,
        contact_method_id: String::new(),
        email_address: my_address.unwrap_or_default(),
    }];
    for (party, cm) in &new.to {
        parties.push(PartyInput {
            role: ActivityPartyRole::To,
            party_id: party.clone(),
            contact_method_id: cm.clone(),
            email_address: String::new(),
        });
    }
    let activity = NewActivity {
        kind: Kind::Email,
        fields: Fields { subject: new.subject.clone(), message: new.message.clone(), ..Default::default() },
        entity: new.entity.clone(),
        parties: vec![],
    };
    let id = activities::create_in(&tx, &activity, &stamp)?;
    tx.execute(
        "INSERT INTO email_message (id, sent_ind, use_system_settings_ind) VALUES (?1, 0, ?2)",
        params![id, i64::from(new.use_system_settings)],
    )?;
    let target = activities::Target::of(&tx, &id)?;
    for p in &parties {
        activities::add_party_in(&tx, &target, p, &stamp)?;
    }
    activities::refresh_caches(&tx, &id)?;
    tx.commit()?;
    Ok(id)
}

/// OP-ACTIVITY-02 for an email (522): subject, message and the account choice. A sent message is refused with
/// AlreadySent (s123-126, AP-08). The date and time stay as they are.
pub fn email_update(
    conn: &mut Connection,
    id: &str,
    subject: &str,
    message: &str,
    use_system_settings: bool,
    actor: &Actor,
) -> Result<(), CommandError> {
    let a = activities::activity_get(conn, id)?;
    if a.email.is_none() {
        return Err(CommandError::NotFound);
    }
    let fields = Fields {
        subject: subject.into(),
        message: message.into(),
        date: a.date_display.clone(),
        time: a.time_display.clone(),
        ..Default::default()
    };
    activities::activity_update(conn, id, &fields, actor)?;
    let stamp = Stamp::now(actor);
    conn.execute(
        "UPDATE email_message SET use_system_settings_ind = ?2 WHERE id = ?1",
        params![id, i64::from(use_system_settings)],
    )?;
    audit::stamp_modified(conn, "activity", &id, &stamp)?;
    Ok(())
}

/// Apply a template to an email (kontacts 343 s6, 346 s5-11; 333 s72): the merged message (OP-TEMPLATE-01) replaces
/// the message and `activity.template_id` is set; the subject does not change (343 copies the message only). A sent
/// message is refused (AP-08). 346 writes the result even when the merge failed: not built.
pub fn email_apply_template(
    conn: &mut Connection,
    id: &str,
    template_id: i64,
    actor: &Actor,
) -> Result<(), CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let target = activities::Target::of(&tx, id)?;
    let is_email: bool =
        tx.query_row("SELECT EXISTS (SELECT 1 FROM email_message WHERE id = ?1)", [id], |r| r.get(0))?;
    if !is_email {
        return Err(CommandError::NotFound);
    }
    if target.sent {
        return Err(CommandError::AlreadySent);
    }
    let merged = crate::templates::template_merge(&tx, template_id, id).map_err(|e| match e {
        CommandError::NotFound => field_error("template_id", "Choose a template."),
        e => e,
    })?;
    tx.execute(
        "UPDATE activity SET message = ?2, template_id = ?3, last_updated = ?4 WHERE id = ?1",
        params![id, opt(&merged.message), template_id, stamp.at],
    )?;
    audit::stamp_modified(&tx, "activity", &id, &stamp)?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-INTEG-01 email_send (BR-INTEG-01)
// ---------------------------------------------------------------------------------------------------------------

/// The connection security of the account (the settings' Encryption). 337 s123-124 read it; s160 then sent with
/// "Use TLS" whatever it said, a shortcut the target does not copy (I4 plan E7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encryption {
    None,
    StartTls,
    Tls,
}

impl Encryption {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "None" => Some(Encryption::None),
            "STARTTLS" => Some(Encryption::StartTls),
            "SSL/TLS" => Some(Encryption::Tls),
            _ => None,
        }
    }
}

/// Which account sends (521 s86: the message's use_system_settings_ind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Account {
    System,
    User,
}

impl Account {
    pub fn secret(self) -> SecretKey {
        match self {
            Account::System => SecretKey::SystemSmtpPassword,
            Account::User => SecretKey::UserSmtpPassword,
        }
    }
}

/// Everything the SMTP session needs. The password is a [`Secret`]: no Display, no Serialize (AP-03).
pub struct Outgoing {
    pub account: Account,
    pub host: String,
    pub port: u16,
    pub encryption: Encryption,
    /// (user name, password) when the account authenticates with a password.
    pub credentials: Option<(String, Secret)>,
    /// `"Full Name" <address>` is built from these (337 s135).
    pub from_name: String,
    pub from_address: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
}

impl fmt::Debug for Outgoing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Outgoing")
            .field("account", &self.account)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("to", &self.to)
            .finish_non_exhaustive()
    }
}

/// Why a send failed. The text never holds the password (lettre's errors do not carry it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The server refused the user name and password (535; FileMaker error 1507 in kontacts 775).
    Auth(String),
    /// Anything else: no connection, TLS, a 4xx/5xx reply, a refused recipient.
    Other(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Auth(m) => write!(f, "the mail server refused the sign-in: {m}"),
            SendError::Other(m) => write!(f, "the email could not be sent: {m}"),
        }
    }
}

/// Sends one message and returns the server's final reply text. Production: [`LettreMailer`]; tests may use a
/// local capture server through it too.
pub trait Mailer: Send + Sync {
    fn send(&self, message: &Outgoing) -> Result<String, SendError>;
}

/// The settings of one account as read from the database (system_setting or the user's row with its fallbacks).
#[derive(Debug, Clone, Default)]
struct AccountRow {
    address: Option<String>,
    full_name: Option<String>,
    user_name: Option<String>,
    server: Option<String>,
    port: Option<String>,
    encryption: Option<String>,
    authentication: Option<String>,
}

fn system_account(conn: &Connection) -> rusqlite::Result<AccountRow> {
    Ok(conn
        .query_row(
            "SELECT system_email_address, system_email_full_name, system_email_user_name, system_email_smtp_server,
                    CAST(system_email_smtp_server_port AS TEXT), system_email_smtp_server_encryption_type,
                    system_email_smtp_server_authentication
             FROM system_setting WHERE singleton = 1",
            [],
            |r| {
                Ok(AccountRow {
                    address: r.get(0)?,
                    full_name: r.get(1)?,
                    user_name: r.get(2)?,
                    server: r.get(3)?,
                    port: r.get(4)?,
                    encryption: r.get(5)?,
                    authentication: r.get(6)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

/// The user's account (337 s84-89): address, full name and user name from the user's row; server, authentication,
/// encryption and port each fall back to the system account when the user's value is empty (s85-88).
fn user_account(conn: &Connection, actor: &Actor) -> rusqlite::Result<AccountRow> {
    let system = system_account(conn)?;
    let mine = match &actor.user_id {
        Some(u) => conn
            .query_row(
                "SELECT smtp_email_address, smtp_full_name, smtp_user_name, smtp_server, smtp_server_port,
                        smtp_server_encryption_type, smtp_server_authentication
                 FROM user WHERE id = ?1",
                [u],
                |r| {
                    Ok(AccountRow {
                        address: r.get(0)?,
                        full_name: r.get(1)?,
                        user_name: r.get(2)?,
                        server: r.get(3)?,
                        port: r.get(4)?,
                        encryption: r.get(5)?,
                        authentication: r.get(6)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default(),
        None => AccountRow::default(),
    };
    let or = |a: Option<String>, b: Option<String>| a.filter(|s| !s.trim().is_empty()).or(b);
    Ok(AccountRow {
        address: mine.address,
        full_name: mine.full_name,
        user_name: mine.user_name,
        server: or(mine.server, system.server),
        port: or(mine.port, system.port),
        encryption: or(mine.encryption, system.encryption),
        authentication: or(mine.authentication, system.authentication),
    })
}

/// The addresses of one role on the message: activity_party_email.email_address (521 s100-109).
fn addresses(conn: &Connection, id: &str, role: ActivityPartyRole) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT e.email_address FROM activity_party ap JOIN activity_party_email e ON e.id = ap.id
         WHERE ap.activity_id = ?1 AND CAST(ap.activity_party_role_id AS INTEGER) = ?2
           AND COALESCE(TRIM(e.email_address), '') <> ''
         ORDER BY ap.rowid",
    )?;
    let rows = stmt.query_map(params![id, role.id()], |r| r.get::<_, String>(0))?.collect();
    rows
}

/// What a send needs, read under the lock. AlreadySent for a sent message (AP-08: the source checks only on edit,
/// 522 s123-126; 08 refuses a re-send too). The required items of 337 s103 are field errors and no connection is
/// made: message, subject, at least one To, sender address, user name (with Password authentication), server, port,
/// and the password, which comes from the keychain (AP-03), or from `password` when the send dialog asked for it.
pub fn prepare_send(
    conn: &Connection,
    secrets: &dyn SecretStore,
    id: &str,
    actor: &Actor,
    password: Option<&Secret>,
) -> Result<Outgoing, CommandError> {
    let a = activities::activity_get(conn, id)?;
    let email = a.email.as_ref().ok_or(CommandError::NotFound)?;
    if email.sent {
        return Err(CommandError::AlreadySent);
    }
    let account = if email.use_system_settings { Account::System } else { Account::User };
    let row = match account {
        Account::System => system_account(conn)?,
        Account::User => user_account(conn, actor)?,
    };
    let to = addresses(conn, id, ActivityPartyRole::To)?;
    let cc = addresses(conn, id, ActivityPartyRole::Cc)?;
    let bcc = addresses(conn, id, ActivityPartyRole::Bcc)?;
    let text = |o: &Option<String>| o.as_deref().map(str::trim).unwrap_or("").to_owned();
    let mut errors = Vec::new();
    let mut need = |ok: bool, field: &'static str, message: &str| {
        if !ok {
            errors.push(FieldError { field, message: message.into() });
        }
    };
    need(!a.subject.trim().is_empty(), "subject", "Enter a subject.");
    need(!a.message.trim().is_empty(), "message", "Write the message.");
    need(!to.is_empty(), "to", "Add at least one recipient with an email address in To.");
    let where_ = if account == Account::User { "your email account" } else { "the system email account" };
    need(!text(&row.address).is_empty(), "sender", &format!("Set the email address of {where_} in Settings."));
    need(!text(&row.server).is_empty(), "smtp_server", &format!("Set the SMTP server of {where_} in Settings."));
    let port = text(&row.port).parse::<u16>().ok().filter(|p| *p > 0);
    need(port.is_some(), "smtp_port", &format!("Set the SMTP port of {where_} in Settings."));
    let encryption = Encryption::parse(&text(&row.encryption)).unwrap_or(Encryption::StartTls);
    let password_auth = text(&row.authentication) != "None";
    let mut credentials = None;
    if password_auth {
        let user = text(&row.user_name);
        need(!user.is_empty(), "user_name", &format!("Set the SMTP user name of {where_} in Settings."));
        let secret = match password {
            Some(p) => Some(p.clone()),
            None => secrets.get(account.secret()).map_err(|e| CommandError::Refused(e.to_string()))?,
        };
        match secret {
            Some(s) if !s.expose().is_empty() => credentials = Some((user, s)),
            _ => need(false, "password", &format!("Enter the SMTP password of {where_}.")),
        }
    }
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }
    Ok(Outgoing {
        account,
        host: text(&row.server),
        port: port.unwrap_or(587),
        encryption,
        credentials,
        from_name: text(&row.full_name),
        from_address: text(&row.address),
        to,
        cc,
        bcc,
        subject: a.subject.trim().to_owned(),
        body: a.message,
    })
}

/// The outbound policy of AP-02 for email: only the SMTP hosts of the settings (the system account's and the
/// user's).
pub fn smtp_policy(conn: &Connection, actor: &Actor) -> rusqlite::Result<OutboundPolicy> {
    let mut policy = OutboundPolicy::deny_all();
    for row in [system_account(conn)?, user_account(conn, actor)?] {
        if let Some(host) = row.server.filter(|h| !h.trim().is_empty()) {
            policy = policy.smtp_host(&host);
        }
    }
    Ok(policy)
}

/// Marks the message sent (521 s92, s125): sent_ind = 1, smtp_timestamp now, and smtp_result = the server's final
/// reply (07 BR-INTEG-01 "records the send result"; no source script writes SmtpResult, table 168 #8). The activity
/// date is not changed.
pub fn mark_sent(conn: &mut Connection, id: &str, reply: &str, actor: &Actor) -> Result<(), CommandError> {
    let stamp = Stamp::now(actor);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let n = tx.execute(
        "UPDATE email_message SET sent_ind = 1, smtp_timestamp = ?2, smtp_result = ?3
         WHERE id = ?1 AND COALESCE(sent_ind, 0) = 0",
        params![id, stamp.at, reply],
    )?;
    if n == 0 {
        return Err(CommandError::AlreadySent);
    }
    tx.execute("UPDATE activity SET last_updated = ?2 WHERE id = ?1", params![id, stamp.at])?;
    audit::stamp_modified(&tx, "activity", &id, &stamp)?;
    tx.commit()?;
    Ok(())
}

/// The result of [`email_send`].
#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    /// Sent; the server's final reply.
    Sent(String),
    /// The SMTP session failed; nothing was written. `forgot_password`: the user's password was removed from the
    /// keychain after an authentication failure (kontacts 775 s18-29), so the next send asks for it.
    Failed { error: SendError, forgot_password: bool },
}

/// OP-INTEG-01 `email_send` (BR-INTEG-01): prepare under the lock, check the host against `policy` (AP-02; a refused
/// host is Refused and nothing connects), send without the lock, then mark it sent. A failure is one masked log line
/// (code Integration; 03 EXT-01 "no retry; failure leaves message unsent and logged"). A password typed in the send
/// dialog (`password`, kontacts 775 s4-9) is saved to the keychain for the user account once the send succeeds.
pub fn email_send(
    db: &Mutex<Connection>,
    secrets: &dyn SecretStore,
    mailer: &dyn Mailer,
    policy: Option<&OutboundPolicy>,
    id: &str,
    actor: &Actor,
    password: Option<&Secret>,
) -> Result<SendOutcome, CommandError> {
    let lock = || db.lock().map_err(|e| CommandError::Refused(format!("database lock: {e}")));
    let (outgoing, policy) = {
        let conn = lock()?;
        let out = prepare_send(&conn, secrets, id, actor, password)?;
        let policy = match policy {
            Some(p) => p.clone(),
            None => smtp_policy(&conn, actor)?,
        };
        (out, policy)
    };
    if let Err(refused) = policy.check_smtp(&outgoing.host) {
        return Err(CommandError::Refused(refused.to_string()));
    }
    match mailer.send(&outgoing) {
        Ok(reply) => {
            mark_sent(&mut *lock()?, id, &reply, actor)?;
            if let (Some(p), Account::User) = (password, outgoing.account) {
                // Kept only after it worked; a keychain failure here does not undo the send.
                let _ = secrets.set(SecretKey::UserSmtpPassword, p);
            }
            Ok(SendOutcome::Sent(reply))
        }
        Err(error) => {
            let message = format!("{} (host {})", error, outgoing.host);
            crate::applog::log_write(&crate::applog::Entry {
                level: crate::applog::Level::Warn,
                operation: "email_send",
                code: Some("Integration"),
                message: &message,
                payload: None,
            });
            let forgot_password = matches!(error, SendError::Auth(_))
                && outgoing.account == Account::User
                && password.is_none()
                && secrets.delete(SecretKey::UserSmtpPassword).is_ok();
            Ok(SendOutcome::Failed { error, forgot_password })
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The production mailer (lettre)
// ---------------------------------------------------------------------------------------------------------------

/// Sends through lettre's blocking SMTP transport. Plain text only (337 s160 sends the plain message; message_html
/// is never sent), no attachments (521 s115 never passes them). The From header is `"Full Name" <account address>`
/// (337 s135); Bcc goes in the envelope only; the Message-ID is a new id on the sender's domain.
pub struct LettreMailer {
    /// Seconds before a connection or reply times out; the source sets none (I4 plan Q5: lettre's default).
    pub timeout: Option<std::time::Duration>,
}

impl Default for LettreMailer {
    fn default() -> Self {
        Self { timeout: Some(std::time::Duration::from_secs(60)) }
    }
}

/// The message as lettre builds it (headers and body), for the mailer and for tests.
pub fn build_message(m: &Outgoing) -> Result<lettre::Message, SendError> {
    use lettre::message::{header::ContentType, Mailbox};
    let bad = |what: &str, v: &str| SendError::Other(format!("{what} is not a valid email address: {v}"));
    let from_addr: lettre::Address = m.from_address.parse().map_err(|_| bad("The sender", &m.from_address))?;
    let name = (!m.from_name.is_empty()).then(|| m.from_name.clone());
    // A Message-ID on the sender's domain (B12): servers may score a message without one as spam, and lettre's own
    // would carry the machine's host name.
    let message_id = format!("<{}@{}>", crate::ids::new_id(), from_addr.domain());
    let mut b = lettre::Message::builder()
        .from(Mailbox::new(name, from_addr))
        .subject(m.subject.clone())
        .message_id(Some(message_id));
    for t in &m.to {
        b = b.to(t.parse::<Mailbox>().map_err(|_| bad("A To recipient", t))?);
    }
    for c in &m.cc {
        b = b.cc(c.parse::<Mailbox>().map_err(|_| bad("A Cc recipient", c))?);
    }
    for c in &m.bcc {
        b = b.bcc(c.parse::<Mailbox>().map_err(|_| bad("A Bcc recipient", c))?);
    }
    b.header(ContentType::TEXT_PLAIN).body(m.body.clone()).map_err(|e| SendError::Other(e.to_string()))
}

impl Mailer for LettreMailer {
    fn send(&self, m: &Outgoing) -> Result<String, SendError> {
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::transport::smtp::client::{Tls, TlsParameters};
        use lettre::{SmtpTransport, Transport};
        let message = build_message(m)?;
        let tls = |host: &str| TlsParameters::new(host.to_owned()).map_err(|e| SendError::Other(e.to_string()));
        let mut t = SmtpTransport::builder_dangerous(m.host.as_str()).port(m.port).timeout(self.timeout);
        t = match m.encryption {
            Encryption::None => t.tls(Tls::None),
            Encryption::StartTls => t.tls(Tls::Required(tls(&m.host)?)),
            Encryption::Tls => t.tls(Tls::Wrapper(tls(&m.host)?)),
        };
        if let Some((user, password)) = &m.credentials {
            t = t.credentials(Credentials::new(user.clone(), password.expose().to_owned()));
        }
        match t.build().send(&message) {
            Ok(response) => {
                let text = response.message().collect::<Vec<_>>().join(" ");
                Ok(format!("{} {}", response.code(), text).trim().to_owned())
            }
            Err(e) => {
                let auth = e.status().is_some_and(|c| c.to_string() == "535") || e.to_string().contains("535");
                if auth {
                    Err(SendError::Auth(e.to_string()))
                } else {
                    Err(SendError::Other(e.to_string()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_in_memory, Seed};
    use crate::secrets::MemoryStore;

    fn demo() -> (Connection, Actor) {
        let (conn, _) = open_in_memory(Seed::Demo).unwrap();
        let actor = crate::admin::current_user(&conn).unwrap().unwrap();
        (conn, actor)
    }

    fn elena_email(conn: &Connection) -> String {
        conn.query_row(
            "SELECT cm.id FROM contact_method cm JOIN network_address n ON n.id = cm.id
             WHERE cm.party_id = 'p-hp-eh' AND n.email_address_ind = 1",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn create_adds_me_as_from_and_starts_unsent() {
        let (mut conn, actor) = demo();
        conn.execute("UPDATE user SET smtp_email_address = 'mara@ellisonstudio.example'", []).unwrap();
        let cm = elena_email(&conn);
        let new = NewEmail {
            subject: "Lighting".into(),
            message: "Hi Elena".into(),
            to: vec![("p-hp-eh".into(), cm)],
            ..Default::default()
        };
        let id = email_create(&mut conn, &new, &actor).unwrap();
        let a = activities::activity_get(&conn, &id).unwrap();
        assert_eq!((a.type_id, a.sent()), (1, false));
        let roles: Vec<(i64, &str)> = a.parties.iter().map(|p| (p.role_id, p.email_address.as_str())).collect();
        assert_eq!(roles[0], (50, ""));
        assert_eq!(roles[1], (54, "mara@ellisonstudio.example"));
        assert_eq!(roles[2].0, 55);
        assert!(roles[2].1.contains('@'), "the To address comes from the contact method");
        let flag: i64 =
            conn.query_row("SELECT email_message_ind FROM activity WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(flag, 1);
    }

    #[test]
    fn prepare_lists_every_missing_item_without_connecting() {
        let (mut conn, actor) = demo();
        let id = email_create(&mut conn, &NewEmail::default(), &actor).unwrap();
        let Err(CommandError::Validation(e)) = prepare_send(&conn, &MemoryStore::default(), &id, &actor, None) else {
            panic!()
        };
        let fields: Vec<_> = e.iter().map(|e| e.field).collect();
        for f in ["subject", "message", "to", "sender", "smtp_server", "smtp_port", "user_name", "password"] {
            assert!(fields.contains(&f), "{f} in {fields:?}");
        }
    }

    #[test]
    fn the_user_account_falls_back_to_the_system_server_and_port() {
        let (mut conn, actor) = demo();
        conn.execute(
            "UPDATE system_setting SET system_email_smtp_server = 'smtp.studio.example', system_email_smtp_server_port = 2525,
                    system_email_smtp_server_encryption_type = 'None', system_email_smtp_server_authentication = 'Password'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE user SET smtp_email_address = 'mara@ellisonstudio.example', smtp_full_name = 'Mara Ellison',
                    smtp_user_name = 'mara'",
            [],
        )
        .unwrap();
        let cm = elena_email(&conn);
        let new = NewEmail {
            subject: "S".into(),
            message: "M".into(),
            to: vec![("p-hp-eh".into(), cm)],
            ..Default::default()
        };
        let id = email_create(&mut conn, &new, &actor).unwrap();
        let store = MemoryStore::default();
        store.set(SecretKey::UserSmtpPassword, &Secret::new("pw")).unwrap();
        let out = prepare_send(&conn, &store, &id, &actor, None).unwrap();
        assert_eq!(
            (out.host.as_str(), out.port, out.encryption, out.account),
            ("smtp.studio.example", 2525, Encryption::None, Account::User)
        );
        assert_eq!(out.from_name, "Mara Ellison");
        assert_eq!(out.credentials.as_ref().unwrap().0, "mara");
        let debug = format!("{out:?}");
        assert!(!debug.contains("pw"), "no password in Debug: {debug}");
    }

    #[test]
    fn apply_template_replaces_the_message_and_keeps_the_subject() {
        let (mut conn, actor) = demo();
        conn.execute(
            "INSERT INTO template (id, name, subject, message) VALUES (90, 'T', 'Template subject', 'Hello {{FirstName}}')",
            [],
        )
        .unwrap();
        let cm = elena_email(&conn);
        let new = NewEmail { subject: "Mine".into(), to: vec![("p-hp-eh".into(), cm)], ..Default::default() };
        let id = email_create(&mut conn, &new, &actor).unwrap();
        email_apply_template(&mut conn, &id, 90, &actor).unwrap();
        let (subject, message, template): (String, String, i64) = conn
            .query_row("SELECT subject, message, template_id FROM activity WHERE id = ?1", [&id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((subject.as_str(), message.as_str(), template), ("Mine", "Hello Elena", 90));
        assert!(matches!(email_apply_template(&mut conn, "a-mail-1048", 90, &actor), Err(CommandError::AlreadySent)));
        assert!(matches!(email_apply_template(&mut conn, &id, 999, &actor), Err(CommandError::Validation(_))));
    }

    #[test]
    fn the_message_is_plain_text_from_the_account_with_bcc_in_the_envelope_only() {
        let out = Outgoing {
            account: Account::System,
            host: "h".into(),
            port: 25,
            encryption: Encryption::None,
            credentials: None,
            from_name: "Ellison Studio".into(),
            from_address: "billing@ellisonstudio.example".into(),
            to: vec!["elena@hp.example".into()],
            cc: vec!["marcus@hp.example".into()],
            bcc: vec!["files@ellisonstudio.example".into()],
            subject: "Invoice".into(),
            body: "Hello".into(),
        };
        let m = build_message(&out).unwrap();
        let envelope: Vec<String> = m.envelope().to().iter().map(|a| a.to_string()).collect();
        assert_eq!(envelope, ["elena@hp.example", "marcus@hp.example", "files@ellisonstudio.example"]);
        let text = String::from_utf8(m.formatted()).unwrap();
        assert!(text.contains("From: \"Ellison Studio\" <billing@ellisonstudio.example>"), "{text}");
        assert!(text.contains("Content-Type: text/plain"));
        assert!(!text.to_lowercase().contains("bcc:"), "Bcc is not a header: {text}");
    }
}
