//! ADMIN commands of I1: first-run setup (OP-ADMIN-01 `setup_welcome`, BR-ADMIN-01), the system settings
//! (OP-ADMIN-03 `settings_get_update`, BR-ADMIN-05) and the single local user (OP-ADMIN-04 `current_user`,
//! BR-ADMIN-04). Plain SQL; each write command is one `BEGIN IMMEDIATE` transaction and stamps every row it writes
//! (BR-PARTY-07). Form parsing and rendering are in `routes::setup` and `routes::settings`.

use crate::audit::{self, Actor, Stamp};
use crate::ids::new_id;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::fmt;

/// A field error to show next to the field (D-25: Validation re-renders the form).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub enum CommandError {
    Validation(Vec<FieldError>),
    /// Setup ran already (or the database has a user): setup never runs twice.
    AlreadySetUp,
    /// The record the command names does not exist (05: 404 for a record, an empty list for a search).
    NotFound,
    /// A create named an id that exists already (BR-PARTY-10: "use Edit Party instead"); carries that id.
    AlreadyExists(String),
    /// A destructive command ran without `confirm` (AP-07); carries the summary for the confirmation dialog.
    ConfirmationRequired(String),
    /// The command is refused by a rule (409), like a duplicate relation pair (BR-PARTY-03); carries the message.
    Refused(String),
    /// An email message that was sent already cannot be edited or sent again (AP-08, BR-ACTIVITY-08; T-08-08).
    AlreadySent,
    /// A posted transaction cannot be changed or deleted; unpost it first (AP-05, BR-SALES-05; T-08-05).
    Locked,
    Storage(rusqlite::Error),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Validation(errors) => write!(f, "validation: {} field errors", errors.len()),
            CommandError::AlreadySetUp => f.write_str("setup is already done"),
            CommandError::NotFound => f.write_str("not found"),
            CommandError::AlreadyExists(id) => write!(f, "{id} exists already"),
            CommandError::ConfirmationRequired(summary) => write!(f, "confirmation required: {summary}"),
            CommandError::Refused(message) => f.write_str(message),
            CommandError::AlreadySent => f.write_str(ALREADY_SENT),
            CommandError::Locked => f.write_str(LOCKED),
            CommandError::Storage(e) => write!(f, "database: {e}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// The AlreadySent message (kontroller 522 s124-125: "This Email has already been sent").
pub const ALREADY_SENT: &str = "This email has already been sent. A sent email cannot be changed or sent again.";

/// The Locked message (kontroller 560 s56: "This transaction has been posted. You have to Un Post it...").
pub const LOCKED: &str = "This transaction has been posted. Unpost it first.";

impl From<rusqlite::Error> for CommandError {
    fn from(e: rusqlite::Error) -> Self {
        CommandError::Storage(e)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ADMIN-04 current_user
// ---------------------------------------------------------------------------------------------------------------

/// The single local user as the actor of every command (BR-ADMIN-04). `None` before setup.
pub fn current_user(conn: &Connection) -> rusqlite::Result<Option<Actor>> {
    // The user an import mapped as "me" (app_meta.local_user, UI-18 step 2), else the first user.
    let sql = "SELECT id, COALESCE(account_name, '') FROM user \
               ORDER BY id IS NOT (SELECT value FROM app_meta WHERE key = 'local_user'), creation_timestamp, id LIMIT 1";
    conn.query_row(sql, [], |r| Ok(Actor::user(r.get::<_, String>(0)?, r.get::<_, String>(1)?))).optional()
}

/// True once setup has run (system_setting.initial_setup_done_ind = 1). The demo seed sets it, so a demo database
/// counts as set up.
pub fn setup_done(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COALESCE((SELECT initial_setup_done_ind FROM system_setting WHERE singleton = 1), 0) = 1",
        [],
        |r| r.get(0),
    )
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ADMIN-01 setup_welcome
// ---------------------------------------------------------------------------------------------------------------

/// What setup collects: organization -> person -> settings (UI-01). Already validated by [`SetupInput::validate`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupInput {
    pub organization_name: String,
    pub first_name: String,
    pub last_name: String,
    pub job_title: Option<String>,
    pub email: Option<String>,
    pub tax_rate_bp: i64,
    pub country_code: String,
    pub area_code: Option<i64>,
    pub format_phone_numbers: bool,
    /// user.account_name: the OS account name (FileMaker used Get(AccountName)).
    pub account_name: String,
}

/// The ids setup created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupDone {
    pub organization_id: String,
    pub person_id: String,
    pub employee_role_id: String,
    pub user: Actor,
}

/// BR-ADMIN-01, one transaction: the organization (set as system owner), the first person as an employee of it, the
/// single user linked to that person, and the settings row with initial_setup_done_ind = 1. Every row is stamped with
/// the new user. A failure rolls back everything, so the database stays as it was (no party, no user, not set up).
pub fn setup_welcome(conn: &mut Connection, input: &SetupInput) -> Result<SetupDone, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let has_user: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM user)", [], |r| r.get(0))?;
    if setup_done(&tx)? || has_user {
        return Err(CommandError::AlreadySetUp);
    }
    let org = new_id();
    let person = new_id();
    let role = new_id();
    let relation = new_id();
    let relation_back = new_id();
    let user_id = new_id();
    let actor = Actor::user(&user_id, &input.account_name);
    let stamp = Stamp::now(&actor);
    let person_name = format!("{} {}", input.first_name, input.last_name);
    let today = &stamp.at[..10];

    tx.execute(
        "INSERT INTO party (id, name_cached, organization_ind, person_ind, deleted_ind) VALUES (?1, ?2, 1, 0, 0)",
        params![org, input.organization_name],
    )?;
    tx.execute(
        "INSERT INTO organization (id, organization_name) VALUES (?1, ?2)",
        params![org, input.organization_name],
    )?;
    tx.execute(
        "INSERT INTO party (id, name_cached, organization_ind, person_ind, deleted_ind) VALUES (?1, ?2, 0, 1, 0)",
        params![person, person_name],
    )?;
    tx.execute(
        "INSERT INTO person (id, first_name, last_name, job_title) VALUES (?1, ?2, ?3, ?4)",
        params![person, input.first_name, input.last_name, input.job_title],
    )?;
    // The person works for the organization (kontroller "_wip_Create Employee": an employee role and a relation).
    // A relation is a pair of rows, one each way (BR-PARTY-03): the person is the employee of the organization, and
    // the organization is the employer of the person.
    for (id, from, to, what) in [(&relation, &person, &org, "Employee"), (&relation_back, &org, &person, "Employer")] {
        tx.execute(
            "INSERT INTO party_relation (id, party_id_from, party_id_to, relationship, from_date, current_ind,
                                         billing_ind, shipping_ind)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, 0, 0)",
            params![id, from, to, what, today],
        )?;
    }
    tx.execute(
        "INSERT INTO party_role (id, party_id, employee_ind, customer_ind, vendor_ind, lead_ind, current_ind, from_date)
         VALUES (?1, ?2, 1, 0, 0, 0, 1, ?3)",
        params![role, person, today],
    )?;
    tx.execute(
        "INSERT INTO employee (id, name_cached, initials) VALUES (?1, ?2, ?3)",
        params![role, person_name, crate::routes::initials(&person_name)],
    )?;
    tx.execute(
        "INSERT INTO user (id, account_name, party_id, email_address) VALUES (?1, ?2, ?3, ?4)",
        params![user_id, input.account_name, person, input.email],
    )?;
    tx.execute(
        "INSERT INTO system_setting (singleton, owner_id, tax_rate_bp, country_code, area_code,
                                     automatically_format_phone_numbers_ind, initial_setup_done_ind)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, 1)
         ON CONFLICT (singleton) DO UPDATE SET owner_id = ?1, tax_rate_bp = ?2, country_code = ?3, area_code = ?4,
           automatically_format_phone_numbers_ind = ?5, initial_setup_done_ind = 1",
        params![org, input.tax_rate_bp, input.country_code, input.area_code, i64::from(input.format_phone_numbers)],
    )?;
    for (table, id) in [
        ("party", &org),
        ("organization", &org),
        ("party", &person),
        ("person", &person),
        ("party_relation", &relation),
        ("party_relation", &relation_back),
        ("party_role", &role),
        ("employee", &role),
        ("user", &user_id),
    ] {
        audit::stamp_created(&tx, table, id, &stamp)?;
    }
    audit::stamp_created_by(&tx, "system_setting", "singleton", &1, &stamp)?;
    tx.commit()?;
    Ok(SetupDone { organization_id: org, person_id: person, employee_role_id: role, user: actor })
}

// ---------------------------------------------------------------------------------------------------------------
// OP-ADMIN-03 settings_get_update
// ---------------------------------------------------------------------------------------------------------------

/// The system settings of BR-ADMIN-05 as the settings page shows them. The SMTP password is not here: it is in the
/// keychain (AP-03).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemSettings {
    pub owner_id: Option<String>,
    pub tax_rate_bp: i64,
    pub default_tax_code: Option<String>,
    pub country_code: Option<String>,
    pub area_code: Option<i64>,
    pub format_phone_numbers: bool,
    pub email_address: Option<String>,
    pub email_full_name: Option<String>,
    pub email_user_name: Option<String>,
    pub smtp_server: Option<String>,
    pub smtp_port: Option<i64>,
    pub smtp_encryption: Option<String>,
    pub smtp_authentication: Option<String>,
    /// The address suggestion service (OP-INTEG-02), `None` = off (AP-12; V4).
    pub address_provider: Option<crate::integrations::AddressProvider>,
    /// FullContact person enrichment (OP-INTEG-03), off by default (AP-12; V4).
    pub fullcontact_enabled: bool,
}

pub fn settings_get(conn: &Connection) -> rusqlite::Result<SystemSettings> {
    Ok(conn
        .query_row(
            "SELECT owner_id, COALESCE(tax_rate_bp, 0), default_tax_code_id, country_code, CAST(area_code AS INTEGER),
                    COALESCE(automatically_format_phone_numbers_ind, 0), system_email_address, system_email_full_name,
                    system_email_user_name, system_email_smtp_server, CAST(system_email_smtp_server_port AS INTEGER),
                    system_email_smtp_server_encryption_type, system_email_smtp_server_authentication,
                    address_suggestion_provider, COALESCE(fullcontact_enabled_ind, 0)
             FROM system_setting WHERE singleton = 1",
            [],
            |r| {
                Ok(SystemSettings {
                    owner_id: r.get(0)?,
                    tax_rate_bp: r.get(1)?,
                    default_tax_code: r.get(2)?,
                    country_code: r.get(3)?,
                    area_code: r.get(4)?,
                    format_phone_numbers: r.get::<_, i64>(5)? == 1,
                    email_address: r.get(6)?,
                    email_full_name: r.get(7)?,
                    email_user_name: r.get(8)?,
                    smtp_server: r.get(9)?,
                    smtp_port: r.get(10)?,
                    smtp_encryption: r.get(11)?,
                    smtp_authentication: r.get(12)?,
                    address_provider: r
                        .get::<_, Option<String>>(13)?
                        .and_then(|p| crate::integrations::AddressProvider::parse(&p)),
                    fullcontact_enabled: r.get::<_, i64>(14)? == 1,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

/// The user's own email account (user.smtp_*, table 130 #111-117; BR-INTEG-01, kontroller 337 s84-89). Its password
/// is in the keychain (`SecretKey::UserSmtpPassword`, AP-03). An empty server, port, encryption or authentication
/// means "the system account's" (337 s85-88).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserEmail {
    pub address: Option<String>,
    pub full_name: Option<String>,
    pub user_name: Option<String>,
    pub server: Option<String>,
    pub port: Option<i64>,
    pub encryption: Option<String>,
    pub authentication: Option<String>,
}

/// The current user's email account (empty before setup).
pub fn user_email_get(conn: &Connection, actor: &Actor) -> rusqlite::Result<UserEmail> {
    let Some(user) = &actor.user_id else { return Ok(UserEmail::default()) };
    Ok(conn
        .query_row(
            "SELECT smtp_email_address, smtp_full_name, smtp_user_name, smtp_server, CAST(smtp_server_port AS INTEGER),
                    smtp_server_encryption_type, smtp_server_authentication
             FROM user WHERE id = ?1",
            [user],
            |r| {
                Ok(UserEmail {
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

/// Saves the current user's email account (one transaction, modification stamps).
pub fn user_email_update(conn: &mut Connection, e: &UserEmail, actor: &Actor) -> Result<(), CommandError> {
    let Some(user) = &actor.user_id else { return Err(CommandError::NotFound) };
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let n = tx.execute(
        "UPDATE user SET smtp_email_address = ?2, smtp_full_name = ?3, smtp_user_name = ?4, smtp_server = ?5,
                         smtp_server_port = ?6, smtp_server_encryption_type = ?7, smtp_server_authentication = ?8
         WHERE id = ?1",
        params![
            user,
            e.address,
            e.full_name,
            e.user_name,
            e.server,
            e.port.map(|p| p.to_string()),
            e.encryption,
            e.authentication
        ],
    )?;
    if n == 0 {
        return Err(CommandError::NotFound);
    }
    audit::stamp_modified(&tx, "user", user, &Stamp::now(actor))?;
    tx.commit()?;
    Ok(())
}

/// The organizations the owner can be: current, not archived.
pub fn owner_choices(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(name_cached, '') FROM party WHERE organization_ind = 1 AND COALESCE(deleted_ind, 0) = 0
         ORDER BY name_cached COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect();
    rows
}

/// Saves the settings row (one transaction, modification stamps of the current user). The owner must be one of
/// [`owner_choices`].
pub fn settings_update(conn: &mut Connection, s: &SystemSettings, actor: &Actor) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let owner_ok: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM party WHERE id = ?1 AND organization_ind = 1 AND COALESCE(deleted_ind, 0) = 0)",
        [&s.owner_id],
        |r| r.get(0),
    )?;
    if !owner_ok {
        return Err(CommandError::Validation(vec![FieldError {
            field: "owner_id",
            message: "Choose the organization that owns this install.".into(),
        }]));
    }
    let changed = tx.execute(
        "UPDATE system_setting SET owner_id = ?1, tax_rate_bp = ?2, default_tax_code_id = ?3, country_code = ?4,
           area_code = ?5, automatically_format_phone_numbers_ind = ?6, system_email_address = ?7,
           system_email_full_name = ?8, system_email_user_name = ?9, system_email_smtp_server = ?10,
           system_email_smtp_server_port = ?11, system_email_smtp_server_encryption_type = ?12,
           system_email_smtp_server_authentication = ?13, address_suggestion_provider = ?14,
           fullcontact_enabled_ind = ?15
         WHERE singleton = 1",
        params![
            s.owner_id,
            s.tax_rate_bp,
            s.default_tax_code,
            s.country_code,
            s.area_code,
            i64::from(s.format_phone_numbers),
            s.email_address,
            s.email_full_name,
            s.email_user_name,
            s.smtp_server,
            s.smtp_port,
            s.smtp_encryption,
            s.smtp_authentication,
            s.address_provider.map(|p| p.stored()),
            i64::from(s.fullcontact_enabled),
        ],
    )?;
    if changed != 1 {
        return Err(CommandError::Storage(rusqlite::Error::QueryReturnedNoRows));
    }
    audit::stamp_modified_by(&tx, "system_setting", "singleton", &1, &Stamp::now(actor))?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Field parsers shared by the setup and settings forms
// ---------------------------------------------------------------------------------------------------------------

/// A tax rate typed as a percent ("5.5", "5.50 %", "0") -> basis points (550). At most 2 decimals, 0 to 100 %.
/// Empty is 0. Never rounded: more decimals is a field error (as money input, S-03).
pub fn parse_percent_bp(text: &str) -> Result<i64, String> {
    let t = text.trim().trim_end_matches('%').trim();
    if t.is_empty() {
        return Ok(0);
    }
    let (int, frac) = t.split_once('.').unwrap_or((t, ""));
    if (int.is_empty() && frac.is_empty())
        || !int.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
    {
        return Err("Enter a percent, like 5.5.".into());
    }
    if frac.len() > 2 {
        return Err("Use at most 2 decimal places.".into());
    }
    if int.len() > 3 {
        return Err("Enter a rate from 0 to 100 %.".into());
    }
    let whole: i64 = if int.is_empty() { 0 } else { int.parse().map_err(|_| "Enter a percent, like 5.5.")? };
    let frac_bp: i64 = format!("{frac:0<2}").parse().unwrap_or(0);
    let bp = whole * 100 + frac_bp;
    if bp > 10_000 {
        return Err("Enter a rate from 0 to 100 %.".into());
    }
    Ok(bp)
}

/// Basis points as the percent text of the form: 550 -> "5.5", 600 -> "6", 525 -> "5.25".
pub fn format_percent(bp: i64) -> String {
    let s = format!("{}.{:02}", bp / 100, bp % 100);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// An optional email address: empty is None; otherwise one `@` with text on both sides and no spaces.
pub fn parse_email(text: &str) -> Result<Option<String>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    match t.split_once('@') {
        Some((a, b)) if !a.is_empty() && b.contains('.') && !t.contains(char::is_whitespace) && !b.contains('@') => {
            Ok(Some(t.to_owned()))
        }
        _ => Err("Enter an email address like name@example.com, or leave it empty.".into()),
    }
}

/// An optional US-style area code: empty is None, otherwise 3 digits.
pub fn parse_area_code(text: &str) -> Result<Option<i64>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if t.len() == 3 && t.chars().all(|c| c.is_ascii_digit()) {
        Ok(Some(t.parse().unwrap()))
    } else {
        Err("Enter 3 digits, like 207.".into())
    }
}

/// The country calling code: 1 to 3 digits, an optional leading `+`. Empty is "1" (US, OD-07-02).
pub fn parse_country_code(text: &str) -> Result<String, String> {
    let t = text.trim().trim_start_matches('+');
    if t.is_empty() {
        return Ok("1".into());
    }
    if (1..=3).contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit()) {
        Ok(t.to_owned())
    } else {
        Err("Enter the calling code, like 1 for the US.".into())
    }
}

/// Trimmed text; empty is None.
pub fn optional(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// The OS account name for user.account_name (FileMaker's Get(AccountName)); "owner" if the OS gives none.
pub fn os_account_name() -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "owner".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_to_basis_points_and_back() {
        for (text, bp) in [("5.5", 550), ("5.50 %", 550), ("0", 0), ("", 0), (".25", 25), ("100", 10_000), ("7", 700)] {
            assert_eq!(parse_percent_bp(text), Ok(bp), "{text}");
        }
        for bad in ["5.555", "abc", "-1", "101", "1,5", "5.5.5", "."] {
            assert!(parse_percent_bp(bad).is_err(), "{bad}");
        }
        assert_eq!(format_percent(550), "5.5");
        assert_eq!(format_percent(600), "6");
        assert_eq!(format_percent(525), "5.25");
        assert_eq!(format_percent(5), "0.05");
        assert_eq!(format_percent(0), "0");
        for bp in [0, 5, 50, 525, 550, 10_000] {
            assert_eq!(parse_percent_bp(&format_percent(bp)), Ok(bp));
        }
    }

    #[test]
    fn small_field_parsers() {
        assert_eq!(parse_email(""), Ok(None));
        assert_eq!(parse_email(" mara@ellison.example "), Ok(Some("mara@ellison.example".into())));
        for bad in ["mara", "mara@", "@x.com", "a b@x.com", "a@b@c.com", "a@localhost"] {
            assert!(parse_email(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_area_code("207"), Ok(Some(207)));
        assert_eq!(parse_area_code(""), Ok(None));
        assert!(parse_area_code("20").is_err() && parse_area_code("2071").is_err());
        assert_eq!(parse_country_code("+44"), Ok("44".into()));
        assert_eq!(parse_country_code(""), Ok("1".into()));
        assert!(parse_country_code("1234").is_err());
    }
}
