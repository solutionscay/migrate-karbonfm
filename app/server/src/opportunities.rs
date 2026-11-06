//! OP-SALES-01 `opportunity_save`, OP-SALES-02 `opportunity_set_status`, OP-SALES-03 `opportunity_status_totals`
//! (BR-SALES-12, 13, 14). Plain SQL, one `BEGIN IMMEDIATE` transaction per write, stamped (BR-PARTY-07). Each command
//! takes today's date, so the route decides the clock. Form parsing and rendering are in `routes::opportunities`.

use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::ids::new_id;
use crate::parties::RoleKind;
use crate::sales_list::paging;
use karbon_money::Cents;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, TransactionBehavior};

/// Value list 43 OpportunityProbability.
pub const PROBABILITIES: [&str; 3] = ["unlikely", "likely", "definite"];
/// The default of a new opportunity (kontroller 602 s56).
pub const UNKNOWN: &str = "unknown";
/// Every new opportunity gets this status (602 s145-146).
pub const STATUS_NEW: i64 = 1;
/// The "Opportunity" row of entity_type (activities and files link with it).
pub const ENTITY_TYPE: i64 = 13;

/// The status name of a row: the cached one (field 50), else the status's own (the small seed leaves the cache empty).
const STATUS_NAME: &str = "COALESCE(NULLIF(o.status_cached, ''), s.name, '')";
const FROM: &str = "FROM opportunity o LEFT JOIN opportunity_status s ON s.id = o.opportunity_status_id \
                    LEFT JOIN party p ON p.id = o.party_id";

fn invalid(field: &'static str, message: &str) -> FieldError {
    FieldError { field, message: message.to_owned() }
}

fn is_date(s: &str) -> bool {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

/// Empty text is NULL, as a FileMaker empty field is.
fn text(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-SALES-01 create
// ---------------------------------------------------------------------------------------------------------------

/// Where a new opportunity starts, which decides its lead link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreatedFrom {
    /// The list's dialog (kontacts 615): a party that is no customer is treated as a lead; with no lead role it gets
    /// one in this transaction (kontacts 602 s8).
    #[default]
    Picker,
    /// A party page (kontacts 610 s3 passes PartyId only): no lead link and no new role, so a lead's opportunity reads
    /// "Customer" there. A source bug, built as intended (D-07).
    PartyPage,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewOpportunity {
    pub party_id: String,
    pub name: String,
    pub from: CreatedFrom,
}

/// O1, O2, O3, O4 (BR-SALES-13). Returns the new id.
pub fn opportunity_create(
    conn: &mut Connection,
    new: &NewOpportunity,
    today: &str,
    actor: &Actor,
) -> Result<String, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let party_id = new.party_id.trim();
    let party_name: Option<Option<String>> =
        tx.query_row("SELECT name_cached FROM party WHERE id = ?1", [party_id], |r| r.get(0)).optional()?;
    let Some(party_name) = party_name else {
        return Err(CommandError::Validation(vec![invalid("party_id", "Choose a person or organization.")]));
    };
    let stamp = Stamp::now(actor);
    // 602 s116-120: any customer role, current or not, clears the lead link.
    let customer: Option<String> = tx
        .query_row(
            "SELECT id FROM party_role WHERE party_id = ?1 AND COALESCE(customer_ind, 0) = 1 LIMIT 1",
            [party_id],
            |r| r.get(0),
        )
        .optional()?;
    let lead_id = match (new.from, customer) {
        (CreatedFrom::PartyPage, _) | (_, Some(_)) => None,
        (CreatedFrom::Picker, None) => {
            let current: Option<String> = tx
                .query_row(
                    "SELECT id FROM party_role WHERE party_id = ?1 AND COALESCE(lead_ind, 0) = 1
                     AND COALESCE(current_ind, 0) = 1 LIMIT 1",
                    [party_id],
                    |r| r.get(0),
                )
                .optional()?;
            Some(match current {
                Some(id) => id,
                None => crate::parties::add_role(&tx, party_id, RoleKind::Lead, false, &stamp)?,
            })
        }
    };
    let id = new_id();
    tx.execute(
        "INSERT INTO opportunity (id, party_id, party_name_cached, name, probability, start_date, lead_id, type_display,
                                  salesperson_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            party_id,
            party_name,
            text(&new.name),
            UNKNOWN,
            today,
            lead_id,
            if lead_id.is_some() { "Lead" } else { "Customer" },
            salesperson_default(&tx, actor)?,
        ],
    )?;
    set_status_in(&tx, &id, STATUS_NEW, "", today, &stamp)?;
    audit::stamp_created(&tx, "opportunity", &id, &stamp)?;
    tx.commit()?;
    Ok(id)
}

/// The current user's employee role (the way kontacts 615 s7-8 works it out; 602 s12 promises it, s55-57 omit it).
fn salesperson_default(conn: &Connection, actor: &Actor) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT r.id FROM party_role r JOIN user u ON u.party_id = r.party_id
         WHERE u.id = ?1 AND COALESCE(r.employee_ind, 0) = 1 LIMIT 1",
        [&actor.user_id],
        |r| r.get(0),
    )
    .optional()
}

// ---------------------------------------------------------------------------------------------------------------
// OP-SALES-01 update
// ---------------------------------------------------------------------------------------------------------------

/// The referring party on a save (kontacts 722 s38-39 copies the picked id and name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Referring {
    #[default]
    Keep,
    Clear,
    Set(String),
}

/// The card's fields (layouts 156, 162). O6: status, active and party do not change here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpportunityEdit {
    pub name: String,
    pub value: String,
    pub probability: String,
    pub start_date: String,
    pub end_date: String,
    pub salesperson_id: String,
    pub notes: String,
    pub primary_contact_party_id: String,
    pub referring: Referring,
}

pub fn opportunity_update(
    conn: &mut Connection,
    id: &str,
    edit: &OpportunityEdit,
    actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (party_id, old_probability, old_salesperson, old_contact): (String, String, String, String) = tx
        .query_row(
            "SELECT COALESCE(party_id, ''), COALESCE(probability, ''), COALESCE(salesperson_id, ''),
                    COALESCE(primary_contact_party_id, '')
             FROM opportunity WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(CommandError::NotFound)?;

    let mut errors = Vec::new();
    let value = Cents::parse_optional(&edit.value);
    if value.is_err() {
        errors.push(invalid("value", "Enter an amount with at most 2 decimals, like 1,200.00."));
    }
    // O7: the list plus "unknown"; a stored value outside it (the seed's "60%") stays until it is changed.
    let probability = edit.probability.trim();
    let offered = PROBABILITIES.contains(&probability) || probability == UNKNOWN || probability == old_probability;
    if !probability.is_empty() && !offered {
        errors.push(invalid("probability", "Choose a probability from the list."));
    }
    for (field, value) in [("start_date", &edit.start_date), ("end_date", &edit.end_date)] {
        if text(value).is_some_and(|d| !is_date(d)) {
            errors.push(invalid(field, "Enter a date."));
        }
    }
    let salesperson = edit.salesperson_id.trim();
    if !salesperson.is_empty() && salesperson != old_salesperson {
        let known: bool =
            tx.query_row("SELECT EXISTS (SELECT 1 FROM employee WHERE id = ?1)", [salesperson], |r| r.get(0))?;
        if !known {
            errors.push(invalid("salesperson_id", "Choose a salesperson from the list."));
        }
    }
    // Value list 54: the party's related parties.
    let contact = edit.primary_contact_party_id.trim();
    if !contact.is_empty() && contact != old_contact {
        let related = crate::parties::relations_of(&tx, &party_id)?.iter().any(|r| r.other_id == contact);
        if !related {
            errors.push(invalid("primary_contact_party_id", "Choose one of this party's contacts."));
        }
    }
    let referring: Option<(Option<String>, Option<String>)> = match &edit.referring {
        Referring::Keep => None,
        Referring::Clear => Some((None, None)),
        Referring::Set(other) => {
            let name: Option<Option<String>> =
                tx.query_row("SELECT name_cached FROM party WHERE id = ?1", [other], |r| r.get(0)).optional()?;
            match name {
                Some(name) => Some((Some(other.clone()), name)),
                None => {
                    errors.push(invalid("referring_party_id", "That party does not exist."));
                    None
                }
            }
        }
    };
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }

    tx.execute(
        "UPDATE opportunity SET name = ?2, value = ?3, probability = ?4, start_date = ?5, end_date = ?6,
                salesperson_id = ?7, notes = ?8, primary_contact_party_id = ?9 WHERE id = ?1",
        params![
            id,
            text(&edit.name),
            value.ok().flatten().map(|c| c.0),
            text(probability),
            text(&edit.start_date),
            text(&edit.end_date),
            text(salesperson),
            text(&edit.notes),
            text(contact),
        ],
    )?;
    if let Some((referring_id, referring_name)) = referring {
        tx.execute(
            "UPDATE opportunity SET referring_party_id = ?2, referring_party_name_cached = ?3 WHERE id = ?1",
            params![id, referring_id, referring_name],
        )?;
    }
    audit::stamp_modified(&tx, "opportunity", &id, &Stamp::now(actor))?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-SALES-02 status (BR-SALES-12)
// ---------------------------------------------------------------------------------------------------------------

/// `status_id` is the form's text; an unknown status is Validation (O5). `end_date` empty is today (608 s45).
pub fn opportunity_set_status(
    conn: &mut Connection,
    id: &str,
    status_id: &str,
    end_date: &str,
    today: &str,
    actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM opportunity WHERE id = ?1)", [id], |r| r.get(0))?;
    if !exists {
        return Err(CommandError::NotFound);
    }
    let Ok(status) = status_id.trim().parse::<i64>() else {
        return Err(CommandError::Validation(vec![invalid("status_id", "Choose a status.")]));
    };
    if text(end_date).is_some_and(|d| !is_date(d)) {
        return Err(CommandError::Validation(vec![invalid("end_date", "Enter a date.")]));
    }
    let stamp = Stamp::now(actor);
    set_status_in(&tx, id, status, end_date, today, &stamp)?;
    audit::stamp_modified(&tx, "opportunity", &id, &stamp)?;
    tx.commit()?;
    Ok(())
}

/// BR-SALES-12 inside the caller's transaction (608 s101-125). A create and a status change go through here (O4).
fn set_status_in(
    tx: &Connection,
    id: &str,
    status: i64,
    end_date: &str,
    today: &str,
    stamp: &Stamp,
) -> Result<(), CommandError> {
    let found: Option<(Option<String>, bool)> = tx
        .query_row("SELECT name, COALESCE(active_ind, 0) <> 0 FROM opportunity_status WHERE id = ?1", [status], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((name, active)) = found else {
        return Err(CommandError::Validation(vec![invalid("status_id", "That is not a status.")]));
    };
    let closed_on = text(end_date).unwrap_or(today);
    tx.execute(
        "UPDATE opportunity SET opportunity_status_id = ?2, status_cached = ?3, active_ind = ?4, end_date = ?5
         WHERE id = ?1",
        params![id, status, name, i64::from(active), if active { None } else { Some(closed_on) }],
    )?;
    let lead: Option<String> =
        tx.query_row("SELECT lead_id FROM opportunity WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
    if let Some(lead) = lead {
        tx.execute(
            "UPDATE party_role SET current_ind = ?2, thru_date = ?3 WHERE id = ?1",
            params![lead, i64::from(active), if active { None } else { Some(closed_on) }],
        )?;
        audit::stamp_modified(tx, "party_role", &lead, stamp)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-SALES-03 totals and the list (BR-SALES-14)
// ---------------------------------------------------------------------------------------------------------------

/// Which opportunities a list or chart covers (682 s36: active by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveFilter {
    #[default]
    Active,
    Inactive,
    All,
}

impl ActiveFilter {
    /// `?active=`: 1 (the default), 0 or all. Anything else is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "" | "1" => Some(ActiveFilter::Active),
            "0" => Some(ActiveFilter::Inactive),
            "all" => Some(ActiveFilter::All),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            ActiveFilter::Active => "1",
            ActiveFilter::Inactive => "0",
            ActiveFilter::All => "all",
        }
    }
    fn clause(self) -> &'static str {
        match self {
            ActiveFilter::Active => "COALESCE(o.active_ind, 0) <> 0",
            ActiveFilter::Inactive => "COALESCE(o.active_ind, 0) = 0",
            ActiveFilter::All => "1 = 1",
        }
    }
}

/// One status with its total (682 s103) and its number of opportunities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTotal {
    pub status: String,
    pub value: i64,
    pub count: i64,
}

/// The WHERE clause and bound values of a list: O9's search covers the name, the party's name and the notes.
fn list_filter(q: &str, active: ActiveFilter) -> (String, Vec<Value>) {
    let text = q.trim().to_lowercase();
    if text.is_empty() {
        return (active.clause().to_owned(), Vec::new());
    }
    let sql = format!(
        "{} AND (instr(lower(COALESCE(o.name, '')), ?1) > 0 OR instr(lower(COALESCE(p.name_cached, '')), ?1) > 0
                 OR instr(lower(COALESCE(o.notes, '')), ?1) > 0)",
        active.clause()
    );
    (sql, vec![Value::Text(text)])
}

fn totals(conn: &Connection, filter: &str, values: &[Value]) -> rusqlite::Result<Vec<StatusTotal>> {
    let sql = format!(
        "SELECT {STATUS_NAME} AS status, COALESCE(SUM(o.value), 0), COUNT(*) {FROM} WHERE {filter}
         GROUP BY status ORDER BY status COLLATE NOCASE"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), |r| {
        Ok(StatusTotal { status: r.get(0)?, value: r.get(1)?, count: r.get(2)? })
    })?;
    rows.collect()
}

/// OP-SALES-03 (BR-SALES-14; O8): the total value per status name, ascending.
pub fn opportunity_status_totals(conn: &Connection, active: ActiveFilter) -> rusqlite::Result<Vec<StatusTotal>> {
    totals(conn, active.clause(), &[])
}

/// Next Task (layout 153; kontacts rel 214): an incomplete task linked through activity_entity type 13. Q3: the
/// relationship has no sort, so the earliest date, then time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NextTask {
    pub activity_id: String,
    pub subject: String,
    pub date: String,
}

/// One list row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    pub id: String,
    pub name: String,
    pub party_id: String,
    pub party: String,
    pub status: String,
    pub value: Option<i64>,
    pub probability: String,
    pub active: bool,
    pub next_task: Option<NextTask>,
}

/// One page of the list. `groups` holds every matched status, so a subtotal covers rows on other pages too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPage {
    pub rows: Vec<ListRow>,
    pub groups: Vec<StatusTotal>,
    pub matched: i64,
    pub page: i64,
    pub pages: i64,
    pub page_size: i64,
    pub total: i64,
}

const ROW_SELECT: &str =
    "SELECT o.id, COALESCE(o.name, ''), COALESCE(o.party_id, ''), COALESCE(p.name_cached, o.party_name_cached, ''),
    COALESCE(NULLIF(o.status_cached, ''), s.name, '') AS status, o.value, COALESCE(o.probability, ''),
    COALESCE(o.active_ind, 0) <> 0";

fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ListRow> {
    Ok(ListRow {
        id: r.get(0)?,
        name: r.get(1)?,
        party_id: r.get(2)?,
        party: r.get(3)?,
        status: r.get(4)?,
        value: r.get(5)?,
        probability: r.get(6)?,
        active: r.get(7)?,
        next_task: None,
    })
}

/// Fills Next Task with one query per page, not one per row.
fn fill_next_tasks(conn: &Connection, rows: &mut [ListRow]) -> rusqlite::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let marks = (1..=rows.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT ae.entity_id, a.id, COALESCE(a.subject, ''), COALESCE(a.date_display, '')
         FROM activity_entity ae
         JOIN activity a ON a.id = ae.activity_id
         JOIN task t ON t.id = a.id AND t.completed_at IS NULL
         WHERE ae.entity_id IN ({marks}) AND CAST(ae.entity_type_id AS INTEGER) = {ENTITY_TYPE}
         ORDER BY a.date_display IS NULL, a.date_display, a.time_display IS NULL, a.time_display, a.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let found = stmt.query_map(params_from_iter(rows.iter().map(|r| r.id.as_str())), |r| {
        Ok((r.get::<_, String>(0)?, NextTask { activity_id: r.get(1)?, subject: r.get(2)?, date: r.get(3)? }))
    })?;
    for item in found {
        let (opportunity, task) = item?;
        if let Some(row) = rows.iter_mut().find(|r| r.id == opportunity && r.next_task.is_none()) {
            row.next_task = Some(task);
        }
    }
    Ok(())
}

/// The list (layout 153; 767 s15, s18; 789): by status, then name.
pub fn opportunity_list(
    conn: &Connection,
    q: &str,
    active: ActiveFilter,
    page: i64,
    page_size: i64,
) -> rusqlite::Result<ListPage> {
    let (filter, values) = list_filter(q, active);
    let groups = totals(conn, &filter, &values)?;
    let matched = groups.iter().map(|g| g.count).sum();
    let total = groups.iter().map(|g| g.value).sum();
    let (page, pages, size) = paging(matched, page, page_size);
    let sql = format!(
        "{ROW_SELECT} {FROM} WHERE {filter} ORDER BY status COLLATE NOCASE, lower(COALESCE(o.name, '')), o.id
         LIMIT {size} OFFSET {}",
        (page - 1) * size
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows: Vec<ListRow> =
        stmt.query_map(params_from_iter(values.iter()), read_row)?.collect::<rusqlite::Result<_>>()?;
    fill_next_tasks(conn, &mut rows)?;
    Ok(ListPage { rows, groups, matched, page, pages, page_size: size, total })
}

/// A party's opportunities, active ones first (O12, layout 156's party side).
pub fn opportunities_of_party(conn: &Connection, party_id: &str) -> rusqlite::Result<Vec<ListRow>> {
    let sql = format!(
        "{ROW_SELECT} {FROM} WHERE o.party_id = ?1
         ORDER BY COALESCE(o.active_ind, 0) = 0, status COLLATE NOCASE, lower(COALESCE(o.name, '')), o.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows: Vec<ListRow> = stmt.query_map([party_id], read_row)?.collect::<rusqlite::Result<_>>()?;
    fill_next_tasks(conn, &mut rows)?;
    Ok(rows)
}

// ---------------------------------------------------------------------------------------------------------------
// Read: one opportunity, and the lists its card offers
// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Opportunity {
    pub id: String,
    pub name: String,
    pub party_id: String,
    pub party: String,
    pub status_id: Option<i64>,
    pub status: String,
    pub active: bool,
    pub type_display: String,
    pub value: Option<i64>,
    pub probability: String,
    pub start_date: String,
    pub end_date: String,
    pub salesperson_id: String,
    pub salesperson: String,
    pub notes: String,
    pub primary_contact_party_id: String,
    pub primary_contact: String,
    pub referring_party_id: String,
    pub referring: String,
    pub created_by: String,
    pub created_at: String,
}

pub fn opportunity_get(conn: &Connection, id: &str) -> Result<Opportunity, CommandError> {
    let sql = format!(
        "SELECT o.id, COALESCE(o.name, ''), COALESCE(o.party_id, ''), COALESCE(p.name_cached, o.party_name_cached, ''),
                CAST(o.opportunity_status_id AS INTEGER), {STATUS_NAME}, COALESCE(o.active_ind, 0) <> 0,
                COALESCE(NULLIF(o.type_display, ''), CASE WHEN o.lead_id IS NOT NULL THEN 'Lead' ELSE 'Customer' END),
                o.value, COALESCE(o.probability, ''), COALESCE(o.start_date, ''), COALESCE(o.end_date, ''),
                COALESCE(o.salesperson_id, ''), COALESCE((SELECT name_cached FROM employee WHERE id = o.salesperson_id), ''),
                COALESCE(o.notes, ''), COALESCE(o.primary_contact_party_id, ''),
                COALESCE((SELECT name_cached FROM party WHERE id = o.primary_contact_party_id), ''),
                COALESCE(o.referring_party_id, ''),
                COALESCE((SELECT name_cached FROM party WHERE id = o.referring_party_id), o.referring_party_name_cached, ''),
                COALESCE(o.creation_account_name, ''), COALESCE(o.creation_timestamp, '')
         {FROM} WHERE o.id = ?1"
    );
    conn.query_row(&sql, [id], |r| {
        Ok(Opportunity {
            id: r.get(0)?,
            name: r.get(1)?,
            party_id: r.get(2)?,
            party: r.get(3)?,
            status_id: r.get(4)?,
            status: r.get(5)?,
            active: r.get(6)?,
            type_display: r.get(7)?,
            value: r.get(8)?,
            probability: r.get(9)?,
            start_date: r.get(10)?,
            end_date: r.get(11)?,
            salesperson_id: r.get(12)?,
            salesperson: r.get(13)?,
            notes: r.get(14)?,
            primary_contact_party_id: r.get(15)?,
            primary_contact: r.get(16)?,
            referring_party_id: r.get(17)?,
            referring: r.get(18)?,
            created_by: r.get(19)?,
            created_at: r.get(20)?,
        })
    })
    .optional()?
    .ok_or(CommandError::NotFound)
}

/// The status menu (layout 156's portal of OpportunityStatus): (id, name), by id as the source seeds them.
pub fn statuses(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT CAST(id AS INTEGER), COALESCE(name, '') FROM opportunity_status ORDER BY CAST(id AS INTEGER)",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// Value list 44 Employee#current: (employee id, name), by name.
pub fn current_employees(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, COALESCE(e.name_cached, '') FROM employee e JOIN party_role r ON r.id = e.id
         WHERE COALESCE(r.current_ind, 0) = 1 ORDER BY lower(COALESCE(e.name_cached, '')), e.id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}
