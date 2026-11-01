//! PARTY commands of I2 (05 OP-PARTY-*, 07 BR-PARTY-*): search, read, create, edit, archive, roles, relations and the
//! create-party wizard. Plain SQL; each write command is one `BEGIN IMMEDIATE` transaction and stamps every row it
//! writes (BR-PARTY-07). Routes parse forms and render; they never run SQL (AP-01).

use crate::admin::{CommandError, FieldError};
use crate::audit::{self, Actor, Stamp};
use crate::contacts::{self, Method, MethodInput};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, TransactionBehavior};

// ---------------------------------------------------------------------------------------------------------------
// OP-PARTY-05 party_search
// ---------------------------------------------------------------------------------------------------------------

/// Rows per page of the party list (05 query contract: page 50).
pub const PAGE_SIZE: i64 = 50;
/// The search returns at most this many rows (05: 500 rows max); a broader search asks the user to refine it.
pub const MAX_ROWS: i64 = 500;
/// BR-PARTY-02: each typed word is cut to this many letters unless full-word search is on (NumberOfDigitsPerWord).
pub const DIGITS_PER_WORD: usize = 12;

/// Person or organization filter (BR-PARTY-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    All,
    Person,
    Organization,
}

impl Kind {
    pub fn parse(s: &str) -> Self {
        match s {
            "person" => Kind::Person,
            "org" => Kind::Organization,
            _ => Kind::All,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Kind::All => "all",
            Kind::Person => "person",
            Kind::Organization => "org",
        }
    }
}

/// A current role filter on party_role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoleFilter {
    #[default]
    Any,
    Customer,
    Lead,
    Employee,
    Vendor,
}

impl RoleFilter {
    pub fn parse(s: &str) -> Self {
        match s {
            "customer" => RoleFilter::Customer,
            "lead" => RoleFilter::Lead,
            "employee" => RoleFilter::Employee,
            "vendor" => RoleFilter::Vendor,
            _ => RoleFilter::Any,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            RoleFilter::Any => "any",
            RoleFilter::Customer => "customer",
            RoleFilter::Lead => "lead",
            RoleFilter::Employee => "employee",
            RoleFilter::Vendor => "vendor",
        }
    }
    fn column(self) -> Option<&'static str> {
        match self {
            RoleFilter::Any => None,
            RoleFilter::Customer => Some("customer_ind"),
            RoleFilter::Lead => Some("lead_ind"),
            RoleFilter::Employee => Some("employee_ind"),
            RoleFilter::Vendor => Some("vendor_ind"),
        }
    }
}

/// What `party_search` looks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub kind: Kind,
    pub role: RoleFilter,
    /// BR-PARTY-02 FullWordSearch: match whole words instead of the first `digits_per_word` letters.
    pub full_word: bool,
    /// 0 means [`DIGITS_PER_WORD`].
    pub digits_per_word: usize,
    /// Archived parties are hidden unless this is set (BR-PARTY-01).
    pub include_deleted: bool,
    /// 1-based; clamped to the pages that exist.
    pub page: i64,
    /// 0 means [`PAGE_SIZE`].
    pub page_size: i64,
}

/// One party row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyRow {
    pub id: String,
    pub name: String,
    pub organization: bool,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub customer: bool,
    pub lead: bool,
    pub employee: bool,
    pub vendor: bool,
    pub archived: bool,
}

/// A page of search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    pub rows: Vec<PartyRow>,
    /// Every party that matches (may be more than [`MAX_ROWS`]).
    pub matched: i64,
    /// True when `matched` is over [`MAX_ROWS`] and only the first rows can be paged.
    pub capped: bool,
    pub page: i64,
    pub pages: i64,
    pub page_size: i64,
}

/// The search words of BR-PARTY-02: lowercase, split on white space, each cut to `digits` letters unless full_word.
pub fn search_words(query: &str, full_word: bool, digits: usize) -> Vec<String> {
    let digits = if digits == 0 { DIGITS_PER_WORD } else { digits };
    query
        .split_whitespace()
        .map(|w| {
            let w = w.to_lowercase();
            if full_word {
                w
            } else {
                w.chars().take(digits).collect()
            }
        })
        .collect()
}

fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The WHERE clause and its parameters for a search (no paging).
fn search_filter(s: &Search) -> (String, Vec<String>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    if !s.include_deleted {
        clauses.push("COALESCE(p.deleted_ind, 0) = 0".into());
    }
    match s.kind {
        Kind::All => {}
        Kind::Person => clauses.push("COALESCE(p.person_ind, 0) = 1".into()),
        Kind::Organization => clauses.push("COALESCE(p.organization_ind, 0) = 1".into()),
    }
    if let Some(col) = s.role.column() {
        // `col` is one of four fixed column names.
        clauses.push(format!(
            "EXISTS (SELECT 1 FROM party_role r WHERE r.party_id = p.id AND COALESCE(r.{col}, 0) = 1 \
             AND COALESCE(r.current_ind, 0) = 1)"
        ));
    }
    // Every word must start a word of the name (prefix), or be a whole word of it (full-word search). Words of the
    // name are separated by white space; the name is padded so the first and last words match too.
    for word in search_words(&s.query, s.full_word, s.digits_per_word) {
        values.push(like_escape(&word));
        let n = values.len();
        if s.full_word {
            clauses.push(format!(
                "(' ' || lower(COALESCE(p.name_cached, '')) || ' ') LIKE '% ' || ?{n} || ' %' ESCAPE '\\'"
            ));
        } else {
            clauses.push(format!("(' ' || lower(COALESCE(p.name_cached, ''))) LIKE '% ' || ?{n} || '%' ESCAPE '\\'"));
        }
    }
    let filter = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };
    (filter, values)
}

/// OP-PARTY-05 `party_search` (BR-PARTY-02): parties whose name matches every typed word, by kind and current role,
/// archived ones hidden (BR-PARTY-01), sorted by name, one page of `page_size` rows within the first [`MAX_ROWS`].
pub fn party_search(conn: &Connection, s: &Search) -> Result<SearchPage, CommandError> {
    let page_size = if s.page_size <= 0 { PAGE_SIZE } else { s.page_size };
    let (filter, values) = search_filter(s);
    let matched: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM party p {filter}"), params_from_iter(values.iter()), |r| {
            r.get(0)
        })?;
    let reachable = matched.min(MAX_ROWS);
    let pages = ((reachable + page_size - 1) / page_size).max(1);
    let page = s.page.clamp(1, pages);
    let offset = (page - 1) * page_size;
    let limit = page_size.min(reachable - offset).max(0);
    let role = |col: &str| {
        format!(
            "EXISTS (SELECT 1 FROM party_role r WHERE r.party_id = p.id AND COALESCE(r.{col}, 0) = 1 \
             AND COALESCE(r.current_ind, 0) = 1)"
        )
    };
    let sql = format!(
        "SELECT p.id, COALESCE(p.name_cached, ''), COALESCE(p.organization_ind, 0) = 1,
                COALESCE(p.email_address_cached, ''), COALESCE(p.phone_number_cached, ''),
                COALESCE(p.address_display_cached, ''), {}, {}, {}, {}, COALESCE(p.deleted_ind, 0) = 1
         FROM party p {filter}
         ORDER BY p.name_cached COLLATE NOCASE, p.id LIMIT {limit} OFFSET {offset}",
        role("customer_ind"),
        role("lead_ind"),
        role("employee_ind"),
        role("vendor_ind"),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), |r| {
            Ok(PartyRow {
                id: r.get(0)?,
                name: r.get(1)?,
                organization: r.get(2)?,
                email: r.get(3)?,
                phone: r.get(4)?,
                address: r.get(5)?,
                customer: r.get(6)?,
                lead: r.get(7)?,
                employee: r.get(8)?,
                vendor: r.get(9)?,
                archived: r.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SearchPage { rows, matched, capped: matched > MAX_ROWS, page, pages, page_size })
}

// ---------------------------------------------------------------------------------------------------------------
// party_get (05 "entity get commands"): the party and the children UI-04 shows
// ---------------------------------------------------------------------------------------------------------------

/// The person fields of a party (person row, same id as the party; D-15).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonFields {
    pub prefix: String,
    pub first_name: String,
    pub middle_name: String,
    pub last_name: String,
    pub suffix: String,
    pub nickname: String,
    pub job_title: String,
    pub department: String,
    pub employer_name: String,
}

/// The organization fields of a party.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrganizationFields {
    pub organization_name: String,
    pub company_type: String,
    pub federal_tax_id_number: String,
}

/// The four role kinds of party_role (one flag each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleKind {
    Customer,
    Lead,
    Employee,
    Vendor,
}

impl RoleKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "customer" => Some(RoleKind::Customer),
            "lead" => Some(RoleKind::Lead),
            "employee" => Some(RoleKind::Employee),
            "vendor" => Some(RoleKind::Vendor),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            RoleKind::Customer => "customer",
            RoleKind::Lead => "lead",
            RoleKind::Employee => "employee",
            RoleKind::Vendor => "vendor",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RoleKind::Customer => "Customer",
            RoleKind::Lead => "Lead",
            RoleKind::Employee => "Employee",
            RoleKind::Vendor => "Vendor",
        }
    }
    fn column(self) -> &'static str {
        match self {
            RoleKind::Customer => "customer_ind",
            RoleKind::Lead => "lead_ind",
            RoleKind::Employee => "employee_ind",
            RoleKind::Vendor => "vendor_ind",
        }
    }
    /// The subtype table of the role (D-15).
    fn table(self) -> &'static str {
        match self {
            RoleKind::Customer => "customer",
            RoleKind::Lead => "lead",
            RoleKind::Employee => "employee",
            RoleKind::Vendor => "vendor",
        }
    }
    /// party_role_type_id: 2 customer and 4 lead are the ids the source hard-codes (BR-PARTY-05); employee and
    /// vendor have none in the source, so they stay empty with only their flag set (CHECK-03 default).
    fn type_id(self) -> Option<i64> {
        match self {
            RoleKind::Customer => Some(crate::reference::PartyRoleType::Customer.id()),
            RoleKind::Lead => Some(crate::reference::PartyRoleType::Lead.id()),
            RoleKind::Employee | RoleKind::Vendor => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub id: String,
    pub kind: RoleKind,
    pub current: bool,
    pub from_date: String,
    pub thru_date: String,
}

/// One relation as the party sees it: the other party and what the other party is to this one (the row from the
/// other party to this one, BR-PARTY-03), with that row's billing and shipping flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    /// The row other -> this party.
    pub id: String,
    pub other_id: String,
    pub other_name: String,
    pub other_organization: bool,
    pub other_archived: bool,
    /// What the other party is to this one ("Office manager"), from the other -> this row.
    pub relationship: String,
    /// What this party is to the other one ("Employer"), from the reciprocal row; empty when it is missing.
    pub reciprocal: String,
    pub current: bool,
    pub from_date: String,
    pub thru_date: String,
    pub billing: bool,
    pub shipping: bool,
    /// The other party's primary email and phone (party caches), for the Contacts tab.
    pub email: String,
    pub phone: String,
}

/// The customer figures of the header tiles (UI-04): posted balance and retainer balance, in cents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerFigures {
    pub role_id: String,
    pub balance: i64,
    pub retainer: i64,
    pub taxable: bool,
    pub terms: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyDetail {
    pub id: String,
    pub name: String,
    pub organization: bool,
    pub person: Option<PersonFields>,
    pub org: Option<OrganizationFields>,
    pub comments: String,
    pub archived: bool,
    pub deleted_reason: String,
    pub deleted_timestamp: String,
    /// creation_timestamp, `YYYY-MM-DD HH:MM:SS` (D-05), or empty.
    pub created: String,
    pub methods: Vec<Method>,
    pub roles: Vec<Role>,
    pub relations: Vec<Relation>,
    pub customer: Option<CustomerFigures>,
}

impl PartyDetail {
    pub fn has_role(&self, kind: RoleKind) -> bool {
        self.roles.iter().any(|r| r.kind == kind && r.current)
    }
}

/// The party `id` with its person or organization row, contact methods, roles, relations and customer figures.
/// An archived party is returned too (BR-PARTY-01: its records stay); the page shows it as archived.
pub fn party_get(conn: &Connection, id: &str) -> Result<PartyDetail, CommandError> {
    let head = conn
        .query_row(
            "SELECT p.id, COALESCE(p.name_cached, ''), COALESCE(p.organization_ind, 0) = 1, COALESCE(p.comments, ''),
                    COALESCE(p.deleted_ind, 0) = 1, COALESCE(p.deleted_reason, ''),
                    COALESCE(p.deleted_timestamp, ''), COALESCE(p.creation_timestamp, '')
             FROM party p WHERE p.id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let (id, name, organization, comments, archived, deleted_reason, deleted_timestamp, created) = head;
    let person = conn
        .query_row(
            "SELECT COALESCE(prefix, ''), COALESCE(first_name, ''), COALESCE(middle_name, ''), COALESCE(last_name, ''),
                    COALESCE(suffix, ''), COALESCE(nickname, ''), COALESCE(job_title, ''), COALESCE(department, ''),
                    COALESCE(employer_name, '')
             FROM person WHERE id = ?1",
            [&id],
            |r| {
                Ok(PersonFields {
                    prefix: r.get(0)?,
                    first_name: r.get(1)?,
                    middle_name: r.get(2)?,
                    last_name: r.get(3)?,
                    suffix: r.get(4)?,
                    nickname: r.get(5)?,
                    job_title: r.get(6)?,
                    department: r.get(7)?,
                    employer_name: r.get(8)?,
                })
            },
        )
        .optional()?;
    let org = conn
        .query_row(
            "SELECT COALESCE(organization_name, ''), COALESCE(company_type, ''), COALESCE(federal_tax_id_number, '')
             FROM organization WHERE id = ?1",
            [&id],
            |r| {
                Ok(OrganizationFields {
                    organization_name: r.get(0)?,
                    company_type: r.get(1)?,
                    federal_tax_id_number: r.get(2)?,
                })
            },
        )
        .optional()?;
    let methods = contacts::methods_of(conn, &id)?;
    let roles = roles_of(conn, &id)?;
    let relations = relations_of(conn, &id)?;
    let customer = conn
        .query_row(
            "SELECT r.id,
                    COALESCE((SELECT SUM(st.balance) FROM sales_transaction st WHERE st.id_customer = r.id
                              AND COALESCE(st.locked_ind, 0) = 1 AND COALESCE(st.deleted, 0) = 0
                              AND COALESCE(st.invoice_ind, 0) = 1), 0),
                    COALESCE(c.retainer_balance_cached, 0), COALESCE(c.taxable, 0) = 1, COALESCE(c.sales_term_ref, '')
             FROM party_role r JOIN customer c ON c.id = r.id
             WHERE r.party_id = ?1 AND COALESCE(r.customer_ind, 0) = 1
             ORDER BY COALESCE(r.current_ind, 0) DESC, r.from_date DESC, r.id LIMIT 1",
            [&id],
            |r| {
                Ok(CustomerFigures {
                    role_id: r.get(0)?,
                    balance: r.get(1)?,
                    retainer: r.get(2)?,
                    taxable: r.get(3)?,
                    terms: r.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(PartyDetail {
        id,
        name,
        organization,
        person,
        org,
        comments,
        archived,
        deleted_reason,
        deleted_timestamp,
        created,
        methods,
        roles,
        relations,
        customer,
    })
}

/// The roles of a party, current first. A role row with several flags set gives one entry per flag.
pub fn roles_of(conn: &Connection, party_id: &str) -> rusqlite::Result<Vec<Role>> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(customer_ind, 0) = 1, COALESCE(lead_ind, 0) = 1, COALESCE(employee_ind, 0) = 1,
                COALESCE(vendor_ind, 0) = 1, COALESCE(current_ind, 0) = 1, COALESCE(from_date, ''),
                COALESCE(thru_date, '')
         FROM party_role WHERE party_id = ?1 ORDER BY COALESCE(current_ind, 0) DESC, from_date, id",
    )?;
    let mut roles = Vec::new();
    let rows = stmt.query_map([party_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            [r.get::<_, bool>(1)?, r.get::<_, bool>(2)?, r.get::<_, bool>(3)?, r.get::<_, bool>(4)?],
            r.get::<_, bool>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    for row in rows {
        let (id, flags, current, from_date, thru_date) = row?;
        let kinds = [RoleKind::Customer, RoleKind::Lead, RoleKind::Employee, RoleKind::Vendor];
        for (kind, on) in kinds.into_iter().zip(flags) {
            if on {
                roles.push(Role {
                    id: id.clone(),
                    kind,
                    current,
                    from_date: from_date.clone(),
                    thru_date: thru_date.clone(),
                });
            }
        }
    }
    Ok(roles)
}

/// The relations of a party (the rows other -> this party), current first, then by the other party's name.
pub fn relations_of(conn: &Connection, party_id: &str) -> rusqlite::Result<Vec<Relation>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, o.id, COALESCE(o.name_cached, ''), COALESCE(o.organization_ind, 0) = 1,
                COALESCE(o.deleted_ind, 0) = 1, COALESCE(r.relationship, ''),
                COALESCE((SELECT b.relationship FROM party_relation b
                          WHERE b.party_id_from = r.party_id_to AND b.party_id_to = r.party_id_from
                          ORDER BY b.id LIMIT 1), ''),
                COALESCE(r.current_ind, 0) = 1, COALESCE(r.from_date, ''), COALESCE(r.thru_date, ''),
                COALESCE(r.billing_ind, 0) = 1, COALESCE(r.shipping_ind, 0) = 1,
                COALESCE(o.email_address_cached, ''), COALESCE(o.phone_number_cached, '')
         FROM party_relation r JOIN party o ON o.id = r.party_id_from
         WHERE r.party_id_to = ?1
         ORDER BY COALESCE(r.current_ind, 0) DESC, o.name_cached COLLATE NOCASE, r.id",
    )?;
    let rows = stmt
        .query_map([party_id], |r| {
            Ok(Relation {
                id: r.get(0)?,
                other_id: r.get(1)?,
                other_name: r.get(2)?,
                other_organization: r.get(3)?,
                other_archived: r.get(4)?,
                relationship: r.get(5)?,
                reciprocal: r.get(6)?,
                current: r.get(7)?,
                from_date: r.get(8)?,
                thru_date: r.get(9)?,
                billing: r.get(10)?,
                shipping: r.get(11)?,
                email: r.get(12)?,
                phone: r.get(13)?,
            })
        })?
        .collect();
    rows
}

// ---------------------------------------------------------------------------------------------------------------
// BR-PARTY-06 party caches
// ---------------------------------------------------------------------------------------------------------------

/// The party display caches, as (column, SQL expression over the party row aliased `t`). The commands that change
/// names or contact methods refresh them with [`refresh_caches`]; the import recomputes them for every party with
/// the same expressions (import::recompute::party_caches), so both write the same formats.
///
/// name_cached is the organization name or "First Last"; the phone, email and address caches point at the primary
/// of their kind, or the first one added when none is primary.
pub fn cache_columns() -> Vec<(&'static str, String)> {
    let name = "COALESCE(CASE WHEN COALESCE(t.organization_ind, 0) = 1 \
                THEN (SELECT NULLIF(trim(o.organization_name), '') FROM organization o WHERE o.id = t.id) \
                ELSE (SELECT NULLIF(trim(COALESCE(pe.first_name, '') || ' ' || COALESCE(pe.last_name, '')), '') \
                      FROM person pe WHERE pe.id = t.id) END, t.name_cached)"
        .to_owned();
    let first = |join: &str, filter: &str, col: &str| {
        format!(
            "(SELECT {col} FROM contact_method cm JOIN {join} WHERE cm.party_id = t.id {filter} \
             ORDER BY COALESCE(s.primary_ind, 0) DESC, cm.creation_timestamp, cm.id LIMIT 1)"
        )
    };
    let phone = |col: &str| first("phone s ON s.id = cm.id", "", col);
    let email = |col: &str| first("network_address s ON s.id = cm.id", "AND COALESCE(s.email_address_ind, 0) = 1", col);
    let address = |col: &str| first("address a ON a.id = cm.id JOIN address s ON s.id = a.id", "", col);
    vec![
        ("name_cached", name),
        ("phone_id_cached", phone("cm.id")),
        ("phone_number_cached", phone("s.number")),
        ("phone_label_cached", phone("cm.label")),
        ("email_address_id_cached", email("cm.id")),
        ("email_address_cached", email("s.address")),
        ("email_address_label_cached", email("cm.label")),
        ("address_id_cached", address("cm.id")),
        ("address_display_cached", address(contacts::address_display_sql())),
        ("address_label_cached", address("cm.label")),
    ]
}

/// BR-PARTY-06: recomputes the display caches of one party (BR-CONTACT-05: a deleted method's caches clear).
pub fn refresh_caches(conn: &Connection, party_id: &str) -> rusqlite::Result<()> {
    let sets: Vec<String> = cache_columns().into_iter().map(|(col, expr)| format!("{col} = ({expr})")).collect();
    conn.execute(&format!("UPDATE party AS t SET {} WHERE t.id = ?1", sets.join(", ")), [party_id])?;
    Ok(())
}

/// BR-PARTY-08 CleanText: leading and trailing white space and control characters removed (line breaks and tabs
/// inside the text stay; Windows line ends become `\n`).
pub fn clean_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_owned()
}

fn opt(s: &str) -> Option<String> {
    let t = clean_text(s);
    (!t.is_empty()).then_some(t)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-PARTY-01 party_create_person, OP-PARTY-02 party_create_organization, OP-PARTY-03 party_update
// ---------------------------------------------------------------------------------------------------------------

/// What a create or edit form gives for the person or organization row, plus the party comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyFields {
    Person(PersonFields),
    Organization(OrganizationFields),
}

impl PartyFields {
    fn check(&self) -> Vec<FieldError> {
        let mut errors = Vec::new();
        match self {
            PartyFields::Person(p) if p.first_name.trim().is_empty() && p.last_name.trim().is_empty() => {
                errors.push(FieldError { field: "first_name", message: "Enter a first or last name.".into() })
            }
            PartyFields::Organization(o) if o.organization_name.trim().is_empty() => {
                errors.push(FieldError { field: "organization_name", message: "Enter the organization's name.".into() })
            }
            _ => {}
        }
        errors
    }
    fn organization(&self) -> bool {
        matches!(self, PartyFields::Organization(_))
    }
}

/// A create (OP-PARTY-01/02): the server-issued id of the form (BR-PARTY-10), the fields, an optional first email
/// and phone (saved through the contact-method rules, BR-CONTACT-03/04) and an optional first role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewParty {
    pub id: String,
    pub fields: PartyFields,
    pub comments: String,
    pub email: String,
    pub phone: String,
    pub role: Option<RoleKind>,
}

/// A form id: what `ids::new_id` gives (36 characters of hex digits and dashes). The import keeps other ids.
fn valid_form_id(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// OP-PARTY-01 / OP-PARTY-02 (BR-PARTY-10): creates the party and its person or organization row with one id, the
/// optional first email and phone and the optional role, in one transaction. An id that exists already is refused
/// with AlreadyExists (a resubmitted form; the route sends the page to that party).
pub fn party_create(conn: &mut Connection, new: &NewParty, actor: &Actor) -> Result<String, CommandError> {
    let errors = check_new(new);
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    create_in(&tx, new, &Stamp::now(actor))?;
    tx.commit()?;
    Ok(new.id.clone())
}

/// The field errors of a create, before any write.
fn check_new(new: &NewParty) -> Vec<FieldError> {
    let mut errors = new.fields.check();
    if !valid_form_id(&new.id) {
        errors.push(FieldError { field: "id", message: "The form is out of date. Close it and try again.".into() });
    }
    if !new.email.trim().is_empty() {
        errors.extend(MethodInput::email(&new.email).check("email_"));
    }
    if !new.phone.trim().is_empty() {
        errors.extend(MethodInput::phone(&new.phone).check("phone_"));
    }
    errors
}

/// [`party_create`] inside the caller's transaction (also the wizard's).
fn create_in(tx: &Connection, new: &NewParty, stamp: &Stamp) -> Result<(), CommandError> {
    let exists: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM party WHERE id = ?1)", [&new.id], |r| r.get(0))?;
    if exists {
        return Err(CommandError::AlreadyExists(new.id.clone()));
    }
    let org = new.fields.organization();
    tx.execute(
        "INSERT INTO party (id, organization_ind, person_ind, deleted_ind, merge_ind, comments) VALUES (?1, ?2, ?3, 0, 0, ?4)",
        params![new.id, i64::from(org), i64::from(!org), opt(&new.comments)],
    )?;
    audit::stamp_created(tx, "party", &new.id, stamp)?;
    match &new.fields {
        PartyFields::Person(_) => tx.execute("INSERT INTO person (id) VALUES (?1)", [&new.id])?,
        PartyFields::Organization(_) => tx.execute("INSERT INTO organization (id) VALUES (?1)", [&new.id])?,
    };
    write_fields(tx, &new.id, &new.fields)?;
    audit::stamp_created(tx, if org { "organization" } else { "person" }, &new.id, stamp)?;
    if !new.email.trim().is_empty() {
        contacts::insert_method(tx, &new.id, &MethodInput::email(&new.email), stamp)?;
    }
    if !new.phone.trim().is_empty() {
        contacts::insert_method(tx, &new.id, &MethodInput::phone(&new.phone), stamp)?;
    }
    refresh_caches(tx, &new.id)?;
    if let Some(role) = new.role {
        // A customer made with its party is taxable, as Create Customer V3 does (kontroller 229 step 154).
        add_role(tx, &new.id, role, true, stamp)?;
    }
    Ok(())
}

fn write_fields(conn: &Connection, id: &str, fields: &PartyFields) -> rusqlite::Result<()> {
    match fields {
        PartyFields::Person(p) => conn.execute(
            "UPDATE person SET prefix = ?2, first_name = ?3, middle_name = ?4, last_name = ?5, suffix = ?6,
                               nickname = ?7, job_title = ?8, department = ?9, employer_name = ?10 WHERE id = ?1",
            params![
                id,
                opt(&p.prefix),
                opt(&p.first_name),
                opt(&p.middle_name),
                opt(&p.last_name),
                opt(&p.suffix),
                opt(&p.nickname),
                opt(&p.job_title),
                opt(&p.department),
                opt(&p.employer_name),
            ],
        )?,
        PartyFields::Organization(o) => conn.execute(
            "UPDATE organization SET organization_name = ?2, company_type = ?3, federal_tax_id_number = ?4 WHERE id = ?1",
            params![id, opt(&o.organization_name), opt(&o.company_type), opt(&o.federal_tax_id_number)],
        )?,
    };
    Ok(())
}

/// OP-PARTY-03 `party_update` (BR-PARTY-06, BR-PARTY-08): the person or organization fields and the comments
/// (cleaned), then the name cache. The fields must be of the party's kind. An archived party is not edited.
pub fn party_update(
    conn: &mut Connection,
    id: &str,
    fields: &PartyFields,
    comments: &str,
    actor: &Actor,
) -> Result<(), CommandError> {
    let errors = fields.check();
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (org, archived) = party_state(&tx, id)?;
    if archived {
        return Err(CommandError::Refused("An archived party cannot be edited.".into()));
    }
    if org != fields.organization() {
        return Err(CommandError::Validation(vec![FieldError {
            field: "kind",
            message: "These fields are for the other kind of party.".into(),
        }]));
    }
    // A party imported without its person or organization row gets one now.
    let table = if org { "organization" } else { "person" };
    let missing = tx.execute(&format!("INSERT OR IGNORE INTO {table} (id) VALUES (?1)"), [id])?;
    write_fields(&tx, id, fields)?;
    tx.execute("UPDATE party SET comments = ?2 WHERE id = ?1", params![id, opt(comments)])?;
    let stamp = Stamp::now(actor);
    if missing == 1 {
        audit::stamp_created(&tx, table, &id, &stamp)?;
    } else {
        audit::stamp_modified(&tx, table, &id, &stamp)?;
    }
    refresh_caches(&tx, id)?;
    audit::stamp_modified(&tx, "party", &id, &stamp)?;
    tx.commit()?;
    Ok(())
}

/// (organization, archived) of a party, or NotFound.
fn party_state(conn: &Connection, id: &str) -> Result<(bool, bool), CommandError> {
    conn.query_row(
        "SELECT COALESCE(organization_ind, 0) = 1, COALESCE(deleted_ind, 0) = 1 FROM party WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .ok_or(CommandError::NotFound)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-PARTY-04 party_archive
// ---------------------------------------------------------------------------------------------------------------

/// OP-PARTY-04 `party_archive` (BR-PARTY-01): no hard delete. Sets deleted_ind, the reason (cleaned), the time and
/// the account name; nothing else changes, so the party's sales, activities and contact methods stay. Archiving an
/// archived party changes nothing.
pub fn party_archive(conn: &mut Connection, id: &str, reason: &str, actor: &Actor) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (_, archived) = party_state(&tx, id)?;
    if archived {
        return Ok(());
    }
    let stamp = Stamp::now(actor);
    tx.execute(
        "UPDATE party SET deleted_ind = 1, deleted_reason = ?2, deleted_timestamp = ?3, deleted_account_name = ?4
         WHERE id = ?1",
        params![id, opt(reason), stamp.at, actor.account_name],
    )?;
    audit::stamp_modified(&tx, "party", &id, &stamp)?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-PARTY-10 party_role_add
// ---------------------------------------------------------------------------------------------------------------

/// OP-PARTY-10 `party_role_add` (BR-PARTY-05): idempotent. When the party holds the role already, its id is
/// returned (a former role is made current again). A new role is current from today, with type 2 for a customer
/// and 4 for a lead, and its subtype row (customer, lead, employee, vendor). The party must exist and not be
/// archived.
pub fn party_role_add(
    conn: &mut Connection,
    party_id: &str,
    role: RoleKind,
    actor: &Actor,
) -> Result<String, CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (_, archived) = party_state(&tx, party_id)?;
    if archived {
        return Err(CommandError::Refused("An archived party cannot get a new role.".into()));
    }
    // A customer role added to an existing party starts not taxable: Add Customer to Party (kontroller 705 step 180)
    // writes the Taxable it is given, and its callers give none.
    let id = add_role(&tx, party_id, role, false, &Stamp::now(actor))?;
    tx.commit()?;
    Ok(id)
}

/// [`party_role_add`] inside the caller's transaction. `taxable`: the Taxable of a new customer row (an existing
/// customer row is kept as it is).
pub(crate) fn add_role(
    conn: &Connection,
    party_id: &str,
    role: RoleKind,
    taxable: bool,
    stamp: &Stamp,
) -> Result<String, CommandError> {
    let col = role.column();
    let existing: Option<(String, bool)> = conn
        .query_row(
            &format!(
                "SELECT id, COALESCE(current_ind, 0) = 1 FROM party_role WHERE party_id = ?1 AND COALESCE({col}, 0) = 1
                 ORDER BY COALESCE(current_ind, 0) DESC, from_date DESC, id LIMIT 1"
            ),
            [party_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let today = &stamp.at[..10];
    let name: String =
        conn.query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [party_id], |r| r.get(0))?;
    let id = match existing {
        Some((id, true)) => id,
        Some((id, false)) => {
            conn.execute("UPDATE party_role SET current_ind = 1, thru_date = NULL WHERE id = ?1", [&id])?;
            audit::stamp_modified(conn, "party_role", &id, stamp)?;
            id
        }
        None => {
            let id = crate::ids::new_id();
            conn.execute(
                "INSERT INTO party_role (id, party_id, party_role_type_id, customer_ind, lead_ind, employee_ind,
                                         vendor_ind, current_ind, from_date)
                 VALUES (?1, ?2, ?3, 0, 0, 0, 0, 1, ?4)",
                params![id, party_id, role.type_id(), today],
            )?;
            conn.execute(&format!("UPDATE party_role SET {col} = 1 WHERE id = ?1"), [&id])?;
            audit::stamp_created(conn, "party_role", &id, stamp)?;
            id
        }
    };
    // The subtype row (D-15); sales transactions point at the customer row (BR-PARTY-05 check).
    let table = role.table();
    let inserted = match role {
        RoleKind::Customer => conn.execute(
            "INSERT OR IGNORE INTO customer (id, display_name, balance, retainer_balance_cached, retainer_adjustment, taxable)
             VALUES (?1, ?2, 0, 0, 0, ?3)",
            params![id, name, i64::from(taxable)],
        )?,
        RoleKind::Lead | RoleKind::Employee | RoleKind::Vendor => {
            conn.execute(&format!("INSERT OR IGNORE INTO {table} (id, name_cached) VALUES (?1, ?2)"), params![id, name])?
        }
    };
    if inserted == 1 {
        audit::stamp_created(conn, table, &id, stamp)?;
    }
    Ok(id)
}

// ---------------------------------------------------------------------------------------------------------------
// OP-PARTY-07..09 relations (BR-PARTY-03, BR-PARTY-04)
// ---------------------------------------------------------------------------------------------------------------

/// A new relation, seen from `party_id` (the page it is added on): `other_id` is `relationship` of the party ("Office
/// manager"), and the party is `reciprocal` of the other one ("Employer"). Billing and shipping say the other party
/// is the party's billing or shipping contact; they are on the other -> party row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationInput {
    pub party_id: String,
    pub other_id: String,
    pub relationship: String,
    pub reciprocal: String,
    pub billing: bool,
    pub shipping: bool,
}

/// The two rows of a relation pair: (other -> party, party -> other) as ids.
fn pair_of(conn: &Connection, id: &str) -> Result<(String, String, String, String), CommandError> {
    let (from, to): (String, String) = conn
        .query_row("SELECT party_id_from, party_id_to FROM party_relation WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .ok_or(CommandError::NotFound)?;
    let back: Option<String> = conn
        .query_row(
            "SELECT id FROM party_relation WHERE party_id_from = ?1 AND party_id_to = ?2 AND id <> ?3 ORDER BY id LIMIT 1",
            params![to, from, id],
            |r| r.get(0),
        )
        .optional()?;
    let back = back.ok_or_else(|| {
        CommandError::Refused("This relation has no reciprocal row, so it cannot be changed here (BR-PARTY-03).".into())
    })?;
    Ok((id.to_owned(), back, from, to))
}

/// The relations of an archived party stay as they are (BR-PARTY-01), as its contact methods do.
fn refuse_archived(conn: &Connection, a: &str, b: &str) -> Result<(), CommandError> {
    for id in [a, b] {
        if party_state(conn, id)?.1 {
            return Err(CommandError::Refused("The relations of an archived party cannot be changed.".into()));
        }
    }
    Ok(())
}

/// OP-PARTY-07 `party_relation_create` (BR-PARTY-03): both rows, current from today. A pair that exists already
/// (either way) is refused, as is a relation of a party to itself or with an archived party. Returns the id of the
/// other -> party row (the one the party's Contacts tab shows).
pub fn party_relation_create(
    conn: &mut Connection,
    input: &RelationInput,
    actor: &Actor,
) -> Result<String, CommandError> {
    if input.other_id.trim().is_empty() {
        return Err(CommandError::Validation(vec![FieldError {
            field: "other_id",
            message: "Choose the related person or organization.".into(),
        }]));
    }
    if input.other_id == input.party_id {
        return Err(CommandError::Validation(vec![FieldError {
            field: "other_id",
            message: "A party cannot be related to itself.".into(),
        }]));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    refuse_archived(&tx, &input.party_id, &input.other_id)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM party_relation WHERE (party_id_from = ?1 AND party_id_to = ?2)
                                                       OR (party_id_from = ?2 AND party_id_to = ?1))",
        params![input.other_id, input.party_id],
        |r| r.get(0),
    )?;
    if exists {
        return Err(CommandError::Refused("These two are related already. Edit that relation instead.".into()));
    }
    let stamp = Stamp::now(actor);
    let today = &stamp.at[..10];
    let forward = crate::ids::new_id();
    let back = crate::ids::new_id();
    for (id, from, to, what, billing, shipping) in [
        (&forward, &input.other_id, &input.party_id, &input.relationship, input.billing, input.shipping),
        (&back, &input.party_id, &input.other_id, &input.reciprocal, false, false),
    ] {
        tx.execute(
            "INSERT INTO party_relation (id, party_id_from, party_id_to, relationship, from_date, current_ind,
                                         billing_ind, shipping_ind)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7)",
            params![id, from, to, opt(what), today, i64::from(billing), i64::from(shipping)],
        )?;
        audit::stamp_created(&tx, "party_relation", id, &stamp)?;
    }
    tx.commit()?;
    Ok(forward)
}

/// What OP-PARTY-08 does to a pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationChange {
    /// New texts and flags, each written to the row it belongs to (the source wrote unset variables; D-07).
    Edit { relationship: String, reciprocal: String, billing: bool, shipping: bool },
    /// BR-PARTY-04: current_ind = 0 and thru_date = today on both rows.
    End,
    /// BR-PARTY-04: current_ind = 1 and no thru_date on both rows.
    Restore,
}

/// OP-PARTY-08 `party_relation_update` on the pair of row `id` (either row of it). Edit takes `id` as the
/// other -> party row (the one the Contacts tab shows): `relationship`, billing and shipping go to it, `reciprocal` to
/// the row back. A pair without its reciprocal row is refused.
pub fn party_relation_update(
    conn: &mut Connection,
    id: &str,
    change: &RelationChange,
    actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (row, back, from, to) = pair_of(&tx, id)?;
    refuse_archived(&tx, &from, &to)?;
    let stamp = Stamp::now(actor);
    match change {
        RelationChange::Edit { relationship, reciprocal, billing, shipping } => {
            tx.execute(
                "UPDATE party_relation SET relationship = ?2, billing_ind = ?3, shipping_ind = ?4 WHERE id = ?1",
                params![row, opt(relationship), i64::from(*billing), i64::from(*shipping)],
            )?;
            tx.execute("UPDATE party_relation SET relationship = ?2 WHERE id = ?1", params![back, opt(reciprocal)])?;
        }
        RelationChange::End => {
            for r in [&row, &back] {
                tx.execute(
                    "UPDATE party_relation SET current_ind = 0, thru_date = ?2 WHERE id = ?1",
                    params![r, &stamp.at[..10]],
                )?;
            }
        }
        RelationChange::Restore => {
            for r in [&row, &back] {
                tx.execute("UPDATE party_relation SET current_ind = 1, thru_date = NULL WHERE id = ?1", [r])?;
            }
        }
    }
    for r in [&row, &back] {
        audit::stamp_modified(&tx, "party_relation", r, &stamp)?;
    }
    tx.commit()?;
    Ok(())
}

/// OP-PARTY-09 `party_relation_delete` (BR-PARTY-03, AP-07): without `confirm`, ConfirmationRequired with a summary;
/// with it, both rows go. A missing reciprocal row is an error.
pub fn party_relation_delete(
    conn: &mut Connection,
    id: &str,
    confirm: bool,
    _actor: &Actor,
) -> Result<(), CommandError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (row, back, from, to) = pair_of(&tx, id)?;
    refuse_archived(&tx, &from, &to)?;
    if !confirm {
        let name = |pid: &str| -> rusqlite::Result<String> {
            tx.query_row("SELECT COALESCE(name_cached, '') FROM party WHERE id = ?1", [pid], |r| r.get(0))
        };
        return Err(CommandError::ConfirmationRequired(format!(
            "Deletes the relation between {} and {} (both of its rows). Ending it instead keeps its history.",
            name(&from)?,
            name(&to)?
        )));
    }
    tx.execute("DELETE FROM party_relation WHERE id IN (?1, ?2)", params![row, back])?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// OP-PARTY-11 party_create_wizard (BR-PARTY-09)
// ---------------------------------------------------------------------------------------------------------------

/// What the create-party wizard (UI-05) gives: an organization and/or a person, each with its role, and the
/// person's relationship to the organization when both are new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardInput {
    pub organization: Option<NewParty>,
    pub person: Option<NewParty>,
    /// What the person is of the organization ("Office manager"); empty means "Employee".
    pub relationship: String,
    pub billing_contact: bool,
}

/// Field names of the wizard: the organization's fields start with org_, the person's with person_.
fn wizard_field(prefix: &str, field: &'static str) -> &'static str {
    match (prefix, field) {
        ("org_", "organization_name") => "org_name",
        ("org_", "email") => "org_email",
        ("org_", "phone") => "org_phone",
        ("org_", "id") | ("person_", "id") => "id",
        ("person_", "first_name") => "person_first_name",
        ("person_", "email") => "person_email",
        ("person_", "phone") => "person_phone",
        (_, f) => f,
    }
}

/// OP-PARTY-11 `party_create_wizard` (BR-PARTY-09), one transaction (D-08: a failure leaves nothing behind):
/// the organization with a customer or lead role or none; the person with an employee role, a lead role (only when
/// no organization is created) or none; then the relation pair between them (person -> organization is the
/// relationship, organization -> person is "Employer"). Returns (organization id, person id).
pub fn party_create_wizard(
    conn: &mut Connection,
    input: &WizardInput,
    actor: &Actor,
) -> Result<(Option<String>, Option<String>), CommandError> {
    let mut errors = Vec::new();
    if input.organization.is_none() && input.person.is_none() {
        errors.push(FieldError { field: "org_name", message: "Enter an organization, a person, or both.".into() });
    }
    if let Some(o) = &input.organization {
        errors.extend(check_new(o).into_iter().map(|e| FieldError { field: wizard_field("org_", e.field), ..e }));
        if !matches!(o.role, None | Some(RoleKind::Customer) | Some(RoleKind::Lead)) {
            errors.push(FieldError {
                field: "org_role",
                message: "An organization is a customer, a lead or neither.".into(),
            });
        }
    }
    if let Some(p) = &input.person {
        errors.extend(check_new(p).into_iter().map(|e| FieldError { field: wizard_field("person_", e.field), ..e }));
        match p.role {
            None | Some(RoleKind::Employee) => {}
            Some(RoleKind::Lead) if input.organization.is_none() => {}
            Some(RoleKind::Lead) => errors.push(FieldError {
                field: "person_role",
                message: "With a new organization, the lead is the organization.".into(),
            }),
            Some(_) => errors.push(FieldError {
                field: "person_role",
                message: "A person here is an employee, a lead or neither.".into(),
            }),
        }
    }
    if !errors.is_empty() {
        return Err(CommandError::Validation(errors));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stamp = Stamp::now(actor);
    if let Some(o) = &input.organization {
        create_in(&tx, o, &stamp)?;
    }
    if let Some(p) = &input.person {
        create_in(&tx, p, &stamp)?;
    }
    if let (Some(o), Some(p)) = (&input.organization, &input.person) {
        let today = &stamp.at[..10];
        let what =
            if input.relationship.trim().is_empty() { "Employee".to_owned() } else { clean_text(&input.relationship) };
        for (from, to, what, billing) in
            [(&p.id, &o.id, what.as_str(), input.billing_contact), (&o.id, &p.id, "Employer", false)]
        {
            let id = crate::ids::new_id();
            tx.execute(
                "INSERT INTO party_relation (id, party_id_from, party_id_to, relationship, from_date, current_ind,
                                             billing_ind, shipping_ind)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, 0)",
                params![id, from, to, what, today, i64::from(billing)],
            )?;
            audit::stamp_created(&tx, "party_relation", &id, &stamp)?;
        }
    }
    tx.commit()?;
    Ok((input.organization.as_ref().map(|o| o.id.clone()), input.person.as_ref().map(|p| p.id.clone())))
}

// ---------------------------------------------------------------------------------------------------------------
// OP-PARTY-06 party_find_by_contact (BR-PARTY-11)
// ---------------------------------------------------------------------------------------------------------------

/// A party found by its email or phone, for the pickers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub organization: bool,
    /// The email or phone that matched (the owner's), or the person's when this is the employer.
    pub matched: String,
    /// True when the party is here as the employer of the owner (BR-PARTY-11).
    pub via_relation: bool,
}

/// OP-PARTY-06 `party_find_by_contact` (BR-PARTY-11): the parties that own one of `emails` (case-insensitive) or
/// `phones` (same digits, the last 10 when longer), archived ones left out. With `prefer_org`, the organization of a
/// current relation of a matching person comes first (the parent party), then the owners.
pub fn party_find_by_contact(
    conn: &Connection,
    emails: &[String],
    phones: &[String],
    prefer_org: bool,
) -> Result<Vec<Found>, CommandError> {
    let mut owners: Vec<Found> = Vec::new();
    let push = |f: Found, list: &mut Vec<Found>| {
        if !list.iter().any(|x| x.id == f.id) {
            list.push(f);
        }
    };
    let mut stmt = conn.prepare(
        "SELECT p.id, COALESCE(p.name_cached, ''), COALESCE(p.organization_ind, 0) = 1, n.address
         FROM network_address n JOIN contact_method cm ON cm.id = n.id JOIN party p ON p.id = cm.party_id
         WHERE COALESCE(n.email_address_ind, 0) = 1 AND lower(trim(n.address)) = lower(trim(?1))
           AND COALESCE(p.deleted_ind, 0) = 0
         ORDER BY p.name_cached COLLATE NOCASE, p.id",
    )?;
    for e in emails.iter().filter(|e| !e.trim().is_empty()) {
        for row in stmt.query_map([e], |r| {
            Ok(Found {
                id: r.get(0)?,
                name: r.get(1)?,
                organization: r.get(2)?,
                matched: r.get(3)?,
                via_relation: false,
            })
        })? {
            push(row?, &mut owners);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT p.id, COALESCE(p.name_cached, ''), COALESCE(p.organization_ind, 0) = 1, ph.number
         FROM phone ph JOIN contact_method cm ON cm.id = ph.id JOIN party p ON p.id = cm.party_id
         WHERE ph.number_digits_only <> '' AND substr(ph.number_digits_only, -10) = ?1 AND COALESCE(p.deleted_ind, 0) = 0
         ORDER BY p.name_cached COLLATE NOCASE, p.id",
    )?;
    for phone in phones {
        let digits = contacts::digits_only(phone);
        if digits.len() < 7 {
            continue;
        }
        let key = if digits.len() > 10 { digits[digits.len() - 10..].to_owned() } else { digits };
        for row in stmt.query_map([key], |r| {
            Ok(Found {
                id: r.get(0)?,
                name: r.get(1)?,
                organization: r.get(2)?,
                matched: r.get(3)?,
                via_relation: false,
            })
        })? {
            push(row?, &mut owners);
        }
    }
    if !prefer_org {
        return Ok(owners);
    }
    let mut found = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT o.id, COALESCE(o.name_cached, '') FROM party_relation r JOIN party o ON o.id = r.party_id_to
         WHERE r.party_id_from = ?1 AND COALESCE(r.current_ind, 0) = 1 AND COALESCE(o.organization_ind, 0) = 1
           AND COALESCE(o.deleted_ind, 0) = 0
         ORDER BY o.name_cached COLLATE NOCASE, o.id",
    )?;
    for owner in owners.iter().filter(|o| !o.organization) {
        for row in stmt.query_map([&owner.id], |r| {
            Ok(Found {
                id: r.get(0)?,
                name: r.get(1)?,
                organization: true,
                matched: owner.matched.clone(),
                via_relation: true,
            })
        })? {
            push(row?, &mut found);
        }
    }
    for owner in owners {
        push(owner, &mut found);
    }
    Ok(found)
}

// ---------------------------------------------------------------------------------------------------------------
// Data checks (BR-PARTY-03, BR-PARTY-05)
// ---------------------------------------------------------------------------------------------------------------

/// The relation rows that have no reciprocal row (BR-PARTY-03: every relation is a From->To and To->From pair).
pub fn relations_without_reciprocal(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT a.id FROM party_relation a
         WHERE NOT EXISTS (SELECT 1 FROM party_relation b WHERE b.id <> a.id
                           AND b.party_id_from = a.party_id_to AND b.party_id_to = a.party_id_from)
         ORDER BY a.id",
    )?;
    let ids = stmt.query_map([], |r| r.get(0))?.collect();
    ids
}

/// The customer roles (customer_ind = 1) that have no `customer` subtype row; sales transactions point at that row.
pub fn customer_roles_without_customer_row(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT r.id FROM party_role r WHERE COALESCE(r.customer_ind, 0) = 1
         AND NOT EXISTS (SELECT 1 FROM customer c WHERE c.id = r.id) ORDER BY r.id",
    )?;
    let ids = stmt.query_map([], |r| r.get(0))?.collect();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_in_memory, Seed};

    fn demo() -> Connection {
        open_in_memory(Seed::Demo).unwrap().0
    }

    fn names(conn: &Connection, s: &Search) -> Vec<String> {
        party_search(conn, s).unwrap().rows.into_iter().map(|r| r.name).collect()
    }

    fn q(query: &str) -> Search {
        Search { query: query.into(), ..Default::default() }
    }

    /// The demo database with its user as the actor.
    fn demo_user() -> (Connection, Actor) {
        let conn = demo();
        let actor = crate::admin::current_user(&conn).unwrap().unwrap();
        (conn, actor)
    }

    fn person(first: &str, last: &str) -> PartyFields {
        PartyFields::Person(PersonFields { first_name: first.into(), last_name: last.into(), ..Default::default() })
    }

    fn new_party(fields: PartyFields) -> NewParty {
        NewParty {
            id: crate::ids::new_id(),
            fields,
            comments: String::new(),
            email: String::new(),
            phone: String::new(),
            role: None,
        }
    }

    fn one(conn: &Connection, sql: &str, id: &str) -> String {
        conn.query_row(sql, [id], |r| r.get::<_, Option<String>>(0)).unwrap().unwrap_or_default()
    }

    fn relation(party: &str, other: &str, what: &str) -> RelationInput {
        RelationInput {
            party_id: party.into(),
            other_id: other.into(),
            relationship: what.into(),
            reciprocal: "Client".into(),
            billing: true,
            shipping: false,
        }
    }

    fn pair_rows(conn: &Connection, a: &str, b: &str) -> Vec<(String, String, String, i64, i64, String)> {
        conn.prepare(
            "SELECT party_id_from, party_id_to, COALESCE(relationship, ''), COALESCE(billing_ind, 0),
                    COALESCE(current_ind, 0), COALESCE(thru_date, '')
             FROM party_relation WHERE (party_id_from = ?1 AND party_id_to = ?2) OR (party_id_from = ?2 AND party_id_to = ?1)
             ORDER BY party_id_from = ?1 DESC",
        )
        .unwrap()
        .query_map([a, b], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    #[test]
    fn a_relation_is_created_edited_and_deleted_as_a_pair() {
        let (mut conn, actor) = demo_user();
        let id = party_relation_create(&mut conn, &relation("p-bm", "p-tf", "Consultant"), &actor).unwrap();
        let rows = pair_rows(&conn, "p-tf", "p-bm");
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].2.as_str(), rows[0].3), ("Consultant", 1), "other -> party: relationship and billing");
        assert_eq!((rows[1].2.as_str(), rows[1].3), ("Client", 0), "party -> other: the reciprocal, no billing");
        // A duplicate pair, either way round, is refused.
        assert!(matches!(
            party_relation_create(&mut conn, &relation("p-bm", "p-tf", "Again"), &actor),
            Err(CommandError::Refused(_))
        ));
        assert!(matches!(
            party_relation_create(&mut conn, &relation("p-tf", "p-bm", "Again"), &actor),
            Err(CommandError::Refused(_))
        ));
        assert!(matches!(
            party_relation_create(&mut conn, &relation("p-tf", "p-tf", "Me"), &actor),
            Err(CommandError::Validation(_))
        ));
        // Edit writes each value to its own row.
        let edit = RelationChange::Edit {
            relationship: "Advisor".into(),
            reciprocal: "Advisee".into(),
            billing: false,
            shipping: true,
        };
        party_relation_update(&mut conn, &id, &edit, &actor).unwrap();
        let rows = pair_rows(&conn, "p-tf", "p-bm");
        assert_eq!((rows[0].2.as_str(), rows[0].3), ("Advisor", 0));
        assert_eq!(rows[1].2, "Advisee");
        // End, then restore, on both rows (BR-PARTY-04).
        party_relation_update(&mut conn, &id, &RelationChange::End, &actor).unwrap();
        let rows = pair_rows(&conn, "p-tf", "p-bm");
        assert!(rows.iter().all(|r| r.4 == 0 && r.5.len() == 10), "{rows:?}");
        party_relation_update(&mut conn, &id, &RelationChange::Restore, &actor).unwrap();
        assert!(pair_rows(&conn, "p-tf", "p-bm").iter().all(|r| r.4 == 1 && r.5.is_empty()));
        // Delete asks first (AP-07), then removes both rows.
        let Err(CommandError::ConfirmationRequired(summary)) = party_relation_delete(&mut conn, &id, false, &actor)
        else {
            panic!()
        };
        assert!(summary.contains("Tomás Ferreira") && summary.contains("Bluefin Marine Supply"), "{summary}");
        assert_eq!(pair_rows(&conn, "p-tf", "p-bm").len(), 2);
        party_relation_delete(&mut conn, &id, true, &actor).unwrap();
        assert!(pair_rows(&conn, "p-tf", "p-bm").is_empty());
        assert!(relations_without_reciprocal(&conn).unwrap().is_empty());
    }

    #[test]
    fn relations_of_an_archived_party_are_read_only() {
        let (mut conn, actor) = demo_user();
        conn.execute("UPDATE party SET deleted_ind = 1 WHERE id = 'p-ellison'", []).unwrap();
        let refused = |r: Result<(), CommandError>| matches!(r, Err(CommandError::Refused(_)));
        assert!(refused(party_relation_update(&mut conn, "rel-mara", &RelationChange::End, &actor)));
        assert!(refused(party_relation_delete(&mut conn, "rel-mara", true, &actor)));
        assert!(refused(party_relation_create(&mut conn, &relation("p-ellison", "p-tf", "x"), &actor).map(|_| ())));
        assert_eq!(pair_rows(&conn, "p-mara", "p-ellison").len(), 2, "nothing changed");
    }

    #[test]
    fn a_missing_reciprocal_row_is_an_error() {
        let (mut conn, actor) = demo_user();
        conn.execute("DELETE FROM party_relation WHERE id = 'rel-mara-r'", []).unwrap();
        assert!(matches!(party_relation_delete(&mut conn, "rel-mara", true, &actor), Err(CommandError::Refused(_))));
        assert!(matches!(
            party_relation_update(&mut conn, "rel-mara", &RelationChange::End, &actor),
            Err(CommandError::Refused(_))
        ));
        assert!(matches!(party_relation_delete(&mut conn, "nope", true, &actor), Err(CommandError::NotFound)));
    }

    fn org(name: &str, role: Option<RoleKind>) -> NewParty {
        NewParty {
            role,
            ..new_party(PartyFields::Organization(OrganizationFields {
                organization_name: name.into(),
                ..Default::default()
            }))
        }
    }

    #[test]
    fn wizard_creates_the_organization_the_person_their_roles_and_the_pair() {
        let (mut conn, actor) = demo_user();
        let input = WizardInput {
            organization: Some(NewParty {
                email: "hello@tidewater.example".into(),
                ..org("Tidewater Kayak", Some(RoleKind::Customer))
            }),
            person: Some(NewParty { role: Some(RoleKind::Employee), ..new_party(person("Nell", "Barrow")) }),
            relationship: "Owner".into(),
            billing_contact: true,
        };
        let (o, p) = party_create_wizard(&mut conn, &input, &actor).unwrap();
        let (o, p) = (o.unwrap(), p.unwrap());
        let org = party_get(&conn, &o).unwrap();
        assert!(org.has_role(RoleKind::Customer));
        assert_eq!(org.relations.len(), 1);
        assert_eq!(
            (org.relations[0].other_name.as_str(), org.relations[0].relationship.as_str()),
            ("Nell Barrow", "Owner")
        );
        assert!(org.relations[0].billing);
        let person = party_get(&conn, &p).unwrap();
        assert!(person.has_role(RoleKind::Employee));
        assert_eq!(person.relations[0].relationship, "Employer");
        assert!(relations_without_reciprocal(&conn).unwrap().is_empty());
        assert!(customer_roles_without_customer_row(&conn).unwrap().is_empty());
    }

    #[test]
    fn wizard_rules_and_rollback() {
        let (mut conn, actor) = demo_user();
        // A lead person only when no organization is created; an organization is never an employee.
        let both_leads = WizardInput {
            organization: Some(org("Lead Co", Some(RoleKind::Lead))),
            person: Some(NewParty { role: Some(RoleKind::Lead), ..new_party(person("Lee", "Dee")) }),
            relationship: String::new(),
            billing_contact: false,
        };
        let Err(CommandError::Validation(errors)) = party_create_wizard(&mut conn, &both_leads, &actor) else {
            panic!()
        };
        assert_eq!(errors.iter().map(|e| e.field).collect::<Vec<_>>(), ["person_role"]);
        let lone_lead = WizardInput { organization: None, ..both_leads.clone() };
        let (o, p) = party_create_wizard(&mut conn, &lone_lead, &actor).unwrap();
        assert!(o.is_none() && party_get(&conn, &p.unwrap()).unwrap().has_role(RoleKind::Lead));
        let empty =
            WizardInput { organization: None, person: None, relationship: String::new(), billing_contact: false };
        assert!(matches!(party_create_wizard(&mut conn, &empty, &actor), Err(CommandError::Validation(_))));
        // A failure in the second insert leaves nothing behind (one transaction, D-08).
        let before: i64 = conn.query_row("SELECT COUNT(*) FROM party", [], |r| r.get(0)).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_person AFTER INSERT ON person BEGIN SELECT RAISE(ABORT, 'boom'); END;")
            .unwrap();
        let input = WizardInput {
            organization: Some(org("Rollback Ltd", Some(RoleKind::Customer))),
            person: Some(new_party(person("Roll", "Back"))),
            relationship: String::new(),
            billing_contact: false,
        };
        assert!(matches!(party_create_wizard(&mut conn, &input, &actor), Err(CommandError::Storage(_))));
        let after: i64 = conn.query_row("SELECT COUNT(*) FROM party", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
        assert!(names(&conn, &q("rollback")).is_empty());
    }

    #[test]
    fn find_by_email_prefers_the_organization() {
        let (conn, _) = demo_user();
        let joanna = vec!["ELENA@haldenpryce.example".to_owned()];
        let found = party_find_by_contact(&conn, &joanna, &[], true).unwrap();
        let names: Vec<_> = found.iter().map(|f| (f.name.as_str(), f.via_relation)).collect();
        assert_eq!(names, [("Halden & Pryce Architects", true), ("Elena Halden", false)], "BR-PARTY-11");
        let found = party_find_by_contact(&conn, &joanna, &[], false).unwrap();
        assert_eq!(found.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Elena Halden"]);
        let by_phone = party_find_by_contact(&conn, &[], &["+1 (207) 555-0148".into()], false).unwrap();
        assert!(by_phone.iter().any(|f| f.name == "Elena Halden"), "{by_phone:?}");
        assert!(party_find_by_contact(&conn, &["nobody@x.example".into()], &["12".into()], true).unwrap().is_empty());
    }

    #[test]
    fn create_a_person_with_email_and_phone() {
        let (mut conn, actor) = demo_user();
        conn.execute("UPDATE system_setting SET automatically_format_phone_numbers_ind = 1, country_code = '1'", [])
            .unwrap();
        let new = NewParty {
            email: " ada@example.com ".into(),
            phone: "207.555.0199".into(),
            comments: "  met at\r\nthe fair\u{7}  ".into(),
            ..new_party(person("Ada", "Quinn"))
        };
        let id = party_create(&mut conn, &new, &actor).unwrap();
        assert_eq!(id, new.id);
        let d = party_get(&conn, &id).unwrap();
        assert_eq!(d.name, "Ada Quinn");
        assert!(!d.organization && d.person.is_some());
        assert_eq!(d.comments, "met at\nthe fair", "CleanText (BR-PARTY-08)");
        assert_eq!(one(&conn, "SELECT phone_number_cached FROM party WHERE id = ?1", &id), "(207) 555-0199");
        assert_eq!(one(&conn, "SELECT email_address_cached FROM party WHERE id = ?1", &id), "ada@example.com");
        assert!(d.methods.iter().all(|m| m.primary), "the first of each kind is primary");
        // The party, person and contact rows share the stamps of one instant by the user (BR-PARTY-07).
        let stamps: Vec<(String, String)> = conn
            .prepare(
                "SELECT creation_user_id, creation_timestamp FROM party WHERE id = ?1
                 UNION ALL SELECT creation_user_id, creation_timestamp FROM person WHERE id = ?1
                 UNION ALL SELECT creation_user_id, creation_timestamp FROM contact_method WHERE party_id = ?1",
            )
            .unwrap()
            .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(stamps.len(), 4);
        assert!(stamps.iter().all(|s| s == &stamps[0] && Some(&s.0) == actor.user_id.as_ref()));
    }

    #[test]
    fn create_an_organization_customer_and_refuse_its_id_twice() {
        let (mut conn, actor) = demo_user();
        let new = NewParty {
            role: Some(RoleKind::Customer),
            ..new_party(PartyFields::Organization(OrganizationFields {
                organization_name: "Pier Nine Ropes".into(),
                ..Default::default()
            }))
        };
        let id = party_create(&mut conn, &new, &actor).unwrap();
        let d = party_get(&conn, &id).unwrap();
        assert!(d.organization && d.has_role(RoleKind::Customer));
        assert_eq!(d.customer.as_ref().unwrap().balance, 0);
        let role = &d.roles[0];
        assert_eq!(one(&conn, "SELECT CAST(party_role_type_id AS TEXT) FROM party_role WHERE id = ?1", &role.id), "2");
        assert!(
            matches!(party_create(&mut conn, &new, &actor), Err(CommandError::AlreadyExists(x)) if x == id),
            "BR-PARTY-10"
        );
        assert!(customer_roles_without_customer_row(&conn).unwrap().is_empty());
    }

    /// Ep03: a customer made in the wizard showed "Not taxable". The source makes a new customer taxable (kontroller
    /// 229 step 154) and leaves a customer role added later as given, which is empty (705 step 180).
    #[test]
    fn a_new_customer_is_taxable_and_a_customer_role_added_later_is_not() {
        let (mut conn, actor) = demo_user();
        let taxable = |conn: &Connection, id: &str| party_get(conn, id).unwrap().customer.unwrap().taxable;
        let made = party_create(&mut conn, &org("Pier Nine Ropes", Some(RoleKind::Customer)), &actor).unwrap();
        assert!(taxable(&conn, &made), "created with its customer role");
        let input = WizardInput {
            organization: Some(org("Tidewater Kayak", Some(RoleKind::Customer))),
            person: None,
            relationship: String::new(),
            billing_contact: false,
        };
        let (wizard, _) = party_create_wizard(&mut conn, &input, &actor).unwrap();
        assert!(taxable(&conn, &wizard.unwrap()), "created in the wizard");
        let plain = party_create(&mut conn, &org("Gull Rock Ferry", None), &actor).unwrap();
        party_role_add(&mut conn, &plain, RoleKind::Customer, &actor).unwrap();
        assert!(!taxable(&conn, &plain), "customer role added to an existing party");
        assert!(taxable(&conn, "p-hp") && !taxable(&conn, "p-bm"), "existing customers are not changed");
    }

    #[test]
    fn create_refuses_bad_input_and_leaves_nothing() {
        let (mut conn, actor) = demo_user();
        let before: i64 = conn.query_row("SELECT COUNT(*) FROM party", [], |r| r.get(0)).unwrap();
        let bad = NewParty { email: "not-an-email".into(), phone: "call me".into(), ..new_party(person(" ", "")) };
        let Err(CommandError::Validation(errors)) = party_create(&mut conn, &bad, &actor) else { panic!() };
        let fields: Vec<_> = errors.iter().map(|e| e.field).collect();
        assert_eq!(fields, ["first_name", "email", "phone"]);
        let bad = NewParty { id: "x'; DROP TABLE party; --".into(), ..new_party(person("A", "B")) };
        assert!(matches!(party_create(&mut conn, &bad, &actor), Err(CommandError::Validation(_))));
        // A failing write after the first insert rolls the whole create back (one transaction).
        conn.execute_batch("CREATE TRIGGER fail_person AFTER INSERT ON person BEGIN SELECT RAISE(ABORT, 'boom'); END;")
            .unwrap();
        let err = party_create(&mut conn, &new_party(person("Roll", "Back")), &actor).unwrap_err();
        assert!(matches!(err, CommandError::Storage(_)), "{err}");
        let after: i64 = conn.query_row("SELECT COUNT(*) FROM party", [], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn update_changes_the_fields_the_name_cache_and_the_stamps() {
        let (mut conn, actor) = demo_user();
        let fields = PartyFields::Person(PersonFields {
            first_name: "Mara".into(),
            last_name: "Ellison-Hart".into(),
            job_title: "Principal".into(),
            ..Default::default()
        });
        party_update(&mut conn, "p-mara", &fields, "\tLikes early calls. ", &actor).unwrap();
        let d = party_get(&conn, "p-mara").unwrap();
        assert_eq!(d.name, "Mara Ellison-Hart");
        assert_eq!(d.person.as_ref().unwrap().job_title, "Principal");
        assert_eq!(d.comments, "Likes early calls.");
        assert_eq!(
            one(&conn, "SELECT modification_user_id FROM party WHERE id = ?1", "p-mara"),
            actor.user_id.clone().unwrap()
        );
        let org = PartyFields::Organization(OrganizationFields { organization_name: "X".into(), ..Default::default() });
        assert!(matches!(party_update(&mut conn, "p-mara", &org, "", &actor), Err(CommandError::Validation(_))));
        assert!(matches!(party_update(&mut conn, "nobody", &fields, "", &actor), Err(CommandError::NotFound)));
    }

    #[test]
    fn archive_hides_the_party_and_keeps_its_records() {
        let (mut conn, actor) = demo_user();
        let sales = |c: &Connection| -> i64 {
            c.query_row("SELECT COUNT(*) FROM sales_transaction WHERE id_customer = 'r-hl'", [], |r| r.get(0)).unwrap()
        };
        let before = sales(&conn);
        party_archive(&mut conn, "p-hl", "  Closed the shop ", &actor).unwrap();
        let d = party_get(&conn, "p-hl").unwrap();
        assert!(d.archived);
        assert_eq!(d.deleted_reason, "Closed the shop");
        assert_eq!(one(&conn, "SELECT deleted_account_name FROM party WHERE id = ?1", "p-hl"), actor.account_name);
        assert!(crate::audit::is_timestamp(&d.deleted_timestamp));
        assert_eq!(sales(&conn), before, "no cascade: the sales stay");
        assert!(!d.methods.is_empty(), "contact methods stay");
        assert!(party_search(&conn, &q("harbor lane")).unwrap().rows.is_empty());
        party_archive(&mut conn, "p-hl", "again", &actor).unwrap();
        assert_eq!(
            party_get(&conn, "p-hl").unwrap().deleted_reason,
            "Closed the shop",
            "archiving twice changes nothing"
        );
        assert!(matches!(
            party_update(&mut conn, "p-hl", &person("a", "b"), "", &actor),
            Err(CommandError::Refused(_))
        ));
        assert!(matches!(party_archive(&mut conn, "nobody", "", &actor), Err(CommandError::NotFound)));
    }

    #[test]
    fn role_add_is_idempotent() {
        let (mut conn, actor) = demo_user();
        let first = party_role_add(&mut conn, "p-tf", RoleKind::Lead, &actor).unwrap();
        let again = party_role_add(&mut conn, "p-tf", RoleKind::Lead, &actor).unwrap();
        assert_eq!(first, again);
        assert_eq!(one(&conn, "SELECT CAST(party_role_type_id AS TEXT) FROM party_role WHERE id = ?1", &first), "4");
        assert_eq!(one(&conn, "SELECT name_cached FROM lead WHERE id = ?1", &first), "Tomás Ferreira");
        // The existing customer role of Halden & Pryce is returned, not a second one.
        assert_eq!(party_role_add(&mut conn, "p-hp", RoleKind::Customer, &actor).unwrap(), "r-hp");
        // Marigold's former customer role is made current again.
        assert_eq!(party_role_add(&mut conn, "p-me", RoleKind::Customer, &actor).unwrap(), "r-me");
        assert!(party_get(&conn, "p-me").unwrap().has_role(RoleKind::Customer));
        // Employee and vendor roles have no type in the source (CHECK-03): only the flag.
        let v = party_role_add(&mut conn, "p-bm", RoleKind::Vendor, &actor).unwrap();
        assert_eq!(one(&conn, "SELECT CAST(party_role_type_id AS TEXT) FROM party_role WHERE id = ?1", &v), "");
        assert_eq!(one(&conn, "SELECT CAST(vendor_ind AS TEXT) FROM party_role WHERE id = ?1", &v), "1");
        assert!(matches!(party_role_add(&mut conn, "nobody", RoleKind::Lead, &actor), Err(CommandError::NotFound)));
    }

    #[test]
    fn cache_columns_give_the_primary_or_the_first_method() {
        let (conn, _) = demo_user();
        // The demo seed's caches already hold what the expressions derive.
        for id in ["p-hp", "p-ellison", "p-mara", "p-bm"] {
            let before = party_get(&conn, id).unwrap();
            let cached = |c: &Connection| -> (String, String) {
                c.query_row(
                    "SELECT COALESCE(phone_number_cached, ''), COALESCE(email_address_cached, '') FROM party WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
            };
            let seeded = cached(&conn);
            refresh_caches(&conn, id).unwrap();
            assert_eq!(cached(&conn), seeded, "{id}");
            assert_eq!(party_get(&conn, id).unwrap().name, before.name);
        }
    }

    #[test]
    fn clean_text_rule() {
        assert_eq!(clean_text("  a\u{0}b\r\nc\t \u{1b}"), "ab\nc");
        assert_eq!(clean_text("\n\n"), "");
    }

    #[test]
    fn party_get_of_an_organization_with_contacts_and_a_customer_role() {
        let conn = demo();
        let hp = party_get(&conn, "p-hp").unwrap();
        assert_eq!(hp.name, "Halden & Pryce Architects");
        assert!(hp.organization && hp.person.is_none());
        assert_eq!(hp.org.as_ref().unwrap().company_type, "Architecture & interiors");
        assert!(hp.has_role(RoleKind::Customer) && !hp.has_role(RoleKind::Lead));
        let c = hp.customer.as_ref().unwrap();
        assert_eq!((c.role_id.as_str(), c.balance, c.retainer), ("r-hp", 305_715, 150_000), "the design's tiles");
        let names: Vec<_> = hp.relations.iter().map(|r| (r.other_name.as_str(), r.relationship.as_str())).collect();
        assert_eq!(
            names,
            [("Elena Halden", "Principal"), ("Joanna Whitfield", "Office manager"), ("Marcus Pryce", "Partner")]
        );
        assert!(hp.relations.iter().all(|r| r.reciprocal == "Employer"));
        assert!(hp.relations.iter().find(|r| r.other_name == "Joanna Whitfield").unwrap().billing);
        assert!(!hp.methods.is_empty());
    }

    #[test]
    fn party_get_of_a_person_and_of_nobody() {
        let conn = demo();
        let mara = party_get(&conn, "p-mara").unwrap();
        assert_eq!(mara.person.as_ref().unwrap().first_name, "Mara");
        assert!(mara.has_role(RoleKind::Employee) && mara.customer.is_none());
        assert_eq!(mara.relations.len(), 1);
        assert_eq!(
            (mara.relations[0].other_name.as_str(), mara.relations[0].relationship.as_str()),
            ("Ellison Studio", "Employer")
        );
        assert!(matches!(party_get(&conn, "nobody"), Err(CommandError::NotFound)));
    }

    #[test]
    fn words_are_cut_to_the_digits_per_word() {
        assert_eq!(search_words("  Halden   PRYCE ", false, 0), ["halden", "pryce"]);
        assert_eq!(search_words("Architectural", false, 0), ["architectura"]);
        assert_eq!(search_words("Architectural", false, 4), ["arch"]);
        assert_eq!(search_words("Architectural", true, 4), ["architectural"]);
        assert!(search_words("   ", false, 0).is_empty());
    }

    #[test]
    fn every_word_must_start_a_word_of_the_name() {
        let conn = demo();
        assert_eq!(names(&conn, &q("hal pry")), ["Halden & Pryce Architects"]);
        assert_eq!(names(&conn, &q("pry hal")), ["Halden & Pryce Architects"], "word order does not matter");
        assert!(names(&conn, &q("alden")).is_empty(), "inside a word is not a match");
        // Cut to 4 letters, "Architectural" still finds "Architects".
        let s = Search { digits_per_word: 4, ..q("Architectural") };
        assert_eq!(names(&conn, &s), ["Halden & Pryce Architects"]);
    }

    #[test]
    fn full_word_search_needs_whole_words() {
        let conn = demo();
        let s = Search { full_word: true, ..q("hald") };
        assert!(names(&conn, &s).is_empty());
        let s = Search { full_word: true, ..q("halden") };
        assert_eq!(names(&conn, &s), ["Elena Halden", "Halden & Pryce Architects"]);
    }

    #[test]
    fn like_characters_are_literal() {
        let conn = demo();
        assert!(names(&conn, &q("%")).is_empty());
        assert!(names(&conn, &q("_")).is_empty());
        assert_eq!(names(&conn, &q("&")), ["Cedar & Slate Realty", "Halden & Pryce Architects"]);
    }

    #[test]
    fn kind_and_role_filters() {
        let conn = demo();
        let all = party_search(&conn, &Search::default()).unwrap();
        assert_eq!(all.matched, 30);
        let people = party_search(&conn, &Search { kind: Kind::Person, ..Default::default() }).unwrap();
        let orgs = party_search(&conn, &Search { kind: Kind::Organization, ..Default::default() }).unwrap();
        assert_eq!((people.matched, orgs.matched), (17, 13));
        assert!(people.rows.iter().all(|r| !r.organization));
        let customers = party_search(&conn, &Search { role: RoleFilter::Customer, ..Default::default() }).unwrap();
        assert_eq!(customers.matched, 13, "14 customer roles, Marigold's is not current");
        assert!(customers.rows.iter().all(|r| r.customer));
        let staff = party_search(&conn, &Search { role: RoleFilter::Employee, ..Default::default() }).unwrap();
        assert_eq!(staff.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Mara Ellison"]);
    }

    #[test]
    fn archived_parties_are_hidden_unless_asked_for() {
        let conn = demo();
        conn.execute("UPDATE party SET deleted_ind = 1 WHERE id = 'p-hp'", []).unwrap();
        assert!(names(&conn, &q("pryce architects")).is_empty());
        let s = Search { include_deleted: true, ..q("pryce architects") };
        let page = party_search(&conn, &s).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert!(page.rows[0].archived);
    }

    #[test]
    fn paging_is_sorted_by_name_and_clamped() {
        let conn = demo();
        let s = Search { page_size: 8, page: 4, ..Default::default() };
        let page = party_search(&conn, &s).unwrap();
        assert_eq!((page.page, page.pages, page.rows.len()), (4, 4, 6));
        let s = Search { page_size: 8, page: 99, ..Default::default() };
        assert_eq!(party_search(&conn, &s).unwrap().page, 4);
        let first = names(&conn, &Search { page_size: 3, ..Default::default() });
        assert_eq!(first, ["Atlas Freight Partners", "Ben Achebe", "Bluefin Marine Supply"]);
    }

    #[test]
    fn at_most_max_rows_can_be_paged() {
        let conn = demo();
        conn.execute_batch(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 600)
             INSERT INTO party (id, name_cached, person_ind, organization_ind, deleted_ind)
             SELECT 'bulk-' || i, 'Bulk ' || printf('%04d', i), 1, 0, 0 FROM n;",
        )
        .unwrap();
        let page = party_search(&conn, &Search { page: 99, ..q("bulk") }).unwrap();
        assert_eq!(page.matched, 600);
        assert!(page.capped);
        assert_eq!(page.pages, 10);
        assert_eq!(page.rows.len(), 50);
        assert_eq!(page.rows.last().unwrap().name, "Bulk 0500");
    }
}
