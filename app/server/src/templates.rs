//! TEMPLATE commands (UI-16): the template list and editor, the merge fields (OP-TEMPLATE-02 `merge_fields_save`) and
//! the merge (OP-TEMPLATE-01 `template_merge`). Plain SQL; each write is one `BEGIN IMMEDIATE` transaction that stamps
//! the rows it writes (BR-PARTY-07).
//!
//! Placeholders are `{{Name}}` (owner, 2026-10-28; OD-07-03, BR-TEMPLATE-01). The source wrote `<<Name>>` (kontroller
//! 478 s3, s183; MergeFieldConfig field 31): [`from_source_tokens`] turns those, and the big demo's older
//! `[First Name]` form, into `{{Name}}` when they are imported. The values come from [`FIELDS`], a fixed list in Rust:
//! there is no Evaluate, Perform Script or Set Field By Name (D-21). Merge field rows keep the delimiters in their
//! name (`{{FirstName}}`), as the source's did (`<<FirstName>>`, table 189 field 31).

use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::invoice::display_date;
use crate::money::usd;
use crate::reference::ActivityType;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

/// The start and end of a placeholder (OD-07-03). 478 takes them as parameters (s7-8; `<<` and `>>` by default).
pub const TOKEN_PREFIX: &str = "{{";
pub const TOKEN_SUFFIX: &str = "}}";

/// 478 s88: the error when a text has more starts than ends of placeholders, or the reverse.
pub const MISMATCH: &str = "token symbol mismatch";

/// One merge field of the fixed list (BR-TEMPLATE-01; plan T2, Q2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    /// What the value is, for the editor's list.
    pub help: &'static str,
}

/// The fixed list. The recipient is the email's first To (55) party and the sender its first From (54) party: 490
/// s113-137 resolves each field with "the first activity party with that role". The invoice is the activity's
/// sales-transaction link (activity_entity, entity type 12). The source names only `<<FirstName>>` and
/// `<<RecipientName>>` (478 s3, 189 f31); the other names are ours (Q2).
pub const FIELDS: &[Field] = &[
    Field { name: "RecipientName", help: "the recipient's name" },
    Field { name: "FirstName", help: "the recipient's first name" },
    Field { name: "LastName", help: "the recipient's last name" },
    Field { name: "OrganizationName", help: "the recipient's organization" },
    Field { name: "EmailAddress", help: "the recipient's email address" },
    Field { name: "PhoneNumber", help: "the recipient's phone number" },
    Field { name: "SenderName", help: "the sender's name" },
    Field { name: "SenderEmail", help: "the sender's email address" },
    Field { name: "InvoiceNumber", help: "the linked invoice's number" },
    Field { name: "InvoiceTotal", help: "the linked invoice's total" },
    Field { name: "BalanceDue", help: "the linked invoice's balance" },
    Field { name: "DueDate", help: "the linked invoice's due date" },
    Field { name: "Date", help: "today's date" },
];

/// `{{Name}}`.
pub fn token(name: &str) -> String {
    format!("{TOKEN_PREFIX}{name}{TOKEN_SUFFIX}")
}

fn known(name: &str) -> bool {
    FIELDS.iter().any(|f| f.name == name)
}

/// The name of an activity type (activity_type, kontroller 660). Layout 112 edits `Template::ActivityTypeId` as a
/// checkbox set over the ActivityType value list, so the column holds a return-separated list of ids.
pub fn activity_type_name(id: i64) -> Option<&'static str> {
    ActivityType::from_id(id).map(ActivityType::name)
}

// ---------------------------------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// The `{{` and `}}` counts differ (478 s83-88).
    Mismatch,
}

/// The distinct placeholder names of `text`, in order of appearance (478). `{{ First Name }}` is read as
/// `FirstName`: spaces inside a placeholder do not count.
pub fn parse_tokens(text: &str) -> Result<Vec<String>, TokenError> {
    if text.matches(TOKEN_PREFIX).count() != text.matches(TOKEN_SUFFIX).count() {
        return Err(TokenError::Mismatch);
    }
    let mut names: Vec<String> = Vec::new();
    for (_, inner) in segments(text).into_iter().filter_map(|s| s.token) {
        if !names.contains(&inner) {
            names.push(inner);
        }
    }
    Ok(names)
}

struct Segment<'a> {
    text: &'a str,
    /// (raw placeholder, its name without spaces) when this segment is a placeholder.
    token: Option<(&'a str, String)>,
}

/// Splits `text` into plain runs and `{{...}}` placeholders. An opening `{{` with no closing `}}` is plain text.
fn segments(text: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(TOKEN_PREFIX) {
        let after = &rest[start + TOKEN_PREFIX.len()..];
        let Some(end) = after.find(TOKEN_SUFFIX) else { break };
        let inner = &after[..end];
        // `{{a {{b}}`: the placeholder is the innermost one.
        let skip = inner.rfind(TOKEN_PREFIX).map_or(0, |i| i + TOKEN_PREFIX.len());
        let open = start + skip;
        let inner = &inner[skip..];
        let close = start + TOKEN_PREFIX.len() + end + TOKEN_SUFFIX.len();
        if open > 0 {
            out.push(Segment { text: &rest[..open], token: None });
        }
        let name: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
        out.push(Segment { text: &rest[open..close], token: Some((&rest[open..close], name)) });
        rest = &rest[close..];
    }
    if !rest.is_empty() {
        out.push(Segment { text: rest, token: None });
    }
    out
}

/// Replaces each placeholder by its value; a placeholder with no value becomes empty (490 s164; never the previous
/// field's value, which s98 carried forward: a bug, not built).
pub fn substitute(text: &str, values: &HashMap<&str, String>) -> String {
    segments(text)
        .into_iter()
        .map(|s| match s.token {
            Some((_, name)) => values.get(name.as_str()).cloned().unwrap_or_default(),
            None => s.text.to_string(),
        })
        .collect()
}

static SOURCE_ANGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<<([^<>\r\n]+?)>>").expect("regex"));
static SOURCE_BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([A-Za-z][A-Za-z ]*)\]").expect("regex"));

/// The import side of OD-07-03: the source's `<<Name>>` placeholders (478 s3, s183; table 189 field 31) become
/// `{{Name}}`, and so does `[First Name]`, the form of the big demo's older templates, when its name without spaces is
/// in [`FIELDS`] (a bracket around other text stays text). Spaces inside a name are dropped (`<<Field Name>>`, the
/// auto-enter of 189 f31, becomes `{{FieldName}}`). Borrowed when nothing changes.
pub fn from_source_tokens(text: &str) -> Cow<'_, str> {
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let angle = SOURCE_ANGLE.replace_all(text, |c: &regex::Captures| token(&squash(&c[1])));
    let out = SOURCE_BRACKET.replace_all(&angle, |c: &regex::Captures| {
        let name = squash(&c[1]);
        if known(&name) {
            token(&name)
        } else {
            c[0].to_string()
        }
    });
    if out == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(out.into_owned())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------------------------------------------

/// One row of the template list (layout 111: Name, Subject).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRow {
    pub id: i64,
    pub name: String,
    pub subject: String,
    pub activity_types: Vec<i64>,
    pub merge_fields: i64,
}

/// Every template, by name.
pub fn template_list(conn: &Connection) -> rusqlite::Result<Vec<TemplateRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, COALESCE(t.name, ''), COALESCE(t.subject, ''), COALESCE(t.activity_type_id, ''), \
         (SELECT COUNT(*) FROM merge_field m WHERE m.template_id = t.id) \
         FROM template t ORDER BY t.name IS NULL OR t.name = '', t.name COLLATE NOCASE, t.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TemplateRow {
            id: r.get(0)?,
            name: r.get(1)?,
            subject: r.get(2)?,
            activity_types: parse_type_ids(&r.get::<_, String>(3)?),
            merge_fields: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// A merge field row of the editor's portal (layout 112).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeFieldRow {
    pub id: i64,
    pub name: String,
    pub required: bool,
    /// False when the template no longer has the placeholder (the row is kept, plan T5).
    pub in_use: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDetail {
    pub id: i64,
    pub name: String,
    pub subject: String,
    pub message: String,
    pub activity_types: Vec<i64>,
    pub merge_fields: Vec<MergeFieldRow>,
}

pub fn template_get(conn: &Connection, id: i64) -> Result<TemplateDetail, CommandError> {
    let (name, subject, message, types): (String, String, String, String) = conn
        .query_row(
            "SELECT COALESCE(name, ''), COALESCE(subject, ''), COALESCE(message, ''), COALESCE(activity_type_id, '') \
             FROM template WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let used = used_tokens(&subject, &message);
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(name, ''), COALESCE(required_ind, 0) = 1 FROM merge_field WHERE template_id = ?1 \
         ORDER BY id",
    )?;
    let merge_fields = stmt
        .query_map([id], |r| {
            let name: String = r.get(1)?;
            Ok(MergeFieldRow { id: r.get(0)?, in_use: used.contains(&name), name, required: r.get(2)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(TemplateDetail { id, name, subject, message, activity_types: parse_type_ids(&types), merge_fields })
}

/// The `{{Name}}` placeholders of a subject and message, as merge field names. A text with a mismatch has none.
fn used_tokens(subject: &str, message: &str) -> Vec<String> {
    [subject, message].iter().flat_map(|t| parse_tokens(t).unwrap_or_default()).map(|n| token(&n)).collect()
}

fn parse_type_ids(text: &str) -> Vec<i64> {
    text.split(['\r', '\n', ',']).filter_map(|s| s.trim().parse().ok()).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// template_create, template_save (layout 112 edits in place; 05 lacks these: plan T5)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateInput {
    pub name: String,
    pub subject: String,
    pub message: String,
    pub activity_types: Vec<i64>,
}

/// Checks the placeholders of the subject and the message (BR-TEMPLATE-01: the editor refuses an unknown one) and
/// the activity types. Returns the placeholder names in order.
fn validate(input: &TemplateInput) -> Result<Vec<String>, CommandError> {
    let mut errors = Vec::new();
    let mut names = Vec::new();
    for (field, text) in [("subject", &input.subject), ("message", &input.message)] {
        match parse_tokens(text) {
            Err(TokenError::Mismatch) => errors.push(FieldError {
                field,
                message: format!(
                    "Token symbol mismatch: every {TOKEN_PREFIX} needs a closing {TOKEN_SUFFIX} ({MISMATCH})."
                ),
            }),
            Ok(found) => {
                let unknown: Vec<String> = found.iter().filter(|n| !known(n)).map(|n| token(n)).collect();
                if !unknown.is_empty() {
                    errors.push(FieldError {
                        field,
                        message: format!("Unknown merge field {}. Use one from the list.", unknown.join(", ")),
                    });
                }
                for n in found {
                    if !names.contains(&n) {
                        names.push(n);
                    }
                }
            }
        }
    }
    if input.activity_types.iter().any(|t| ActivityType::from_id(*t).is_none()) {
        errors.push(FieldError { field: "activity_types", message: "Choose activity types from the list.".into() });
    }
    if errors.is_empty() {
        Ok(names)
    } else {
        Err(CommandError::Validation(errors))
    }
}

fn type_ids_text(types: &[i64]) -> Option<String> {
    let mut types = types.to_vec();
    types.sort_unstable();
    types.dedup();
    (!types.is_empty()).then(|| types.iter().map(i64::to_string).collect::<Vec<_>>().join("\n"))
}

/// The list's New button: an empty template, or one with the given fields. Returns the new id (a serial, as the
/// source's Template::Id; table 187 field 1).
pub fn template_create(conn: &mut Connection, input: &TemplateInput, actor: &Actor) -> Result<i64, CommandError> {
    let input = TemplateInput { subject: input.subject.trim().to_string(), ..input.clone() };
    let names = validate(&input)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stamp = Stamp::now(actor);
    let id: i64 = tx.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM template", [], |r| r.get(0))?;
    tx.execute(
        "INSERT INTO template (id, name, subject, message, activity_type_id, user_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            non_empty(&input.name),
            non_empty(&input.subject),
            non_empty(&input.message),
            type_ids_text(&input.activity_types),
            actor.user_id
        ],
    )?;
    audit::stamp_created(&tx, "template", &id, &stamp)?;
    sync_merge_fields(&tx, id, &names, &stamp)?;
    tx.commit()?;
    Ok(id)
}

/// Saves the name, subject, message and activity types (layout 112) and creates the merge field row of each new
/// placeholder, matched on (template_id, name) (480 s95). A row whose placeholder is gone is kept; the editor marks
/// it "not in use". The subject is trimmed (table 187 field 6 auto-enter `Trim ( Self )`).
pub fn template_save(conn: &mut Connection, id: i64, input: &TemplateInput, actor: &Actor) -> Result<(), CommandError> {
    let input = TemplateInput { subject: input.subject.trim().to_string(), ..input.clone() };
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM template WHERE id = ?1)", [id], |r| r.get(0))?;
    if !exists {
        return Err(CommandError::NotFound);
    }
    let names = validate(&input)?;
    let stamp = Stamp::now(actor);
    tx.execute(
        "UPDATE template SET name = ?2, subject = ?3, message = ?4, activity_type_id = ?5 WHERE id = ?1",
        params![
            id,
            non_empty(&input.name),
            non_empty(&input.subject),
            non_empty(&input.message),
            type_ids_text(&input.activity_types)
        ],
    )?;
    audit::stamp_modified(&tx, "template", &id, &stamp)?;
    sync_merge_fields(&tx, id, &names, &stamp)?;
    tx.commit()?;
    Ok(())
}

fn non_empty(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// One merge field row per placeholder name, created when missing (480 s95 cascadeCreate on TemplateId + Name).
fn sync_merge_fields(conn: &Connection, template_id: i64, names: &[String], stamp: &Stamp) -> rusqlite::Result<()> {
    for name in names {
        let name = token(name);
        let exists: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM merge_field WHERE template_id = ?1 AND name = ?2)",
            params![template_id, name],
            |r| r.get(0),
        )?;
        if !exists {
            let id: i64 = conn.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM merge_field", [], |r| r.get(0))?;
            conn.execute(
                "INSERT INTO merge_field (id, template_id, name, required_ind) VALUES (?1, ?2, ?3, 0)",
                params![id, template_id, name],
            )?;
            audit::stamp_created(conn, "merge_field", &id, stamp)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-TEMPLATE-02 merge_fields_save (BR-TEMPLATE-02)
// ---------------------------------------------------------------------------------------------------------------

/// Sets required_ind of every merge field of the template from the editor's checkboxes: the ids in `required` get 1,
/// the others 0 (480 s105-106 overwrite the row's properties; 481 s85 clears RequiredInd). Nothing reads required_ind
/// at merge or send time (plan T6). An id that is not one of this template's rows is Validation.
pub fn merge_fields_save(
    conn: &mut Connection,
    template_id: i64,
    required: &[i64],
    actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: bool =
        tx.query_row("SELECT EXISTS (SELECT 1 FROM template WHERE id = ?1)", [template_id], |r| r.get(0))?;
    if !exists {
        return Err(CommandError::NotFound);
    }
    let rows: Vec<(i64, bool)> = {
        let mut stmt =
            tx.prepare("SELECT id, COALESCE(required_ind, 0) = 1 FROM merge_field WHERE template_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([template_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    if required.iter().any(|id| !rows.iter().any(|(r, _)| r == id)) {
        return Err(CommandError::Validation(vec![FieldError {
            field: "required",
            message: "That merge field is not on this template.".into(),
        }]));
    }
    let stamp = Stamp::now(actor);
    for (id, was) in rows {
        let now = required.contains(&id);
        if now != was {
            tx.execute("UPDATE merge_field SET required_ind = ?2 WHERE id = ?1", params![id, i64::from(now)])?;
            audit::stamp_modified(&tx, "merge_field", &id, &stamp)?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-TEMPLATE-01 template_merge (BR-TEMPLATE-01)
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub subject: String,
    pub message: String,
}

/// The value of each field of [`FIELDS`] for an activity. A field with nothing to show is absent.
pub fn merge_values(
    conn: &Connection,
    activity_id: &str,
    today: &str,
) -> rusqlite::Result<HashMap<&'static str, String>> {
    let mut v: HashMap<&'static str, String> = HashMap::new();
    let put = |v: &mut HashMap<&'static str, String>, k: &'static str, s: Option<String>| {
        if let Some(s) = s.filter(|s| !s.trim().is_empty()) {
            v.insert(k, s);
        }
    };
    // The first party of a role (490 s113-137): the oldest row.
    let party_sql = "SELECT COALESCE(p.name_cached, ap.party_name_cached), pe.first_name, pe.last_name, \
         CASE WHEN p.organization_ind = 1 THEN COALESCE(o.organization_name, p.name_cached) ELSE pe.employer_name END, \
         COALESCE(NULLIF(ape.email_address, ''), p.email_address_cached), p.phone_number_cached \
         FROM activity_party ap LEFT JOIN party p ON p.id = ap.party_id LEFT JOIN person pe ON pe.id = ap.party_id \
         LEFT JOIN organization o ON o.id = ap.party_id LEFT JOIN activity_party_email ape ON ape.id = ap.id \
         WHERE ap.activity_id = ?1 AND ap.activity_party_role_id = ?2 \
         ORDER BY ap.creation_timestamp IS NULL, ap.creation_timestamp, ap.rowid LIMIT 1";
    type PartyValues = (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
    let party = |role: i64| -> rusqlite::Result<Option<PartyValues>> {
        conn.query_row(party_sql, params![activity_id, role], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .optional()
    };
    if let Some((name, first, last, org, email, phone)) = party(55)? {
        put(&mut v, "RecipientName", name);
        put(&mut v, "FirstName", first);
        put(&mut v, "LastName", last);
        put(&mut v, "OrganizationName", org);
        put(&mut v, "EmailAddress", email);
        put(&mut v, "PhoneNumber", phone);
    }
    if let Some((name, _, _, _, email, _)) = party(54)? {
        put(&mut v, "SenderName", name);
        put(&mut v, "SenderEmail", email);
    }
    let invoice = conn
        .query_row(
            "SELECT st.doc_number, st.total_amt, st.balance, st.due_date FROM activity_entity ae \
             JOIN sales_transaction st ON st.id = ae.entity_id \
             WHERE ae.activity_id = ?1 AND ae.entity_type_id = 12 ORDER BY ae.rowid LIMIT 1",
            [activity_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some((number, total, balance, due)) = invoice {
        put(&mut v, "InvoiceNumber", number);
        put(&mut v, "InvoiceTotal", total.map(usd));
        put(&mut v, "BalanceDue", balance.map(usd));
        put(&mut v, "DueDate", due.map(|d| display_date(&d)));
    }
    put(&mut v, "Date", Some(display_date(today)));
    Ok(v)
}

/// Merges a template for an activity: each placeholder of the subject and the message becomes its value, or empty
/// (490 s164). A template with no merge field rows merges too (490 s88-90 refused it only because the config lookup
/// needed them). Nothing is written: the caller writes the result (T4, the email composer) only when this succeeds.
pub fn template_merge(conn: &Connection, template_id: i64, activity_id: &str) -> Result<Merged, CommandError> {
    template_merge_at(conn, template_id, activity_id, &audit::local_now()[..10])
}

/// [`template_merge`] with `{{Date}}` read from `today` (`AppState::today`).
pub fn template_merge_at(
    conn: &Connection,
    template_id: i64,
    activity_id: &str,
    today: &str,
) -> Result<Merged, CommandError> {
    let (subject, message): (String, String) = conn
        .query_row(
            "SELECT COALESCE(subject, ''), COALESCE(message, '') FROM template WHERE id = ?1",
            [template_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let exists: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM activity WHERE id = ?1)", [activity_id], |r| r.get(0))?;
    if !exists {
        return Err(CommandError::NotFound);
    }
    let values = merge_values(conn, activity_id, today)?;
    Ok(Merged { subject: substitute(&subject, &values), message: substitute(&message, &values) })
}

/// A placeholder of a preview that has no value on the chosen email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingField {
    pub token: String,
    pub help: &'static str,
}

/// What the preview shows: the merge, and why a part of it may be empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePreview {
    pub merged: Merged,
    /// The subject and the message are both empty: there was nothing to merge.
    pub nothing_to_merge: bool,
    pub missing: Vec<MissingField>,
}

/// The preview of layout 112: merges this text, which may be unsaved, for an activity. It refuses what
/// `template_save` refuses (Validation) and writes nothing. A field with no value merges as empty (490 s164), as
/// [`template_merge`] does, and is named in `missing`.
pub fn merge_preview(
    conn: &Connection,
    activity_id: &str,
    subject: &str,
    message: &str,
    today: &str,
) -> Result<MergePreview, CommandError> {
    let exists: bool =
        conn.query_row("SELECT EXISTS (SELECT 1 FROM activity WHERE id = ?1)", [activity_id], |r| r.get(0))?;
    if !exists {
        return Err(CommandError::NotFound);
    }
    let subject = subject.trim();
    let input = TemplateInput { subject: subject.into(), message: message.into(), ..Default::default() };
    let names = validate(&input)?;
    let values = merge_values(conn, activity_id, today)?;
    let missing = names
        .iter()
        .filter(|n| !values.contains_key(n.as_str()))
        .filter_map(|n| FIELDS.iter().find(|f| f.name == n))
        .map(|f| MissingField { token: token(f.name), help: f.help })
        .collect();
    Ok(MergePreview {
        merged: Merged { subject: substitute(subject, &values), message: substitute(message, &values) },
        nothing_to_merge: subject.is_empty() && message.trim().is_empty(),
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_parsed_in_order_without_repeats() {
        assert_eq!(
            parse_tokens("Hi {{FirstName}}, {{ Invoice Number }} {{FirstName}}").unwrap(),
            ["FirstName", "InvoiceNumber"]
        );
        assert_eq!(parse_tokens("no tokens").unwrap(), Vec::<String>::new());
        assert_eq!(parse_tokens("").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn unequal_delimiters_are_a_mismatch() {
        assert_eq!(parse_tokens("Hi {{FirstName"), Err(TokenError::Mismatch));
        assert_eq!(parse_tokens("Hi FirstName}}"), Err(TokenError::Mismatch));
        assert_eq!(parse_tokens("{{a}} }}"), Err(TokenError::Mismatch));
    }

    #[test]
    fn substitute_uses_empty_for_a_missing_value_never_the_previous_one() {
        let values = HashMap::from([("FirstName", "Jane".to_string())]);
        assert_eq!(substitute("Hi {{FirstName}} {{LastName}}!", &values), "Hi Jane !");
        assert_eq!(substitute("{{LastName}}{{FirstName}}{{LastName}}", &values), "Jane");
        assert_eq!(substitute("plain {text}", &values), "plain {text}");
    }

    #[test]
    fn source_tokens_become_braces() {
        assert_eq!(from_source_tokens("Hi <<FirstName>>, <<Field Name>>"), "Hi {{FirstName}}, {{FieldName}}");
        assert_eq!(
            from_source_tokens("Hi [First Name], invoice [Invoice Number] [Project] [x]"),
            "Hi {{FirstName}}, invoice {{InvoiceNumber}} [Project] [x]"
        );
        assert!(matches!(from_source_tokens("nothing [here]"), Cow::Borrowed(_)));
        assert!(matches!(from_source_tokens("{{FirstName}}"), Cow::Borrowed(_)));
        assert_eq!(from_source_tokens("a << b >> c"), "a {{b}} c");
    }

    #[test]
    fn every_field_name_is_a_valid_token() {
        for f in FIELDS {
            assert_eq!(parse_tokens(&token(f.name)).unwrap(), [f.name]);
            assert_eq!(from_source_tokens(&format!("<<{}>>", f.name)), token(f.name));
        }
    }

    #[test]
    fn organization_name_of_a_person_is_their_employer() {
        // B2: the seed email INV-1048 goes To Joanna Whitfield of Halden & Pryce. A person's OrganizationName is
        // Person::EmployerName (156 f131), which a person made with their organization takes (kontroller 229 s229).
        let (conn, _) = crate::db::open_in_memory(crate::db::Seed::Demo).unwrap();
        let v = merge_values(&conn, "a-mail-1048", &audit::local_now()[..10]).unwrap();
        assert_eq!(v.get("FirstName").map(String::as_str), Some("Joanna"));
        assert_eq!(v.get("OrganizationName").map(String::as_str), Some("Halden & Pryce Architects"));
    }

    #[test]
    fn type_ids_round_trip() {
        assert_eq!(parse_type_ids("1\r2\n3"), [1, 2, 3]);
        assert_eq!(type_ids_text(&[3, 1, 1]).as_deref(), Some("1\n3"));
        assert_eq!(type_ids_text(&[]), None);
    }
}
