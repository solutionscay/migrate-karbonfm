//! PARTY commands of I2 (05 OP-PARTY-*, 07 BR-PARTY-*): search, read, create, edit, archive, roles, relations and the
//! create-party wizard. Plain SQL; each write command is one `BEGIN IMMEDIATE` transaction and stamps every row it
//! writes (BR-PARTY-07). Routes parse forms and render; they never run SQL (AP-01).

use crate::admin::CommandError;
use crate::contacts::{self, Method};
use rusqlite::{params_from_iter, Connection, OptionalExtension};

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
