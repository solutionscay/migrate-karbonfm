//! CONTACT commands of I2 (05 OP-CONTACT-*, 07 BR-CONTACT-*): a party's phones, network addresses (email and web)
//! and postal addresses. A contact method is a `contact_method` row plus one subtype row with the same id (D-15);
//! purposes (billing, shipping, mailing) are `contact_method_purpose` rows with a cached flag on the contact method.

use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::ids::new_id;
use crate::reference::ContactMethodPurposeType;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

/// Which subtype a contact method is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodKind {
    Phone,
    /// network_address with email_address_ind = 1.
    Email,
    /// network_address without email_address_ind (a web address).
    Web,
    Address,
}

impl MethodKind {
    pub fn key(self) -> &'static str {
        match self {
            MethodKind::Phone => "phone",
            MethodKind::Email => "email",
            MethodKind::Web => "web",
            MethodKind::Address => "address",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "phone" => Some(MethodKind::Phone),
            "email" => Some(MethodKind::Email),
            "web" => Some(MethodKind::Web),
            "address" => Some(MethodKind::Address),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            MethodKind::Phone => "Phone",
            MethodKind::Email => "Email",
            MethodKind::Web => "Website",
            MethodKind::Address => "Address",
        }
    }
    /// The subtype table that holds `primary_ind` (BR-CONTACT-01 is per subtype table: email and web share one).
    pub fn table(self) -> &'static str {
        match self {
            MethodKind::Phone => "phone",
            MethodKind::Email | MethodKind::Web => "network_address",
            MethodKind::Address => "address",
        }
    }
}

/// One contact method with its subtype fields, as the party detail shows and the edit form fills it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub id: String,
    pub party_id: String,
    pub kind: MethodKind,
    pub label: String,
    pub comment: String,
    pub primary: bool,
    pub billing: bool,
    pub shipping: bool,
    pub mailing: bool,
    // phone
    pub number: String,
    pub extension: String,
    pub country_code: Option<i64>,
    pub fax: bool,
    pub sms: bool,
    pub voice: bool,
    // network address
    pub address: String,
    // postal address
    pub line1: String,
    pub line2: String,
    pub line3: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

impl Method {
    /// What the detail page shows: the number (with its extension), the address, or the postal address on one line.
    pub fn display(&self) -> String {
        match self.kind {
            MethodKind::Phone if self.extension.is_empty() => self.number.clone(),
            MethodKind::Phone => format!("{} ext. {}", self.number, self.extension),
            MethodKind::Email | MethodKind::Web => self.address.clone(),
            MethodKind::Address => {
                address_display(&self.line1, &self.line2, &self.city, &self.state, &self.postal_code)
            }
        }
    }

    /// The purposes that are set, as their reference names.
    pub fn purposes(&self) -> Vec<&'static str> {
        [
            (self.billing, ContactMethodPurposeType::Billing),
            (self.shipping, ContactMethodPurposeType::Shipping),
            (self.mailing, ContactMethodPurposeType::Mailing),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, p)| p.name())
        .collect()
    }
}

/// A postal address on one line: "48 Wharf Street, Suite 3, Portland, ME 04101". It is the format of
/// party.address_display_cached (BR-PARTY-06); see [`address_display_sql`].
pub fn address_display(line1: &str, line2: &str, city: &str, state: &str, postal: &str) -> String {
    let mut out = String::new();
    for part in [line1, line2, city] {
        let part = part.trim();
        if !part.is_empty() {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str(part);
        }
    }
    let tail = [state.trim(), postal.trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
    if !tail.is_empty() {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(&tail);
    }
    out
}

/// [`address_display`] as an SQL expression over an `address` row aliased `a`, for the party caches.
pub fn address_display_sql() -> &'static str {
    "NULLIF(trim(
       COALESCE(NULLIF(trim(a.address_line_1), ''), '')
       || COALESCE(', ' || NULLIF(trim(a.address_line_2), ''), '')
       || COALESCE(', ' || NULLIF(trim(a.city), ''), '')
       || COALESCE(', ' || NULLIF(trim(COALESCE(NULLIF(trim(a.state_province), '') || ' ', '')
                                        || COALESCE(trim(a.postal_code), '')), ''), ''),
     ', '), '')"
}

const METHOD_SQL: &str = "
SELECT cm.id, COALESCE(cm.party_id, ''), COALESCE(cm.phone_ind, 0) = 1, COALESCE(cm.address_ind, 0) = 1,
       COALESCE(n.email_address_ind, 0) = 1, COALESCE(cm.label, ''), COALESCE(cm.comment, ''),
       COALESCE(p.primary_ind, n.primary_ind, a.primary_ind, 0) = 1,
       COALESCE(cm.billing_ind_cached, 0) = 1, COALESCE(cm.shipping_ind_cached, 0) = 1,
       COALESCE(cm.mailing_ind_cached, 0) = 1,
       COALESCE(p.number, ''), COALESCE(p.extension, ''), CAST(p.country_code AS INTEGER),
       COALESCE(p.fax_ind, 0) = 1, COALESCE(p.sms_ind, 0) = 1, COALESCE(p.voice_ind, 0) = 1,
       COALESCE(n.address, ''),
       COALESCE(a.address_line_1, ''), COALESCE(a.address_line_2, ''), COALESCE(a.address_line_3, ''),
       COALESCE(a.city, ''), COALESCE(a.state_province, ''), COALESCE(a.postal_code, ''), COALESCE(a.country, '')
FROM contact_method cm
LEFT JOIN phone p ON p.id = cm.id
LEFT JOIN network_address n ON n.id = cm.id
LEFT JOIN address a ON a.id = cm.id";

fn method_row(r: &rusqlite::Row) -> rusqlite::Result<Method> {
    let (phone, address, email): (bool, bool, bool) = (r.get(2)?, r.get(3)?, r.get(4)?);
    let kind = if phone {
        MethodKind::Phone
    } else if address {
        MethodKind::Address
    } else if email {
        MethodKind::Email
    } else {
        MethodKind::Web
    };
    Ok(Method {
        id: r.get(0)?,
        party_id: r.get(1)?,
        kind,
        label: r.get(5)?,
        comment: r.get(6)?,
        primary: r.get(7)?,
        billing: r.get(8)?,
        shipping: r.get(9)?,
        mailing: r.get(10)?,
        number: r.get(11)?,
        extension: r.get(12)?,
        country_code: r.get(13)?,
        fax: r.get(14)?,
        sms: r.get(15)?,
        voice: r.get(16)?,
        address: r.get(17)?,
        line1: r.get(18)?,
        line2: r.get(19)?,
        line3: r.get(20)?,
        city: r.get(21)?,
        state: r.get(22)?,
        postal_code: r.get(23)?,
        country: r.get(24)?,
    })
}

/// The contact methods of a party: primary first within each kind, then in the order they were added.
pub fn methods_of(conn: &Connection, party_id: &str) -> rusqlite::Result<Vec<Method>> {
    let mut stmt = conn.prepare(&format!(
        "{METHOD_SQL} WHERE cm.party_id = ?1
         ORDER BY COALESCE(p.primary_ind, n.primary_ind, a.primary_ind, 0) DESC, cm.creation_timestamp, cm.id"
    ))?;
    let rows = stmt.query_map([party_id], method_row)?.collect();
    rows
}

/// One contact method by id.
pub fn method(conn: &Connection, id: &str) -> rusqlite::Result<Option<Method>> {
    conn.query_row(&format!("{METHOD_SQL} WHERE cm.id = ?1"), [id], method_row).optional()
}

// ---------------------------------------------------------------------------------------------------------------
// BR-CONTACT-04 phone numbers
// ---------------------------------------------------------------------------------------------------------------

/// The digits of a phone number (phone.number_digits_only, a stored calculation kept by the commands).
pub fn digits_only(number: &str) -> String {
    number.chars().filter(char::is_ascii_digit).collect()
}

/// BR-CONTACT-04: with automatic formatting on, a US number (country code 1, 10 digits, or 11 digits starting with
/// the country code 1) is stored as `(207) 555-0142`; every other number is kept as typed (trimmed).
pub fn format_phone(number: &str, country_code: Option<i64>, format_on: bool) -> String {
    let typed = number.trim();
    if !format_on || country_code != Some(1) {
        return typed.to_owned();
    }
    let digits = digits_only(typed);
    let ten = match digits.len() {
        10 => digits.as_str(),
        11 if digits.starts_with('1') => &digits[1..],
        _ => return typed.to_owned(),
    };
    // A typed letter (a vanity number or a note) is not a plain number: keep it as typed.
    if typed.chars().any(char::is_alphabetic) {
        return typed.to_owned();
    }
    format!("({}) {}-{}", &ten[..3], &ten[3..6], &ten[6..])
}

/// The phone settings of the install: the default country code and automatic formatting (system_setting).
pub fn phone_settings(conn: &Connection) -> rusqlite::Result<(Option<i64>, bool)> {
    conn.query_row(
        "SELECT CAST(NULLIF(trim(country_code), '') AS INTEGER), COALESCE(automatically_format_phone_numbers_ind, 0) = 1
         FROM system_setting WHERE singleton = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map(|o| o.unwrap_or((Some(1), false)))
}

// ---------------------------------------------------------------------------------------------------------------
// OP-CONTACT-01 contact_method_save (the insert; S4 adds the update)
// ---------------------------------------------------------------------------------------------------------------

/// What a contact-method form gives. Only the fields of `kind` are used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodInput {
    pub kind: Option<MethodKind>,
    pub label: String,
    pub comment: String,
    /// Make it the party's primary of its kind (BR-CONTACT-01). The first of a kind is primary anyway.
    pub primary: bool,
    /// Replaces the purposes (BR-CONTACT-03).
    pub purposes: Vec<ContactMethodPurposeType>,
    pub number: String,
    pub extension: String,
    /// Empty uses the install's country code.
    pub country_code: Option<i64>,
    pub fax: bool,
    pub sms: bool,
    pub address: String,
    pub line1: String,
    pub line2: String,
    pub line3: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

impl MethodInput {
    pub fn email(address: &str) -> Self {
        Self { kind: Some(MethodKind::Email), address: address.into(), ..Default::default() }
    }
    pub fn phone(number: &str) -> Self {
        Self { kind: Some(MethodKind::Phone), number: number.into(), ..Default::default() }
    }

    /// Field errors of the form; `prefix` names the fields (`""`, or `"email_"` on the create dialog).
    pub fn check(&self, prefix: &str) -> Vec<FieldError> {
        let field = |name: &'static str| -> &'static str {
            match (prefix, name) {
                ("", n) => n,
                ("email_", "address") => "email",
                ("phone_", "number") => "phone",
                (_, n) => n,
            }
        };
        let mut errors = Vec::new();
        let mut err =
            |name: &'static str, message: &str| errors.push(FieldError { field: field(name), message: message.into() });
        match self.kind {
            None => err("kind", "Choose phone, email, website or address."),
            Some(MethodKind::Phone) => {
                if digits_only(&self.number).is_empty() {
                    err("number", "Enter a phone number.");
                }
            }
            Some(MethodKind::Email) => {
                if let Err(e) = crate::admin::parse_email(&self.address)
                    .and_then(|o| o.ok_or_else(|| "Enter an email address like name@example.com.".to_owned()))
                {
                    err("address", &e);
                }
            }
            Some(MethodKind::Web) => {
                let a = self.address.trim();
                if a.is_empty() || a.contains(char::is_whitespace) {
                    err("address", "Enter a web address like example.com.");
                }
            }
            Some(MethodKind::Address) => {
                if [&self.line1, &self.city, &self.postal_code].iter().all(|s| s.trim().is_empty()) {
                    err("line1", "Enter a street, a city or a postal code.");
                }
            }
        }
        errors
    }
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// Inserts a contact method of `party_id` with its subtype row and purposes, inside the caller's transaction, then
/// keeps one primary per kind (BR-CONTACT-01: the new one is primary when asked or when it is the first of its
/// kind). The caller refreshes the party caches. Returns the new id.
pub fn insert_method(
    conn: &Connection,
    party_id: &str,
    input: &MethodInput,
    stamp: &Stamp,
) -> Result<String, CommandError> {
    let errors = input.check("");
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }
    let kind = input.kind.expect("checked");
    let id = new_id();
    conn.execute(
        "INSERT INTO contact_method (id, party_id, phone_ind, address_ind, network_address_ind, label, comment,
                                     billing_ind_cached, shipping_ind_cached, mailing_ind_cached)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0)",
        params![
            id,
            party_id,
            i64::from(kind == MethodKind::Phone),
            i64::from(kind == MethodKind::Address),
            i64::from(matches!(kind, MethodKind::Email | MethodKind::Web)),
            opt(&input.label),
            opt(&input.comment),
        ],
    )?;
    audit::stamp_created(conn, "contact_method", &id, stamp)?;
    write_subtype(conn, &id, kind, input, true)?;
    audit::stamp_created(conn, kind.table(), &id, stamp)?;
    set_purposes(conn, &id, &input.purposes, stamp)?;
    let has_primary: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM contact_method cm JOIN {t} s ON s.id = cm.id
             WHERE cm.party_id = ?1 AND cm.id <> ?2 AND COALESCE(s.primary_ind, 0) = 1 {email})",
            t = kind.table(),
            email = email_filter(kind, "s"),
        ),
        params![party_id, id],
        |r| r.get(0),
    )?;
    if input.primary || !has_primary {
        make_primary(conn, party_id, &id, kind, stamp)?;
    }
    Ok(id)
}

/// Email and web addresses share network_address but each kind has its own primary (the party caches the primary
/// email; a website is never the primary email). Returns the extra filter for `alias`.
fn email_filter(kind: MethodKind, alias: &str) -> String {
    match kind {
        MethodKind::Email => format!("AND COALESCE({alias}.email_address_ind, 0) = 1"),
        MethodKind::Web => format!("AND COALESCE({alias}.email_address_ind, 0) = 0"),
        _ => String::new(),
    }
}

/// Writes the subtype row of `kind` (insert, or update of an existing row).
fn write_subtype(
    conn: &Connection,
    id: &str,
    kind: MethodKind,
    input: &MethodInput,
    insert: bool,
) -> Result<(), CommandError> {
    match kind {
        MethodKind::Phone => {
            let (default_cc, format_on) = phone_settings(conn)?;
            let cc = input.country_code.or(default_cc);
            let number = format_phone(&input.number, cc, format_on);
            let digits = digits_only(&number);
            if insert {
                conn.execute(
                    "INSERT INTO phone (id, number, extension, country_code, primary_ind, fax_ind, sms_ind, voice_ind,
                                        number_digits_only)
                     VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        number,
                        opt(&input.extension),
                        cc,
                        i64::from(input.fax),
                        i64::from(input.sms),
                        i64::from(!input.fax),
                        digits
                    ],
                )?;
            } else {
                conn.execute(
                    "UPDATE phone SET number = ?2, extension = ?3, country_code = ?4, fax_ind = ?5, sms_ind = ?6,
                                      voice_ind = ?7, number_digits_only = ?8 WHERE id = ?1",
                    params![
                        id,
                        number,
                        opt(&input.extension),
                        cc,
                        i64::from(input.fax),
                        i64::from(input.sms),
                        i64::from(!input.fax),
                        digits
                    ],
                )?;
            }
        }
        MethodKind::Email | MethodKind::Web => {
            let email = kind == MethodKind::Email;
            if insert {
                conn.execute(
                    "INSERT INTO network_address (id, address, primary_ind, email_address_ind, web_address_ind)
                     VALUES (?1, ?2, 0, ?3, ?4)",
                    params![id, input.address.trim(), i64::from(email), i64::from(!email)],
                )?;
            } else {
                conn.execute(
                    "UPDATE network_address SET address = ?2, email_address_ind = ?3, web_address_ind = ?4 WHERE id = ?1",
                    params![id, input.address.trim(), i64::from(email), i64::from(!email)],
                )?;
            }
        }
        MethodKind::Address => {
            let values = params![
                id,
                opt(&input.line1),
                opt(&input.line2),
                opt(&input.line3),
                opt(&input.city),
                opt(&input.state),
                opt(&input.postal_code),
                opt(&input.country),
            ];
            if insert {
                conn.execute(
                    "INSERT INTO address (id, address_line_1, address_line_2, address_line_3, city, state_province,
                                          postal_code, country, primary_ind)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
                    values,
                )?;
            } else {
                conn.execute(
                    "UPDATE address SET address_line_1 = ?2, address_line_2 = ?3, address_line_3 = ?4, city = ?5,
                                        state_province = ?6, postal_code = ?7, country = ?8 WHERE id = ?1",
                    values,
                )?;
            }
        }
    }
    Ok(())
}

/// BR-CONTACT-03: replaces the purposes of a contact method with `purposes` and sets the cached flags.
pub fn set_purposes(
    conn: &Connection,
    id: &str,
    purposes: &[ContactMethodPurposeType],
    stamp: &Stamp,
) -> Result<(), CommandError> {
    conn.execute("DELETE FROM contact_method_purpose WHERE contact_method_id = ?1", [id])?;
    let mut seen = Vec::new();
    for p in purposes {
        if seen.contains(p) {
            continue;
        }
        seen.push(*p);
        let row = new_id();
        conn.execute(
            "INSERT INTO contact_method_purpose (id, contact_method_id, contact_method_purpose_type_id) VALUES (?1, ?2, ?3)",
            params![row, id, p.id()],
        )?;
        audit::stamp_created(conn, "contact_method_purpose", &row, stamp)?;
    }
    sync_purpose_flags(conn, id)
}

/// Sets billing/shipping/mailing_ind_cached from the purpose rows (ids 1, 2, 3; BR-CONTACT-02).
fn sync_purpose_flags(conn: &Connection, id: &str) -> Result<(), CommandError> {
    let has = |t: ContactMethodPurposeType| {
        format!(
            "EXISTS (SELECT 1 FROM contact_method_purpose x WHERE x.contact_method_id = ?1 \
             AND x.contact_method_purpose_type_id = {})",
            t.id()
        )
    };
    conn.execute(
        &format!(
            "UPDATE contact_method SET billing_ind_cached = {}, shipping_ind_cached = {}, mailing_ind_cached = {}
             WHERE id = ?1",
            has(ContactMethodPurposeType::Billing),
            has(ContactMethodPurposeType::Shipping),
            has(ContactMethodPurposeType::Mailing)
        ),
        [id],
    )?;
    Ok(())
}

/// BR-CONTACT-01: makes `id` the primary of its kind and clears primary_ind on the party's others of that kind.
fn make_primary(
    conn: &Connection,
    party_id: &str,
    id: &str,
    kind: MethodKind,
    stamp: &Stamp,
) -> Result<(), CommandError> {
    let t = kind.table();
    let mut stmt = conn.prepare(&format!(
        "SELECT s.id FROM contact_method cm JOIN {t} s ON s.id = cm.id
         WHERE cm.party_id = ?1 AND s.id <> ?2 AND COALESCE(s.primary_ind, 0) = 1 {}",
        email_filter(kind, "s")
    ))?;
    let others: Vec<String> = stmt.query_map(params![party_id, id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for other in others {
        conn.execute(&format!("UPDATE {t} SET primary_ind = 0 WHERE id = ?1"), [&other])?;
        audit::stamp_modified(conn, t, &other, stamp)?;
    }
    conn.execute(&format!("UPDATE {t} SET primary_ind = 1 WHERE id = ?1"), [id])?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// The commands (OP-CONTACT-01..04): one BEGIN IMMEDIATE transaction each, stamped (BR-PARTY-07), and each ends by
// refreshing the party caches (BR-PARTY-06)
// ---------------------------------------------------------------------------------------------------------------

/// The party must exist and not be archived (an archived party's records stay as they are, BR-PARTY-01).
fn writable_party(conn: &Connection, party_id: &str) -> Result<(), CommandError> {
    let archived: Option<bool> = conn
        .query_row("SELECT COALESCE(deleted_ind, 0) = 1 FROM party WHERE id = ?1", [party_id], |r| r.get(0))
        .optional()?;
    match archived {
        None => Err(CommandError::NotFound),
        Some(true) => Err(CommandError::Refused("The contact methods of an archived party cannot be changed.".into())),
        Some(false) => Ok(()),
    }
}

fn existing(conn: &Connection, id: &str) -> Result<Method, CommandError> {
    let m = method(conn, id)?.ok_or(CommandError::NotFound)?;
    writable_party(conn, &m.party_id)?;
    Ok(m)
}

fn finish(conn: &Connection, party_id: &str, stamp: &Stamp) -> Result<(), CommandError> {
    crate::parties::refresh_caches(conn, party_id)?;
    audit::stamp_modified(conn, "party", &party_id, stamp)?;
    Ok(())
}

/// OP-CONTACT-01 `contact_method_save`, new (`POST /parties/{id}/contact-methods`; BR-CONTACT-03, BR-CONTACT-04).
pub fn contact_method_create(
    conn: &mut Connection,
    party_id: &str,
    input: &MethodInput,
    actor: &Actor,
) -> Result<String, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    writable_party(&tx, party_id)?;
    let stamp = Stamp::now(actor);
    let id = insert_method(&tx, party_id, input, &stamp)?;
    finish(&tx, party_id, &stamp)?;
    tx.commit()?;
    Ok(id)
}

/// OP-CONTACT-01 `contact_method_save`, edit (`PUT /contact-methods/{id}`): label, comment and the subtype fields;
/// the purposes are replaced, not merged (BR-CONTACT-03). A phone stays a phone and an address an address; an email
/// may become a website and back. `primary` makes it the primary (it never clears a primary: choose another one).
pub fn contact_method_update(
    conn: &mut Connection,
    id: &str,
    input: &MethodInput,
    actor: &Actor,
) -> Result<(), CommandError> {
    let errors = input.check("");
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = existing(&tx, id)?;
    let kind = input.kind.expect("checked");
    if kind.table() != m.kind.table() {
        return Err(CommandError::Validation(vec![FieldError {
            field: "kind",
            message: format!("A {} cannot become a {}.", m.kind.label().to_lowercase(), kind.label().to_lowercase()),
        }]));
    }
    let stamp = Stamp::now(actor);
    tx.execute(
        "UPDATE contact_method SET label = ?2, comment = ?3 WHERE id = ?1",
        params![id, opt(&input.label), opt(&input.comment)],
    )?;
    audit::stamp_modified(&tx, "contact_method", &id, &stamp)?;
    write_subtype(&tx, id, kind, input, false)?;
    if kind != m.kind && m.primary {
        // An email that becomes a website (or back) leaves the primary of its old kind.
        tx.execute(&format!("UPDATE {} SET primary_ind = 0 WHERE id = ?1", kind.table()), [id])?;
    }
    audit::stamp_modified(&tx, kind.table(), &id, &stamp)?;
    set_purposes(&tx, id, &input.purposes, &stamp)?;
    if input.primary {
        make_primary(&tx, &m.party_id, id, kind, &stamp)?;
    }
    finish(&tx, &m.party_id, &stamp)?;
    tx.commit()?;
    Ok(())
}

/// OP-CONTACT-02 `contact_method_set_primary` (BR-CONTACT-01, BR-PARTY-06): this one becomes the primary of its
/// kind, the party's others of that kind are cleared, and the party caches follow.
pub fn contact_method_set_primary(conn: &mut Connection, id: &str, actor: &Actor) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = existing(&tx, id)?;
    let stamp = Stamp::now(actor);
    make_primary(&tx, &m.party_id, id, m.kind, &stamp)?;
    audit::stamp_modified(&tx, m.kind.table(), &id, &stamp)?;
    finish(&tx, &m.party_id, &stamp)?;
    tx.commit()?;
    Ok(())
}

/// OP-CONTACT-03 `contact_method_toggle_purpose` (BR-CONTACT-02): creates or deletes the purpose row and sets or
/// clears the cached flag; toggling twice returns to the start. Returns whether the purpose is now set.
pub fn contact_method_toggle_purpose(
    conn: &mut Connection,
    id: &str,
    purpose: ContactMethodPurposeType,
    actor: &Actor,
) -> Result<bool, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = existing(&tx, id)?;
    let stamp = Stamp::now(actor);
    // Every row of this purpose goes (an import may have left two); otherwise one is added.
    let removed = tx.execute(
        "DELETE FROM contact_method_purpose WHERE contact_method_id = ?1 AND contact_method_purpose_type_id = ?2",
        params![id, purpose.id()],
    )?;
    if removed == 0 {
        let row = new_id();
        tx.execute(
            "INSERT INTO contact_method_purpose (id, contact_method_id, contact_method_purpose_type_id) VALUES (?1, ?2, ?3)",
            params![row, id, purpose.id()],
        )?;
        audit::stamp_created(&tx, "contact_method_purpose", &row, &stamp)?;
    }
    sync_purpose_flags(&tx, id)?;
    audit::stamp_modified(&tx, "contact_method", &id, &stamp)?;
    finish(&tx, &m.party_id, &stamp)?;
    tx.commit()?;
    Ok(removed == 0)
}

/// What a delete removes, for the confirmation dialog (AP-07).
pub fn delete_summary(m: &Method, party_name: &str) -> String {
    format!("Deletes the {} {} of {party_name}.", m.kind.label().to_lowercase(), m.display())
}

/// OP-CONTACT-04 `contact_method_delete` (BR-CONTACT-05, AP-07): without `confirm` nothing changes and the answer is
/// ConfirmationRequired with a summary. With it, the purposes, the subtype row and the contact method go, and every
/// party cache that pointed at it is recomputed (the source left email_address_id_cached behind; D-07).
pub fn contact_method_delete(
    conn: &mut Connection,
    id: &str,
    confirm: bool,
    actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = existing(&tx, id)?;
    if !confirm {
        let name: String =
            tx.query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [&m.party_id], |r| r.get(0))?;
        return Err(CommandError::ConfirmationRequired(delete_summary(&m, &name)));
    }
    tx.execute("DELETE FROM contact_method_purpose WHERE contact_method_id = ?1", [id])?;
    tx.execute(&format!("DELETE FROM {} WHERE id = ?1", m.kind.table()), [id])?;
    tx.execute("DELETE FROM contact_method WHERE id = ?1", [id])?;
    finish(&tx, &m.party_id, &Stamp::now(actor))?;
    tx.commit()?;
    Ok(())
}

/// The number of primaries per kind of a party (BR-CONTACT-01 says at most one each); for the tests and checks.
pub fn primaries(conn: &Connection, party_id: &str) -> rusqlite::Result<Vec<(MethodKind, usize)>> {
    let methods = methods_of(conn, party_id)?;
    Ok([MethodKind::Phone, MethodKind::Email, MethodKind::Web, MethodKind::Address]
        .into_iter()
        .map(|k| (k, methods.iter().filter(|m| m.kind == k && m.primary).count()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_in_memory, Seed};

    fn setup() -> (Connection, Actor) {
        let (conn, _) = open_in_memory(Seed::Demo).unwrap();
        conn.execute("UPDATE system_setting SET automatically_format_phone_numbers_ind = 1, country_code = '1'", [])
            .unwrap();
        let actor = crate::admin::current_user(&conn).unwrap().unwrap();
        (conn, actor)
    }

    fn cached(conn: &Connection, party: &str) -> (String, String, String, String) {
        conn.query_row(
            "SELECT COALESCE(phone_number_cached, ''), COALESCE(email_address_cached, ''),
                    COALESCE(email_address_id_cached, ''), COALESCE(address_display_cached, '')
             FROM party WHERE id = ?1",
            [party],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    fn phone(number: &str, primary: bool) -> MethodInput {
        MethodInput { primary, label: "Mobile".into(), ..MethodInput::phone(number) }
    }

    #[test]
    fn create_formats_us_phones_and_keeps_others_as_typed() {
        let (mut conn, actor) = setup();
        let id = contact_method_create(&mut conn, "p-tf", &phone("207 555 0177", false), &actor).unwrap();
        let m = method(&conn, &id).unwrap().unwrap();
        assert_eq!((m.number.as_str(), m.country_code), ("(207) 555-0177", Some(1)));
        let digits: String =
            conn.query_row("SELECT number_digits_only FROM phone WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(digits, "2075550177");
        let uk = MethodInput { country_code: Some(44), ..phone("020 7946 0958", false) };
        let id = contact_method_create(&mut conn, "p-tf", &uk, &actor).unwrap();
        assert_eq!(method(&conn, &id).unwrap().unwrap().number, "020 7946 0958");
        conn.execute("UPDATE system_setting SET automatically_format_phone_numbers_ind = 0", []).unwrap();
        let id = contact_method_create(&mut conn, "p-tf", &phone("2075550178", false), &actor).unwrap();
        assert_eq!(method(&conn, &id).unwrap().unwrap().number, "2075550178", "setting off");
    }

    #[test]
    fn only_one_primary_per_kind_after_any_sequence() {
        let (mut conn, actor) = setup();
        let party = "p-pr";
        let mut ids = Vec::new();
        for (i, n) in ["6175550001", "6175550002", "6175550003"].iter().enumerate() {
            ids.push(contact_method_create(&mut conn, party, &phone(n, i == 1), &actor).unwrap());
        }
        let emails: Vec<String> = ["a@x.example", "b@x.example"]
            .iter()
            .map(|e| contact_method_create(&mut conn, party, &MethodInput::email(e), &actor).unwrap())
            .collect();
        let web = MethodInput { kind: Some(MethodKind::Web), address: "priya.example".into(), ..Default::default() };
        let web = contact_method_create(&mut conn, party, &web, &actor).unwrap();
        // A fixed pseudo-random sequence of primary changes, deletes and adds.
        let mut seed = 7u64;
        for step in 0..40 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let all: Vec<String> = ids.iter().chain(emails.iter()).chain([&web]).cloned().collect();
            let pick = &all[(seed >> 33) as usize % all.len()];
            if method(&conn, pick).unwrap().is_some() {
                if step % 9 == 8 {
                    contact_method_delete(&mut conn, pick, true, &actor).unwrap();
                } else {
                    contact_method_set_primary(&mut conn, pick, &actor).unwrap();
                }
            } else {
                ids.push(contact_method_create(&mut conn, party, &phone("6175550999", step % 2 == 0), &actor).unwrap());
            }
            for (kind, n) in primaries(&conn, party).unwrap() {
                assert!(n <= 1, "step {step}: {n} primaries of {kind:?}");
            }
            // The cache always shows the primary phone and email when there is one.
            let methods = methods_of(&conn, party).unwrap();
            let (cached_phone, cached_email, cached_email_id, _) = cached(&conn, party);
            if let Some(p) = methods.iter().find(|m| m.kind == MethodKind::Phone && m.primary) {
                assert_eq!(cached_phone, p.number, "step {step}");
            }
            if let Some(e) = methods.iter().find(|m| m.kind == MethodKind::Email && m.primary) {
                assert_eq!((cached_email, cached_email_id), (e.address.clone(), e.id.clone()), "step {step}");
            }
        }
        // The website is never the cached email.
        let (_, email, _, _) = cached(&conn, party);
        assert_ne!(email, "priya.example");
    }

    #[test]
    fn set_primary_moves_the_cache() {
        let (mut conn, actor) = setup();
        let second = contact_method_create(&mut conn, "p-hp", &phone("2075550111", false), &actor).unwrap();
        assert_eq!(cached(&conn, "p-hp").0, "(207) 555-0142");
        contact_method_set_primary(&mut conn, &second, &actor).unwrap();
        assert_eq!(cached(&conn, "p-hp").0, "(207) 555-0111");
        let label: String =
            conn.query_row("SELECT phone_label_cached FROM party WHERE id = 'p-hp'", [], |r| r.get(0)).unwrap();
        assert_eq!(label, "Mobile");
        assert!(matches!(contact_method_set_primary(&mut conn, "nope", &actor), Err(CommandError::NotFound)));
    }

    #[test]
    fn toggle_purpose_twice_returns_to_the_start() {
        let (mut conn, actor) = setup();
        let id = contact_method_create(&mut conn, "p-tf", &phone("2075550123", false), &actor).unwrap();
        let start = method(&conn, &id).unwrap().unwrap();
        assert!(contact_method_toggle_purpose(&mut conn, &id, ContactMethodPurposeType::Shipping, &actor).unwrap());
        let on = method(&conn, &id).unwrap().unwrap();
        assert!(on.shipping && !on.billing && !on.mailing);
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM contact_method_purpose WHERE contact_method_id = ?1 AND contact_method_purpose_type_id = 2", [&id], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert!(!contact_method_toggle_purpose(&mut conn, &id, ContactMethodPurposeType::Shipping, &actor).unwrap());
        let back = method(&conn, &id).unwrap().unwrap();
        assert_eq!((back.billing, back.shipping, back.mailing), (start.billing, start.shipping, start.mailing));
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM contact_method_purpose WHERE contact_method_id = ?1", [&id], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "toggling is idempotent in the rows too");
    }

    #[test]
    fn save_replaces_the_purposes() {
        let (mut conn, actor) = setup();
        use ContactMethodPurposeType::*;
        let input = MethodInput { purposes: vec![Billing, Mailing], ..MethodInput::email("ap@x.example") };
        let id = contact_method_create(&mut conn, "p-tf", &input, &actor).unwrap();
        let m = method(&conn, &id).unwrap().unwrap();
        assert!(m.billing && m.mailing && !m.shipping);
        let edit =
            MethodInput { purposes: vec![Shipping], label: "Work".into(), ..MethodInput::email("ap2@x.example") };
        contact_method_update(&mut conn, &id, &edit, &actor).unwrap();
        let m = method(&conn, &id).unwrap().unwrap();
        assert_eq!((m.billing, m.shipping, m.mailing), (false, true, false), "replaced, not merged");
        assert_eq!((m.address.as_str(), m.label.as_str()), ("ap2@x.example", "Work"));
        let wrong = MethodInput { kind: Some(MethodKind::Address), line1: "1 Main".into(), ..Default::default() };
        assert!(matches!(contact_method_update(&mut conn, &id, &wrong, &actor), Err(CommandError::Validation(_))));
        let bad = MethodInput::email("nope");
        assert!(matches!(contact_method_update(&mut conn, &id, &bad, &actor), Err(CommandError::Validation(_))));
    }

    #[test]
    fn delete_needs_confirmation_and_clears_the_caches() {
        let (mut conn, actor) = setup();
        crate::parties::refresh_caches(&conn, "p-hp").unwrap();
        let email_id = cached(&conn, "p-hp").2;
        assert!(!email_id.is_empty());
        let Err(CommandError::ConfirmationRequired(summary)) =
            contact_method_delete(&mut conn, &email_id, false, &actor)
        else {
            panic!("AP-07")
        };
        assert_eq!(summary, "Deletes the email studio@haldenpryce.example of Halden & Pryce Architects.");
        assert!(method(&conn, &email_id).unwrap().is_some(), "nothing changed");
        contact_method_delete(&mut conn, &email_id, true, &actor).unwrap();
        assert!(method(&conn, &email_id).unwrap().is_none());
        let left: i64 =
            conn.query_row("SELECT COUNT(*) FROM network_address WHERE id = ?1", [&email_id], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "the subtype row goes too");
        let (_, email, id, _) = cached(&conn, "p-hp");
        assert_ne!(id, email_id, "BR-CONTACT-05: email_address_id_cached no longer points at it");
        assert_ne!(email, "studio@haldenpryce.example");
        assert!(crate::db::foreign_key_check(&conn).unwrap().is_empty());
    }

    #[test]
    fn archived_parties_are_read_only() {
        let (mut conn, actor) = setup();
        let m = methods_of(&conn, "p-hl").unwrap().remove(0);
        conn.execute("UPDATE party SET deleted_ind = 1 WHERE id = 'p-hl'", []).unwrap();
        assert!(matches!(contact_method_set_primary(&mut conn, &m.id, &actor), Err(CommandError::Refused(_))));
        assert!(matches!(
            contact_method_create(&mut conn, "p-hl", &phone("1", false), &actor),
            Err(CommandError::Refused(_))
        ));
        assert!(matches!(
            contact_method_create(&mut conn, "nobody", &phone("1", false), &actor),
            Err(CommandError::NotFound)
        ));
    }

    #[test]
    fn us_numbers_are_formatted_when_the_setting_is_on() {
        assert_eq!(format_phone("2075550142", Some(1), true), "(207) 555-0142");
        assert_eq!(format_phone(" 207.555.0142 ", Some(1), true), "(207) 555-0142");
        assert_eq!(format_phone("+1 207 555 0142", Some(1), true), "(207) 555-0142");
        assert_eq!(format_phone("2075550142", Some(1), false), "2075550142", "setting off: as typed");
        assert_eq!(format_phone("+44 20 7946 0958", Some(44), true), "+44 20 7946 0958", "not US: as typed");
        assert_eq!(format_phone("555-0142", Some(1), true), "555-0142", "7 digits: as typed");
        assert_eq!(format_phone("1-800-FLOWERS", Some(1), true), "1-800-FLOWERS");
        assert_eq!(digits_only("(207) 555-0142"), "2075550142");
    }

    #[test]
    fn address_display_skips_empty_parts() {
        assert_eq!(
            address_display("48 Wharf Street", "Suite 3", "Portland", "ME", "04101"),
            "48 Wharf Street, Suite 3, Portland, ME 04101"
        );
        assert_eq!(address_display("12 Fore St", "", "Portland", "", "04101"), "12 Fore St, Portland, 04101");
        assert_eq!(address_display("", " ", "", "", ""), "");
    }

    #[test]
    fn address_display_sql_matches_the_rust_format() {
        let (conn, _) = open_in_memory(Seed::Reference).unwrap();
        for parts in [
            ["48 Wharf Street", "Suite 3", "Portland", "ME", "04101"],
            ["12 Fore St", "", "Portland", "", "04101"],
            ["", "", "Portland", "ME", ""],
            ["  1 Main  ", "", "", "", ""],
            ["", "", "", "", ""],
        ] {
            let sql: Option<String> = conn
                .query_row(
                    &format!(
                        "SELECT {} FROM (SELECT ?1 AS address_line_1, ?2 AS address_line_2, ?3 AS city, \
                         ?4 AS state_province, ?5 AS postal_code) a",
                        address_display_sql()
                    ),
                    parts,
                    |r| r.get(0),
                )
                .unwrap();
            let rust = address_display(parts[0], parts[1], parts[2], parts[3], parts[4]);
            assert_eq!(sql.unwrap_or_default(), rust, "{parts:?}");
        }
    }

    #[test]
    fn methods_of_a_demo_party() {
        let (conn, _) = open_in_memory(Seed::Demo).unwrap();
        let methods = methods_of(&conn, "p-ellison").unwrap();
        let kinds: Vec<_> = methods.iter().map(|m| m.kind).collect();
        assert!(kinds.contains(&MethodKind::Phone) && kinds.contains(&MethodKind::Email));
        let address = methods.iter().find(|m| m.kind == MethodKind::Address).unwrap();
        assert_eq!(address.display(), "12 Fore Street, Portland, ME 04101");
        assert!(address.primary);
        assert_eq!(method(&conn, &address.id).unwrap().unwrap(), *address);
        assert!(method(&conn, "nope").unwrap().is_none());
    }
}
