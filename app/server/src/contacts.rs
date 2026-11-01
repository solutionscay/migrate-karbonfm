//! CONTACT commands of I2 (05 OP-CONTACT-*, 07 BR-CONTACT-*): a party's phones, network addresses (email and web)
//! and postal addresses. A contact method is a `contact_method` row plus one subtype row with the same id (D-15);
//! purposes (billing, shipping, mailing) are `contact_method_purpose` rows with a cached flag on the contact method.

use crate::reference::ContactMethodPurposeType;
use rusqlite::{Connection, OptionalExtension};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open_in_memory, Seed};

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
