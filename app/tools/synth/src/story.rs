//! The clean part of the export: a small interiors and wayfinding studio's 46 months of business.

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};

use crate::{
    fm_bool, fm_date, fm_money, fm_qty, fm_text, fm_time, fm_timestamp, placeholder, Config, Export, Rng, Row,
};

/// The anchors kept from 04_demo_seed.sql.
pub const ANCHOR_OWNER_ORG: &str = "p-ellison";
pub const ANCHOR_OWNER_PERSON: &str = "p-mara";
pub const ANCHOR_INVOICE: &str = "t-inv-1048";

const ANCHOR_HP: &str = "p-hp";
const ANCHOR_HP_ROLE: &str = "r-hp";

/// The sales tax rate of the studio (Maine, 5.5%), in basis points.
pub const TAX_RATE_BP: i64 = 550;

fn start() -> NaiveDate {
    NaiveDate::from_ymd_opt(2023, 1, 2).unwrap()
}
fn end() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 31).unwrap()
}

/// Builds a row from (field, value) pairs; empty values are left out (an empty field).
pub(crate) fn row(pairs: &[(&'static str, String)]) -> Row {
    pairs.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (*k, v.clone())).collect()
}

#[derive(Clone)]
struct User {
    id: &'static str,
    account: &'static str,
}

const USERS: [User; 3] = [
    User { id: "u-mara", account: "mara" },
    User { id: "u-devon", account: "devon" },
    User { id: "u-priya", account: "priya" },
];

#[derive(Clone)]
struct Customer {
    role: String,
    party: String,
    name: String,
    weight: f64,
    since: NaiveDate,
    until: Option<NaiveDate>,
    terms: i64,
    taxable: bool,
    bill: [String; 4],
    email: String,
    contact: Option<String>,
    retainer: i64,
}

#[derive(Clone)]
struct Item {
    id: String,
    name: &'static str,
    price: i64,
    taxable: bool,
    hourly: bool,
}

struct G<'a> {
    x: &'a mut Export,
    r: Rng,
    /// The draws of the invoices collected late (`pay`): a stream of its own, so the rest of the export stays the same.
    late: Rng,
    scale: f64,
    employees: Vec<(String, String, String)>, // (party id, role id, name)
    customers: Vec<Customer>,
    leads: Vec<(String, String, String)>, // (party id, role id, name)
    items: Vec<Item>,
    txns: Vec<(String, String, NaiveDate, String)>, // (txn id, customer party id, date, doc number)
    discussions: Vec<(String, String)>,             // (activity id, title)
    org_names: BTreeMap<String, String>,            // (party id, organization name)
    serial: BTreeMap<&'static str, u32>,
}

pub fn build(x: &mut Export, cfg: Config) {
    let mut g = G {
        x,
        r: Rng::new(cfg.seed),
        late: Rng::new(cfg.seed ^ 0x6c61_7465),
        scale: cfg.scale,
        employees: Vec::new(),
        customers: Vec::new(),
        leads: Vec::new(),
        items: Vec::new(),
        txns: Vec::new(),
        discussions: Vec::new(),
        org_names: BTreeMap::new(),
        serial: BTreeMap::new(),
    };
    g.reference();
    g.owner_and_staff();
    g.clients();
    g.individuals();
    g.leads();
    g.vendors();
    g.items();
    g.halden_pryce();
    g.sales();
    g.opportunities();
    g.activities();
    g.templates();
    g.files();
    g.notifications();
}

const FIRST: &[&str] = &[
    "Avery", "Jordan", "Riley", "Casey", "Morgan", "Quinn", "Harper", "Rowan", "Emerson", "Sage", "Elena", "Marcus",
    "Nadia", "Owen", "Grace", "Julian", "Leah", "Mateo", "Ines", "Caleb", "Hannah", "Isaac", "Maya", "Noah", "Olivia",
    "Samuel", "Tessa", "Victor", "Wren", "Zoe", "Amara", "Ben", "Clara", "Diego", "Esme", "Felix", "Greta", "Hugo",
    "Iris", "Jonah", "Kira", "Liam", "Mira", "Nico", "Opal", "Pavel", "Rosa", "Silas", "Talia", "Uma", "Will", "Yara",
    "Zane", "Ada", "Bram", "Cora", "Dmitri", "Effie", "Farah", "Gideon", "Hana", "Ivo", "José", "Søren",
];
const LAST: &[&str] = &[
    "Abbott",
    "Barrows",
    "Carver",
    "Dunlap",
    "Ellery",
    "Fairbanks",
    "Gilman",
    "Hatch",
    "Ingalls",
    "Jewett",
    "Kimball",
    "Libby",
    "Merrill",
    "Nason",
    "Orcutt",
    "Pike",
    "Quimby",
    "Rand",
    "Sewall",
    "Thaxter",
    "Upham",
    "Varney",
    "Whitten",
    "Yeaton",
    "Achorn",
    "Brackett",
    "Coffin",
    "Dyer",
    "Emery",
    "Frost",
    "Goodwin",
    "Hamlin",
    "Ireland",
    "Jordan",
    "Knight",
    "Lord",
    "Moody",
    "Noyes",
    "Osgood",
    "Pratt",
    "Ricker",
    "Small",
    "Tibbetts",
    "Usher",
    "Vose",
    "Wescott",
    "Young",
    "Nguyen",
    "Okafor",
    "Patel",
    "Rossi",
    "Sato",
    "Kowalski",
    "Haddad",
    "Lindqvist",
    "O'Brien",
    "Müller",
];
const PLACE: &[&str] = &[
    "Harbor",
    "Casco",
    "Granite",
    "Back Cove",
    "Old Port",
    "Fore River",
    "Bayside",
    "Munjoy",
    "Willard",
    "Spring Point",
    "Deering",
    "Riverton",
    "Stroudwater",
    "Cumberland",
    "Falmouth",
    "Yarmouth",
    "Kennebec",
    "Penobscot",
    "Saco",
    "Sebago",
    "Katahdin",
    "Acadia",
    "Pemaquid",
    "Monhegan",
    "Damariscotta",
    "Bowdoin",
    "Merrymeeting",
    "Royal River",
    "Presumpscot",
    "Scarborough",
    "Cape",
    "Pineland",
    "Tidewater",
    "Lighthouse",
    "North Star",
    "Blue Heron",
    "Driftwood",
    "Ironbound",
    "Saltmarsh",
    "Highland",
    "Mill Creek",
    "Ledgewood",
    "Birchwood",
    "Seaglass",
    "Two Lights",
    "Peaks",
];
const KIND: &[(&str, &str, &str)] = &[
    ("Architects", "Architecture", "arch"),
    ("Dental Group", "Healthcare", "dental"),
    ("Brewing Co.", "Food & beverage", "brew"),
    ("Bakery", "Food & beverage", "bakery"),
    ("Marine Supply", "Retail", "marine"),
    ("Realty", "Real estate", "realty"),
    ("Law Group", "Professional services", "law"),
    ("Family Medicine", "Healthcare", "med"),
    ("Coffee Roasters", "Food & beverage", "coffee"),
    ("Outfitters", "Retail", "outfit"),
    ("Animal Hospital", "Healthcare", "vet"),
    ("Credit Union", "Finance", "cu"),
    ("Public Library", "Public sector", "library"),
    ("Yoga Studio", "Fitness", "yoga"),
    ("Builders", "Construction", "build"),
    ("Hotel", "Hospitality", "hotel"),
    ("Bistro", "Hospitality", "bistro"),
    ("Cycle Works", "Retail", "cycle"),
    ("Montessori School", "Education", "school"),
    ("Physical Therapy", "Healthcare", "pt"),
    ("Development Partners", "Real estate", "dev"),
    ("Museum", "Arts & culture", "museum"),
];
const STREETS: &[&str] = &[
    "Fore Street",
    "Commercial Street",
    "Congress Street",
    "Exchange Street",
    "Middle Street",
    "Forest Avenue",
    "Washington Avenue",
    "Brighton Avenue",
    "Stevens Avenue",
    "Ocean Avenue",
    "Main Street",
    "Water Street",
    "Elm Street",
    "High Street",
    "Pleasant Street",
    "Route 1",
    "Bridge Street",
    "Free Street",
    "India Street",
];
const TOWNS: &[(&str, &str, &str)] = &[
    ("Portland", "ME", "04101"),
    ("Portland", "ME", "04103"),
    ("South Portland", "ME", "04106"),
    ("Westbrook", "ME", "04092"),
    ("Falmouth", "ME", "04105"),
    ("Yarmouth", "ME", "04096"),
    ("Brunswick", "ME", "04011"),
    ("Biddeford", "ME", "04005"),
    ("Saco", "ME", "04072"),
    ("Scarborough", "ME", "04074"),
    ("Freeport", "ME", "04032"),
    ("Bath", "ME", "04530"),
    ("Rockland", "ME", "04841"),
    ("Kennebunk", "ME", "04043"),
    ("Portsmouth", "NH", "03801"),
    ("Dover", "NH", "03820"),
    ("Newburyport", "MA", "01950"),
];
const JOBS: &[&str] = &[
    "Owner",
    "Principal",
    "Office manager",
    "Operations director",
    "Facilities manager",
    "Marketing lead",
    "Project manager",
    "Partner",
    "Executive director",
    "General manager",
    "Bookkeeper",
    "Practice manager",
];

impl G<'_> {
    fn next_serial(&mut self, key: &'static str, first: u32) -> u32 {
        let n = self.serial.entry(key).or_insert(first - 1);
        *n += 1;
        *n
    }

    fn user(&mut self) -> User {
        USERS[self.r.weighted(&[0.6, 0.25, 0.15])].clone()
    }

    /// A time of day during office hours on `d`.
    fn at(&mut self, d: NaiveDate) -> NaiveDateTime {
        d.and_hms_opt(self.r.range(8, 17) as u32, self.r.range(0, 59) as u32, self.r.range(0, 59) as u32).unwrap()
    }

    /// Fills the creation and modification stamps the table has (D-39: imported with every record).
    fn stamp(&mut self, table: &str, row: &mut Row, created: NaiveDateTime) {
        let u = self.user();
        let m = self.user();
        let later = (created + Duration::days(self.r.range(0, 40)) + Duration::minutes(self.r.range(0, 600)))
            .min(end().and_hms_opt(18, 0, 0).unwrap())
            .max(created);
        let pairs: [(&'static str, String); 8] = [
            ("CreationTimestamp", fm_timestamp(created)),
            ("CreationAccountName", u.account.into()),
            ("CreationUserId", u.id.into()),
            ("ModificationTimestamp", fm_timestamp(later)),
            ("ModificationAccountName", m.account.into()),
            ("ModificationUserId", m.id.into()),
            ("CreatedBy", u.account.into()),
            ("ModifiedBy", m.account.into()),
        ];
        for (k, v) in pairs {
            if self.x.has_field(table, k) {
                row.insert(k, v);
            }
        }
    }

    fn push(&mut self, table: &'static str, mut r: Row, created: NaiveDateTime) {
        self.stamp(table, &mut r, created);
        self.x.push(table, r);
    }

    fn reference(&mut self) {
        let t0 = NaiveDate::from_ymd_opt(2019, 4, 1).unwrap().and_hms_opt(9, 0, 0).unwrap();
        let types = ["Email", "Note", "Task", "Event", "Comment", "Meeting", "Letter", "Call", "SMS", "Discussion"];
        for (i, n) in types.iter().enumerate() {
            self.push("ActivityType", row(&[("Id", (i + 1).to_string()), ("Description", n.to_string())]), t0);
        }
        let roles = [
            "Creator",
            "Completed By",
            "Assigned By",
            "Assigned To",
            "From",
            "To",
            "Cc",
            "Bcc",
            "Notification From",
            "Notification To",
            "Invited",
            "Unused",
            "Organizer",
            "Regarding",
            "Subscriber",
            "Spoken With",
        ];
        for (i, n) in roles.iter().enumerate() {
            let id = 50 + i;
            let email = matches!(id, 54..=57);
            let event = matches!(id, 60 | 62);
            // ActivityTypeIds of kontroller 710 s34: the activity types each role fits (BR-ACTIVITY-03).
            let types: &[u8] = match id {
                50 => &[5, 4, 2, 3, 1, 6, 8, 9, 10],
                51 => &[5, 1, 4, 2, 3, 6, 8],
                52 | 53 => &[3, 5],
                54..=57 => &[1, 7],
                58 | 59 => &[5, 1, 4, 2, 3, 8],
                60 | 62 => &[4, 6],
                63 => &[2, 3, 8],
                64 => &[10],
                65 => &[8],
                _ => &[],
            };
            let types = fm_text(&types.iter().map(u8::to_string).collect::<Vec<_>>().join("\n"));
            self.push(
                "ActivityPartyRole",
                row(&[
                    ("Id", id.to_string()),
                    ("Name", n.to_string()),
                    ("ActivityTypeIds", types),
                    ("CreateActivityPartyEmailInd", if email { "1".into() } else { String::new() }),
                    ("CreateActivityPartyEventInd", if event { "1".into() } else { String::new() }),
                ]),
                t0,
            );
        }
        let statuses = [
            ("New", 1),
            ("In process", 1),
            ("Proposal sent", 1),
            ("Won", 0),
            ("Lost", 0),
            ("Unqualified", 0),
            ("Closed (other)", 0),
            ("On Hold", 1),
        ];
        for (i, (n, a)) in statuses.iter().enumerate() {
            let r = row(&[("Id", (i + 1).to_string()), ("Name", n.to_string()), ("ActiveInd", a.to_string())]);
            self.push("OpportunityStatus", r, t0);
        }
        for (id, n) in
            [(10, "Party"), (11, "Activity"), (12, "Sales Transaction"), (13, "Opportunity"), (14, "Chat Message")]
        {
            self.push("EntityType", row(&[("Id", id.to_string()), ("Name", n.into())]), t0);
        }
        // CHECK-03 (10_ddr_schema_audit.md): only 2 and 4 are hard-coded in the source; 1 and 3 are this export's.
        for (id, n) in [(1, "Employee"), (2, "Customer"), (3, "Vendor"), (4, "Lead")] {
            self.push("PartyRoleType", row(&[("Id", id.to_string()), ("Name", n.into())]), t0);
        }
        for (id, n) in [(1, "Billing"), (2, "Shipping"), (3, "Mailing")] {
            self.push("ContactMethodPurposeType", row(&[("Id", id.to_string()), ("Name", n.into())]), t0);
        }
        let links = [
            (1, "Website", "Party"),
            (2, "LinkedIn", "Party"),
            (3, "Google Places", "Address"),
            (4, "FullContact", "Party"),
            (5, "QBO", "PartyRole"),
            (6, "Harvest", "Party"),
        ];
        for (id, e, i) in links {
            let r = row(&[("Id", id.to_string()), ("ExternalType", e.into()), ("InternalType", i.into())]);
            self.push("ExternalLinkType", r, t0);
        }
        let countries = [
            ("United States", "1", "US"),
            ("Canada", "1", "CA"),
            ("United Kingdom", "44", "GB"),
            ("Ireland", "353", "IE"),
            ("France", "33", "FR"),
            ("Germany", "49", "DE"),
            ("Italy", "39", "IT"),
            ("Spain", "34", "ES"),
            ("Mexico", "52", "MX"),
            ("Japan", "81", "JP"),
            ("Australia", "61", "AU"),
            ("Norway", "47", "NO"),
        ];
        for (i, (n, code, iso)) in countries.iter().enumerate() {
            let r = row(&[
                ("Id", (i + 1).to_string()),
                ("Country", n.to_string()),
                ("PhoneCode", code.to_string()),
                ("ISOCode", iso.to_string()),
                ("Idd", if *code == "1" { "011".into() } else { "00".into() }),
            ]);
            self.x.push("Country", r);
        }
        for (f, cc) in [("(###) ###-####", "1"), ("+44 #### ######", "44"), ("+33 # ## ## ## ##", "33")] {
            let id = self.r.uuid();
            self.push("PhoneFormat", row(&[("ID", id), ("Format", f.into()), ("CountryCode", cc.into())]), t0);
        }
    }

    fn party(&mut self, id: &str, name: &str, org: bool, created: NaiveDateTime) {
        let r = row(&[
            ("Id", id.into()),
            ("NameCached", name.into()),
            ("OrganizationInd", fm_bool(org)),
            ("PersonInd", fm_bool(!org)),
            ("DeletedInd", "0".into()),
        ]);
        self.push("Party", r, created);
    }

    fn organization(&mut self, id: &str, name: &str, kind: &str, tax_id: String, created: NaiveDateTime) {
        self.party(id, name, true, created);
        self.org_names.insert(id.to_string(), name.to_string());
        let r = row(&[
            ("Id", id.into()),
            ("OrganizationName", name.into()),
            ("CompanyType", kind.into()),
            ("FederalTaxIdNumber", tax_id),
        ]);
        self.push("Organization", r, created);
    }

    fn person(&mut self, id: &str, first: &str, last: &str, job: &str, created: NaiveDateTime) {
        self.person_at(id, first, last, job, "", created);
    }

    /// A person made with their organization: EmployerName is the organization's name, as Create Customer V3
    /// plucks it (kontroller 229 s229; 246 s143). `{{OrganizationName}}` of a person merges from it.
    fn person_at(&mut self, id: &str, first: &str, last: &str, job: &str, org: &str, created: NaiveDateTime) {
        self.party(id, &format!("{first} {last}"), false, created);
        let employer = self.org_names.get(org).cloned().unwrap_or_default();
        let r = row(&[
            ("Id", id.into()),
            ("FirstName", first.into()),
            ("LastName", last.into()),
            ("JobTitle", job.into()),
            ("EmployerName", employer),
        ]);
        self.push("Person", r, created);
    }

    fn contact_method(
        &mut self,
        id: &str,
        party: &str,
        label: &str,
        kind: (bool, bool, bool),
        purposes: &[i64],
        at: NaiveDateTime,
    ) {
        let r = row(&[
            ("Id", id.into()),
            ("PartyId", party.into()),
            ("Label", label.into()),
            ("PhoneInd", fm_bool(kind.0)),
            ("AddressInd", fm_bool(kind.1)),
            ("NetworkAddressInd", fm_bool(kind.2)),
            ("_BillingIndCached", fm_bool(purposes.contains(&1))),
            ("_ShippingIndCached", fm_bool(purposes.contains(&2))),
            ("_MailingIndCached", fm_bool(purposes.contains(&3))),
        ]);
        self.push("ContactMethod", r, at);
        for p in purposes {
            let pid = self.r.uuid();
            let r = row(&[("Id", pid), ("ContactMethodId", id.into()), ("ContactMethodPurposeTypeId", p.to_string())]);
            self.push("ContactMethodPurpose", r, at);
        }
    }

    fn address(&mut self, party: &str, label: &str, a: &[String; 4], purposes: &[i64], at: NaiveDateTime) -> String {
        let id = self.r.uuid();
        self.contact_method(&id, party, label, (false, true, false), purposes, at);
        let r = row(&[
            ("Id", id.clone()),
            ("AddressLine1", a[0].clone()),
            ("City", a[1].clone()),
            ("StateProvince", a[2].clone()),
            ("PostalCode", a[3].clone()),
            ("Country", "United States".into()),
            ("PrimaryInd", "1".into()),
        ]);
        self.push("Address", r, at);
        id
    }

    fn phone(&mut self, party: &str, label: &str, number: &str, sms: bool, at: NaiveDateTime) -> String {
        let id = self.r.uuid();
        self.contact_method(&id, party, label, (true, false, false), &[], at);
        let r = row(&[
            ("Id", id.clone()),
            ("Number", number.into()),
            ("CountryCode", "1".into()),
            ("Country", "United States".into()),
            ("PrimaryInd", "1".into()),
            ("VoiceInd", "1".into()),
            ("SmsInd", fm_bool(sms)),
        ]);
        self.push("Phone", r, at);
        id
    }

    fn network(&mut self, party: &str, label: &str, address: &str, web: bool, purposes: &[i64], at: NaiveDateTime) {
        let id = self.r.uuid();
        self.contact_method(&id, party, label, (false, false, true), purposes, at);
        let r = row(&[
            ("Id", id),
            ("Address", address.into()),
            ("PrimaryInd", fm_bool(!web)),
            ("EmailAddressInd", fm_bool(!web)),
            ("WebAddressInd", fm_bool(web)),
        ]);
        self.push("NetworkAddress", r, at);
    }

    fn role(&mut self, party: &str, kind: i64, from: NaiveDate, thru: Option<NaiveDate>, at: NaiveDateTime) -> String {
        let id = self.r.uuid();
        self.role_with_id(&id, party, kind, from, thru, at);
        id
    }

    fn role_with_id(
        &mut self,
        id: &str,
        party: &str,
        kind: i64,
        from: NaiveDate,
        thru: Option<NaiveDate>,
        at: NaiveDateTime,
    ) {
        let r = row(&[
            ("Id", id.into()),
            ("PartyId", party.into()),
            ("PartyRoleTypeId", kind.to_string()),
            ("EmployeeInd", fm_bool(kind == 1)),
            ("CustomerInd", fm_bool(kind == 2)),
            ("VendorInd", fm_bool(kind == 3)),
            ("LeadInd", fm_bool(kind == 4)),
            ("FromDate", fm_date(from)),
            ("ThruDate", thru.map(fm_date).unwrap_or_default()),
            ("CurrentInd", fm_bool(thru.is_none())),
        ]);
        self.push("PartyRole", r, at);
    }

    /// A relation pair (BR-PARTY-03): `from` is `what` of `to` (the person is the employee, owner, ...), and the
    /// reciprocal row says `to` is the employer of `from`. The reciprocal row takes nothing from the random stream, so
    /// the rest of the export stays the same: its id is the forward id with the UUID version digit 5 instead of 4 (it
    /// cannot collide with a generated v4 id), and it carries the forward row's stamps.
    fn relation(&mut self, from: &str, to: &str, what: &str, billing: bool, at: NaiveDateTime) {
        let id = self.r.uuid();
        let back = format!("{}5{}", &id[..14], &id[15..]);
        let pair = |id: String, from: &str, to: &str, what: &str, billing: bool| {
            row(&[
                ("Id", id),
                ("PartyIdFrom", from.into()),
                ("PartyIdTo", to.into()),
                ("Relationship", what.into()),
                ("FromDate", fm_date(at.date())),
                ("CurrentInd", "1".into()),
                ("BillingInd", fm_bool(billing)),
            ])
        };
        let mut forward = pair(id, from, to, what, billing);
        let mut reciprocal = pair(back, to, from, "Employer", false);
        self.stamp("PartyRelation", &mut forward, at);
        for (k, v) in forward.iter() {
            if k.starts_with("Creation") || k.starts_with("Modification") || matches!(*k, "CreatedBy" | "ModifiedBy") {
                reciprocal.insert(k, v.clone());
            }
        }
        self.x.push("PartyRelation", forward);
        self.x.push("PartyRelation", reciprocal);
    }

    fn street(&mut self) -> [String; 4] {
        let (town, st, zip) = *self.r.pick(TOWNS);
        let n = self.r.range(2, 480);
        let s = self.r.pick(STREETS);
        [format!("{n} {s}"), town.into(), st.into(), zip.into()]
    }

    fn phone_number(&mut self) -> String {
        let area = if self.r.chance(0.9) { 207 } else { *self.r.pick(&[603, 617, 978]) };
        format!("({area}) 555-{:04}", self.r.range(200, 9999))
    }

    fn owner_and_staff(&mut self) {
        let t0 = NaiveDate::from_ymd_opt(2019, 4, 1).unwrap().and_hms_opt(9, 0, 0).unwrap();
        self.organization(ANCHOR_OWNER_ORG, "Ellison Studio", "Interiors & wayfinding", "82-5501920".into(), t0);
        self.phone(ANCHOR_OWNER_ORG, "Office", "(207) 555-0100", false, t0);
        self.network(ANCHOR_OWNER_ORG, "Office", "billing@ellisonstudio.example", false, &[1], t0);
        let a = ["14 Fore Street".into(), "Portland".into(), "ME".into(), "04101".into()];
        self.address(ANCHOR_OWNER_ORG, "Office", &a, &[1, 3], t0);
        let staff = [
            (ANCHOR_OWNER_PERSON, "r-mara-emp", "Mara", "Ellison", "Owner", "ME", Some("u-mara")),
            ("p-devon", "r-devon-emp", "Devon", "Park", "Senior designer", "DP", Some("u-devon")),
            ("p-priya", "r-priya-emp", "Priya", "Nair", "Project manager", "PN", Some("u-priya")),
            ("p-sam", "r-sam-emp", "Sam", "Whitaker", "Drafter", "SW", None),
            ("p-lucia", "r-lucia-emp", "Lucía", "Ortega", "Designer", "LO", None),
            ("p-theo", "r-theo-emp", "Theo", "Grant", "Bookkeeper", "TG", None),
        ];
        for (i, (pid, rid, first, last, job, initials, user)) in staff.iter().enumerate() {
            let since = t0 + Duration::days(i as i64 * 190);
            self.person(pid, first, last, job, since);
            let num = if i == 0 { "(207) 555-0101".to_string() } else { self.phone_number() };
            self.phone(pid, "Mobile", &num, true, since);
            let mail = format!("{}@ellisonstudio.example", first.to_lowercase().replace('í', "i"));
            self.network(pid, "Work", &mail, false, &[], since);
            self.relation(pid, ANCHOR_OWNER_ORG, if i == 0 { "Owner" } else { "Employee" }, false, since);
            self.role_with_id(rid, pid, 1, since.date(), None, since);
            let rate = [0, 14500, 12500, 8500, 11000, 7500][i];
            let ssn = format!("900-00-{:04}", i + 1); // not imported (D-09, BR-IMPORT-02)
            let r = row(&[
                ("Id", rid.to_string()),
                ("NameCached", format!("{first} {last}")),
                ("Initials", initials.to_string()),
                ("Type", job.to_string()),
                ("EmployeeNumber", (101 + i).to_string()),
                ("DefaultHourlyRate", if rate > 0 { fm_money(rate) } else { String::new() }),
                ("EstimatedHourlyCost", if rate > 0 { fm_money(rate * 45 / 100) } else { String::new() }),
                ("PayPeriod", "Biweekly".into()),
                ("BillableTime", fm_bool(i < 5)),
                ("SocialSecurityNumber", ssn),
            ]);
            self.push("Employee", r, since);
            self.employees.push((pid.to_string(), rid.to_string(), format!("{first} {last}")));
            if let Some(uid) = user {
                let r = row(&[
                    ("Id", uid.to_string()),
                    ("AccountName", uid[2..].to_string()),
                    ("PartyId", pid.to_string()),
                    ("EmailAddress", mail.clone()),
                    ("SmtpEmailAddress", mail.clone()),
                    ("SmtpFullName", format!("{first} {last}")),
                    ("SmtpServer", "smtp.ellisonstudio.example".into()),
                    ("SmtpServerPort", "587".into()),
                    ("SmtpServerAuthentication", "TLS".into()),
                    ("EmailSignature", fm_text(&format!("{first} {last}\n{job}, Ellison Studio\n(207) 555-0100"))),
                ]);
                self.push("User", r, since);
            }
        }
        let sys = row(&[
            ("OwnerId", ANCHOR_OWNER_ORG.into()),
            ("AutomaticallyFormatPhoneNumbersInd", "1".into()),
            ("CountryId", "1".into()),
            ("CountryCode", "1".into()),
            ("AreaCode", "207".into()),
            ("SystemEmailAddress", "billing@ellisonstudio.example".into()),
            ("SystemEmailFullName", "Ellison Studio".into()),
            ("SystemEmailUserName", "billing@ellisonstudio.example".into()),
            ("SystemEmailAccountPassword", "synthetic-secret-not-imported".into()), // D-09
            ("SmartyStreetsAuthToken", "synthetic-token-not-imported".into()),      // D-09
            ("TwilioAuthToken", "synthetic-twilio-not-imported".into()),            // OD-05-02
            ("SystemEmailSmtpServer", "smtp.ellisonstudio.example".into()),
            ("SystemEmailSmtpServerPort", "587".into()),
            ("SystemEmailSmtpServerEncryptionType", "None".into()),
            ("SystemEmailSmtpServerAuthentication", "TLS".into()),
            ("SystemEmailSender", "FileMakar SMTP".into()),
            ("ServerType", "Production".into()),
            ("AuthenticationMethod", "FileMaker Accounts".into()),
            ("InitialSetupDoneInd", "1".into()),
            ("BookCloseDate", "12/31/2025".into()), // D-20: not migrated
            ("UsingQBOInd", "0".into()),            // D-02: not migrated
        ]);
        self.push("System", sys, t0);
    }

    fn org_name(&mut self, used: &mut std::collections::BTreeSet<String>) -> (String, &'static str, &'static str) {
        loop {
            let (kind, industry, slug) = *self.r.pick(KIND);
            let place = self.r.pick(PLACE);
            let name = format!("{place} {kind}");
            if used.insert(name.clone()) {
                let domain = format!("{}{}", place.to_lowercase().replace([' ', '\''], ""), slug);
                return (name, industry, Box::leak(domain.into_boxed_str()));
            }
        }
    }

    /// A date of first contact: spread over the period, more clients early (the studio's book of business).
    fn since(&mut self) -> NaiveDate {
        let span = (end() - start()).num_days() - 60;
        let x = self.r.unit().powf(1.6);
        start() - Duration::days(120) + Duration::days((x * span as f64) as i64)
    }

    fn contact(&mut self, org: &str, domain: &str, billing: bool, at: NaiveDateTime) -> String {
        let id = self.r.uuid();
        let first = *self.r.pick(FIRST);
        let last = *self.r.pick(LAST);
        let job = *self.r.pick(JOBS);
        self.person_at(&id, first, last, job, org, at);
        let mail =
            format!("{}.{}@{domain}.example", first.to_lowercase(), last.to_lowercase().replace(['\'', 'ü'], ""));
        self.network(&id, "Work", &mail, false, &[], at);
        if self.r.chance(0.7) {
            let n = self.phone_number();
            let sms = self.r.chance(0.4);
            self.phone(&id, "Mobile", &n, sms, at);
        }
        self.relation(&id, org, job, billing, at);
        id
    }

    fn clients(&mut self) {
        let n = ((150.0 * self.scale).round() as usize).max(4);
        let mut used = std::collections::BTreeSet::from(["Halden & Pryce Architects".to_string()]);
        for i in 0..n {
            let (name, industry, domain) = self.org_name(&mut used);
            let since = self.since();
            let at = self.at(since);
            let id = self.r.uuid();
            let tax = if self.r.chance(0.6) {
                format!("{:02}-{:07}", self.r.range(10, 99), self.r.range(1_000_000, 9_999_999))
            } else {
                String::new()
            };
            self.organization(&id, &name, industry, tax, at);
            let bill = self.street();
            self.address(&id, "Office", &bill, &[1, 2], at);
            let n = self.phone_number();
            self.phone(&id, "Office", &n, false, at);
            let email = format!("accounts@{domain}.example");
            self.network(&id, "Accounts", &email, false, &[1], at);
            if self.r.chance(0.5) {
                self.network(&id, "Website", &format!("https://{domain}.example"), true, &[], at);
            }
            let mut contact = None;
            for k in 0..(if self.r.chance(0.7) { 1 } else { 2 }) {
                let c = self.contact(&id, domain, k == 0, at);
                contact.get_or_insert(c);
            }
            // About 8% of clients stopped buying; their role has a thru date.
            let until = (self.r.chance(0.08) && since < end() - Duration::days(400))
                .then(|| since + Duration::days(self.r.range(200, (end() - since).num_days() - 100)));
            let role = self.role(&id, 2, since, until, at);
            let weight = 0.25 + 9.0 * self.r.unit().powi(3) + if i < 12 { 4.0 } else { 0.0 };
            let terms = *self.r.pick(&[15, 30, 30, 30]);
            let taxable = self.r.chance(0.85);
            self.customer_row(&role, &name, terms, taxable, at);
            self.customers.push(Customer {
                role,
                party: id,
                name,
                weight,
                since,
                until,
                terms,
                taxable,
                bill,
                email,
                contact,
                retainer: 0,
            });
        }
    }

    fn customer_row(&mut self, role: &str, name: &str, terms: i64, taxable: bool, at: NaiveDateTime) {
        let r = row(&[
            ("Id", role.into()),
            ("DisplayName", name.into()),
            ("PrintOnCheckName", name.into()),
            ("Taxable", fm_bool(taxable)),
            ("SalesTermRef", format!("Net {terms}")),
            ("PreferredDeliveryMethod", "Email".into()),
            ("RetainerAdjustment", "0".into()),
            ("CurrencyRef", "USD".into()),
        ]);
        self.push("Customer", r, at);
    }

    fn individuals(&mut self) {
        let n = ((40.0 * self.scale).round() as usize).max(2);
        for _ in 0..n {
            let since = self.since();
            let at = self.at(since);
            let id = self.r.uuid();
            let (first, last) = (*self.r.pick(FIRST), *self.r.pick(LAST));
            let name = format!("{first} {last}");
            self.person(&id, first, last, "", at);
            let bill = self.street();
            self.address(&id, "Home", &bill, &[1], at);
            let email = format!("{}{}@mail.example", first.to_lowercase(), self.r.range(1, 99));
            self.network(&id, "Personal", &email, false, &[1], at);
            let n = self.phone_number();
            self.phone(&id, "Mobile", &n, true, at);
            let role = self.role(&id, 2, since, None, at);
            self.customer_row(&role, &name, 15, true, at);
            let weight = 0.15 + self.r.unit() * 0.6;
            self.customers.push(Customer {
                role,
                party: id,
                name,
                weight,
                since,
                until: None,
                terms: 15,
                taxable: true,
                bill,
                email,
                contact: None,
                retainer: 0,
            });
        }
    }

    fn leads(&mut self) {
        let n = ((30.0 * self.scale).round() as usize).max(2);
        let mut used: std::collections::BTreeSet<String> = self.customers.iter().map(|c| c.name.clone()).collect();
        for _ in 0..n {
            let (name, industry, domain) = self.org_name(&mut used);
            let since = end() - Duration::days(self.r.range(5, 420));
            let at = self.at(since);
            let id = self.r.uuid();
            self.organization(&id, &name, industry, String::new(), at);
            let n = self.phone_number();
            self.phone(&id, "Office", &n, false, at);
            self.contact(&id, domain, false, at);
            let role = self.role(&id, 4, since, None, at);
            let source = *self.r.pick(&["Referral", "Website", "Trade show", "Instagram", "Walk-in"]);
            let status = *self.r.pick(&["New", "Contacted", "Qualified", "Nurturing"]);
            let r = row(&[
                ("Id", role.clone()),
                ("NameCached", name.clone()),
                ("Source", source.into()),
                ("Status", status.into()),
                ("Notes", fm_text(&format!("Met via {source}.\nInterested in wayfinding and signage."))),
            ]);
            self.push("Lead", r, at);
            self.leads.push((id, role, name));
        }
    }

    fn vendors(&mut self) {
        let names = [
            ("Northeast Sign Supply", "Signage materials"),
            ("Casco Print & Imaging", "Large-format printing"),
            ("Pine Tree Millwork", "Millwork"),
            ("Bay State Lighting", "Lighting"),
            ("Granite Fabrication Co.", "Stone"),
            ("Harbor Freight Movers", "Logistics"),
            ("Old Port Textiles", "Textiles"),
            ("Maine Woodworks", "Furniture"),
            ("Coastal Vinyl Graphics", "Vinyl graphics"),
            ("Acadia Metalworks", "Metal fabrication"),
            ("Sebago Paint Supply", "Paint"),
            ("Kennebec Glass", "Glass"),
            ("Back Cove Hardware", "Hardware"),
            ("Penobscot Photo Lab", "Photography"),
            ("Saco River Plants", "Plants"),
            ("Two Lights Electric", "Electrical"),
            ("Ledgewood Carpentry", "Carpentry"),
            ("Seaglass Rentals", "Equipment rental"),
            ("Driftwood Framing", "Framing"),
            ("Highland Courier", "Courier"),
        ];
        let n = ((20.0 * self.scale).round() as usize).clamp(2, names.len());
        for (name, kind) in names.iter().take(n) {
            let since = self.since();
            let at = self.at(since);
            let id = self.r.uuid();
            self.organization(&id, name, kind, String::new(), at);
            let a = self.street();
            self.address(&id, "Office", &a, &[3], at);
            let p = self.phone_number();
            self.phone(&id, "Office", &p, false, at);
            let role = self.role(&id, 3, since, None, at);
            let limit = match self.r.range(0, 3) {
                0 => String::new(),
                1 => "5000".into(),
                2 => "2500.00".into(),
                _ => "10000".into(),
            };
            let r = row(&[
                ("Id", role),
                ("NameCached", name.to_string()),
                ("TermsId", "Net 30".into()),
                ("CreditLimit", limit),
                ("Eligible_1099_Ind", fm_bool(self.r.chance(0.3))),
                ("CriticalSupplierInd", fm_bool(self.r.chance(0.2))),
            ]);
            self.push("Vendor", r, at);
        }
    }

    fn items(&mut self) {
        // The demo seed's six items, with their ids, then the rest of the catalogue.
        let list: &[(&str, &'static str, &str, i64, bool, bool, &str)] = &[
            ("i-svc-workshop", "Discovery workshop", "SVC-WORKSHOP", 120000, false, false, "Service"),
            ("i-svc-survey", "Site survey & measured drawings", "SVC-SURVEY", 45000, false, false, "Service"),
            ("i-svc-hourly", "Design revision hours", "SVC-HOURLY", 9500, false, true, "Service"),
            ("i-prd-print-a1", "Large-format prints (A1)", "PRD-PRINT-A1", 3250, true, false, "NonInventory"),
            ("i-svc-design", "Design services", "SVC-DESIGN", 0, false, false, "Service"),
            ("i-retainer", "Retainer", "RETAINER", 0, false, false, "Service"),
            ("", "Project management hours", "SVC-PM", 8500, false, true, "Service"),
            ("", "Wayfinding concept package", "SVC-WAYFIND", 280000, false, false, "Service"),
            ("", "Signage design (per sign type)", "SVC-SIGNTYPE", 65000, false, false, "Service"),
            ("", "Installation supervision", "SVC-INSTALL", 11000, false, true, "Service"),
            ("", "3D rendering", "SVC-RENDER", 65000, false, false, "Service"),
            ("", "Material sample board", "PRD-SAMPLES", 18500, true, false, "NonInventory"),
            ("", "Acrylic room sign", "PRD-ACRYLIC", 8900, true, false, "Inventory"),
            ("", "Brushed aluminium plaque", "PRD-PLAQUE", 24500, true, false, "Inventory"),
            ("", "Vinyl window graphics (sq ft)", "PRD-VINYL", 1450, true, false, "NonInventory"),
            ("", "Furniture procurement fee", "SVC-PROCURE", 0, false, false, "Service"),
            ("", "Lighting plan", "SVC-LIGHTING", 145000, false, false, "Service"),
            ("", "Colour consultation", "SVC-COLOUR", 35000, false, false, "Service"),
            ("", "Mileage", "EXP-MILEAGE", 67, false, false, "Service"),
            ("", "Courier and delivery", "EXP-COURIER", 4500, true, false, "Service"),
            ("", "Framed art print", "PRD-ART", 32000, true, false, "Inventory"),
            ("", "Mural design", "SVC-MURAL", 190000, false, false, "Service"),
            ("", "ADA signage audit", "SVC-ADA", 75000, false, false, "Service"),
            ("", "Annual support plan", "SVC-SUPPORT", 60000, false, false, "Service"),
            ("", "Legacy item (discontinued)", "OLD-ITEM", 5000, false, false, "Service"),
        ];
        for (id, name, sku, price, taxable, hourly, kind) in list {
            let id = if id.is_empty() { self.r.uuid() } else { id.to_string() };
            let active = *sku != "OLD-ITEM";
            let r = row(&[
                ("id", id.clone()),
                ("name", name.to_string()),
                ("description", name.to_string()),
                ("sku", sku.to_string()),
                ("fullyQualifiedName", name.to_string()),
                ("unitPrice", fm_money(*price)),
                ("taxable", fm_bool(*taxable)),
                ("active", fm_bool(active)),
                ("itemType", kind.to_string()),
                ("trackQtyOnHand", fm_bool(*kind == "Inventory")),
                ("qtyOnHand", if *kind == "Inventory" { self.r.range(0, 60).to_string() } else { String::new() }),
                ("qbo_id", self.r.range(1, 400).to_string()), // D-02: not migrated
            ]);
            self.x.push("Item", r);
            self.items.push(Item { id, name, price: *price, taxable: *taxable, hourly: *hourly });
        }
    }

    fn item(&self, sku_name: &str) -> Item {
        self.items.iter().find(|i| i.name == sku_name).unwrap().clone()
    }

    /// Halden & Pryce and its five demo transactions: INV-1048 keeps its lines, tax, retainer and payment.
    fn halden_pryce(&mut self) {
        let since = NaiveDate::from_ymd_opt(2021, 3, 4).unwrap();
        let at = since.and_hms_opt(10, 0, 0).unwrap();
        self.organization(ANCHOR_HP, "Halden & Pryce Architects", "Architecture & interiors", "84-2214417".into(), at);
        self.phone(ANCHOR_HP, "Office", "(207) 555-0142", false, at);
        self.network(ANCHOR_HP, "Office", "studio@haldenpryce.example", false, &[1], at);
        self.network(ANCHOR_HP, "Website", "https://haldenpryce.example", true, &[], at);
        let bill: [String; 4] = ["220 Commercial Street".into(), "Portland".into(), "ME".into(), "04101".into()];
        self.address(ANCHOR_HP, "Office", &bill, &[1, 2], at);
        for (pid, first, last, what, billing) in [
            ("p-hp-eh", "Elena", "Halden", "Principal", false),
            ("p-hp-mp", "Marcus", "Pryce", "Partner", false),
            ("p-hp-jw", "June", "Whitlock", "Office manager", true),
        ] {
            self.person_at(pid, first, last, what, ANCHOR_HP, at);
            self.network(pid, "Work", &format!("{}@haldenpryce.example", first.to_lowercase()), false, &[], at);
            self.relation(pid, ANCHOR_HP, what, billing, at);
        }
        self.role_with_id(ANCHOR_HP_ROLE, ANCHOR_HP, 2, since, None, at);
        self.customer_row(ANCHOR_HP_ROLE, "Halden & Pryce Architects", 15, true, at);
        let c = Customer {
            role: ANCHOR_HP_ROLE.into(),
            party: ANCHOR_HP.into(),
            name: "Halden & Pryce Architects".into(),
            weight: 3.0,
            since,
            until: None,
            terms: 15,
            taxable: true,
            bill,
            email: "studio@haldenpryce.example".into(),
            contact: Some("p-hp-eh".into()),
            retainer: 0,
        };
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        let design = self.item("Design services");
        let retainer = self.item("Retainer");
        let (workshop, survey, hourly, prints) = (
            self.item("Discovery workshop"),
            self.item("Site survey & measured drawings"),
            self.item("Design revision hours"),
            self.item("Large-format prints (A1)"),
        );
        let t = |id: &str, doc: &str, date, invoice: bool, lines: Vec<Line>, memo: &str| Txn {
            id: id.into(),
            doc: doc.into(),
            date,
            invoice,
            lines,
            ship: 0,
            memo: memo.into(),
            locked: true,
        };
        let line = |id: &str, item: &Item, desc: &str, qty: i64, price: i64| Line {
            id: id.into(),
            item: item.id.clone(),
            desc: desc.into(),
            qty,
            price,
            taxable: item.taxable,
            amount_text: None,
            price_7dp: None,
        };
        let txns = [
            (
                t(
                    "t-inv-1027",
                    "INV-1027",
                    d(7, 18),
                    true,
                    vec![line("l-001", &design, "Brand refresh: design", 10000, 418000)],
                    "",
                ),
                vec![("pay-001", "pl-001", d(7, 30), 418000, "Check #2041")],
            ),
            (
                t(
                    "t-ret-0042",
                    "RET-0042",
                    d(8, 1),
                    false,
                    vec![line("l-002", &retainer, "Retainer deposit", 10000, 300000)],
                    "",
                ),
                vec![],
            ),
            (
                t(
                    "t-inv-1035",
                    "INV-1035",
                    d(8, 29),
                    true,
                    vec![
                        line("l-003", &design, "Office fit-out, phase 1: design", 10000, 520000),
                        line("l-004", &retainer, "Retainer applied (RET-0042)", 10000, -100000),
                    ],
                    "",
                ),
                vec![("pay-002", "pl-002", d(9, 10), 420000, "Check #2107")],
            ),
            (
                t(
                    "t-inv-1042",
                    "INV-1042",
                    d(9, 30),
                    true,
                    vec![line("l-005", &design, "Office fit-out, phase 1: documentation", 10000, 280000)],
                    "",
                ),
                vec![("pay-003", "pl-003", d(10, 10), 180000, "Check #2188")],
            ),
            (
                t(
                    ANCHOR_INVOICE,
                    "INV-1048",
                    d(10, 14),
                    true,
                    vec![
                        line("l-007", &workshop, "Discovery workshop", 20000, 120000),
                        line("l-008", &survey, "Site survey & measured drawings", 10000, 45000),
                        line("l-009", &hourly, "Design revision hours", 60000, 9500),
                        line("l-010", &prints, "Large-format prints (A1)", 40000, 3250),
                        line("l-011", &retainer, "Retainer applied (RET-0042)", 10000, -50000),
                    ],
                    "Thank you for your business.",
                ),
                vec![("pay-004", "pl-004", d(10, 20), 100000, "Check #2214")],
            ),
        ];
        for (txn, pays) in txns {
            let paid = pays.iter().map(|p| p.3).sum();
            self.write_txn(&c, &txn, paid);
            for (pid, plid, date, amount, rf) in pays {
                self.payment(pid, &c, date, amount, "Check", rf, &[(plid.to_string(), txn.id.clone(), amount)]);
            }
        }
        self.customers.push(c);
    }

    /// The total of a transaction as FileMaker computed it (2-decimal line amounts).
    fn txn_total(c: &Customer, t: &Txn) -> i64 {
        let mut subtotal = 0;
        let mut taxable = 0;
        for l in &t.lines {
            let amount = div_round(l.qty as i128 * l.price as i128, 10_000) as i64;
            subtotal += amount;
            if l.taxable && c.taxable {
                taxable += amount;
            }
        }
        subtotal + div_round(taxable as i128 * TAX_RATE_BP as i128, 10_000) as i64 + t.ship
    }

    /// Writes a transaction, its subtype row and its lines with the totals FileMaker stored; `paid` is the sum of
    /// its payments, so the exported balance is the one FileMaker had.
    fn write_txn(&mut self, c: &Customer, t: &Txn, paid: i64) {
        let mut subtotal = 0;
        let mut taxable = 0;
        for l in &t.lines {
            let amount = div_round(l.qty as i128 * l.price as i128, 10_000) as i64;
            subtotal += amount;
            if l.taxable && c.taxable {
                taxable += amount;
            }
        }
        let tax = div_round(taxable as i128 * TAX_RATE_BP as i128, 10_000) as i64;
        let total = subtotal + tax + t.ship;
        let due = t.invoice.then(|| t.date + Duration::days(c.terms));
        let at = self.at(t.date);
        let r = row(&[
            ("id", t.id.clone()),
            ("docNumber", t.doc.clone()),
            ("date", fm_date(t.date)),
            ("dueDate", due.map(fm_date).unwrap_or_default()),
            ("id_Customer", c.role.clone()),
            ("billAddressLine1", c.bill[0].clone()),
            ("billAddressCity", c.bill[1].clone()),
            ("billAddressState", c.bill[2].clone()),
            ("billAddressPostalCode", c.bill[3].clone()),
            ("billAddressCountry", "USA".into()),
            ("billEmailAddress", c.email.clone()),
            ("subtotal", fm_money(subtotal)),
            ("taxableSubtotal", fm_money(taxable)),
            ("totalTax", fm_money(tax)),
            ("shipAmt", if t.ship > 0 { fm_money(t.ship) } else { String::new() }),
            ("totalAmt", fm_money(total)),
            // The balance FileMaker had; the import recomputes it from the payments (BR-SALES-02).
            ("balance", fm_money(if t.invoice { total - paid } else { 0 })),
            (
                "status",
                if !t.invoice || total == paid {
                    "Paid".into()
                } else if paid > 0 {
                    "Partially paid".into()
                } else {
                    "Open".into()
                },
            ),
            ("customerMemo", t.memo.clone()),
            ("InvoiceInd", fm_bool(t.invoice)),
            ("SalesReceiptInd", fm_bool(!t.invoice)),
            ("LockedInd", fm_bool(t.locked)),
            ("deleted", "0".into()),
            ("qbo_send", "1".into()), // D-02: not migrated
        ]);
        self.push("SalesTransaction", r, at);
        if t.invoice {
            let r = row(&[("Id", t.id.clone()), ("id_Term", format!("Net {}", c.terms))]);
            self.push("Invoice", r, at);
        } else {
            let pay = format!("{} {}", self.r.pick(&["Card", "Bank transfer", "Check"]), self.r.range(1000, 9999));
            self.x.push("SalesReceipt", row(&[("id", t.id.clone()), ("paymentRefNum", pay)]));
        }
        for l in &t.lines {
            let amount = div_round(l.qty as i128 * l.price as i128, 10_000) as i64;
            let r = row(&[
                ("id", l.id.clone()),
                ("SalesTransactionId", t.id.clone()),
                ("ItemId", l.item.clone()),
                ("description", fm_text(&l.desc)),
                ("qty", fm_qty(l.qty)),
                ("unitPrice", l.price_text()),
                ("amount", l.amount_text.clone().unwrap_or_else(|| fm_money(amount))),
                ("taxable", fm_bool(l.taxable && c.taxable)),
            ]);
            self.x.push("SalesLine", r);
        }
        self.txns.push((t.id.clone(), c.party.clone(), t.date, t.doc.clone()));
    }

    #[allow(clippy::too_many_arguments)]
    fn payment(
        &mut self,
        id: &str,
        c: &Customer,
        date: NaiveDate,
        amount: i64,
        method: &str,
        rf: &str,
        lines: &[(String, String, i64)],
    ) {
        let r = row(&[
            ("id", id.into()),
            ("id_Customer", c.role.clone()),
            ("date", fm_date(date)),
            ("amount", fm_money(amount)),
            ("id_PaymentMethod", method.into()),
            ("PaymentRefNum", rf.into()),
            ("id_DepositAccount", "Checking".into()),
        ]);
        self.x.push("Payment", r);
        for (lid, txn, a) in lines {
            let r = row(&[
                ("id", lid.clone()),
                ("id_Payment", id.into()),
                ("id_Invoice", txn.clone()),
                ("id_Customer", c.role.clone()),
                ("amount", fm_money(*a)),
            ]);
            self.x.push("PaymentLine", r);
        }
    }

    /// 46 months of invoices and sales receipts: spring and autumn peaks, a quiet December, growth over time.
    fn sales(&mut self) {
        const SEASON: [f64; 12] = [0.7, 0.8, 1.05, 1.2, 1.25, 1.1, 0.85, 0.8, 1.15, 1.25, 1.0, 0.65];
        let months = (end().year() - start().year()) * 12 + end().month() as i32 - start().month() as i32 + 1;
        let per_month = 4400.0 / months as f64 * self.scale;
        let mut inv_no = 10001;
        let mut sr_no = 10001;
        let mut ret_no = 1001;
        let mut pay_no = 0u32;
        for m in 0..months {
            let first = NaiveDate::from_ymd_opt(start().year() + (m / 12), (m % 12) as u32 + 1, 1).unwrap();
            let days = days_in_month(first);
            let growth = 0.8 + 0.4 * (m as f64 / months as f64);
            let n = (per_month * SEASON[first.month0() as usize] * growth * (0.9 + 0.2 * self.r.unit())).round() as i64;
            for _ in 0..n {
                let date = first + Duration::days(self.r.range(0, days - 1));
                if date > end() || date.weekday().number_from_monday() > 6 {
                    continue;
                }
                let Some(ci) = self.pick_customer(date) else { continue };
                let mut c = self.customers[ci].clone();
                let is_hp = c.party == ANCHOR_HP;
                let receipt = !is_hp && self.r.chance(0.1);
                let retainer_deposit = !is_hp && !receipt && self.r.chance(0.008);
                let id = self.r.uuid();
                let mut lines = Vec::new();
                let (doc, invoice) = if retainer_deposit {
                    ret_no += 1;
                    let amount = *self.r.pick(&[100000, 200000, 250000, 500000]);
                    lines.push(self.line(&self.item("Retainer"), "Retainer deposit", 10000, amount));
                    c.retainer += amount;
                    (format!("RET-{ret_no}"), false)
                } else if receipt {
                    sr_no += 1;
                    for _ in 0..self.r.range(1, 3) {
                        lines.push(self.random_line(true, false));
                    }
                    (format!("SR-{sr_no}"), false)
                } else {
                    inv_no += 1;
                    for _ in 0..self.r.range(1, 6) {
                        lines.push(self.random_line(false, !is_hp));
                    }
                    (format!("INV-{inv_no}"), true)
                };
                if invoice && c.retainer > 0 && self.r.chance(0.5) {
                    let sub: i64 =
                        lines.iter().map(|l| div_round(l.qty as i128 * l.price as i128, 10_000) as i64).sum();
                    let apply = c.retainer.min(sub / 2).max(0);
                    if apply > 0 {
                        lines.push(self.line(&self.item("Retainer"), "Retainer applied", 10000, -apply));
                        c.retainer -= apply;
                    }
                }
                for l in lines.iter_mut() {
                    l.id = self.r.uuid();
                }
                let ship = if !invoice || !self.r.chance(0.06) { 0 } else { *self.r.pick(&[1500, 2500, 4500]) };
                // In the last 6 days one transaction in four is still a draft (by its number: no PRNG draw, so the rows stay the
                // same). The rest are posted, so the demo's newest revenue reaches its last day (B11).
                let number: i64 = doc.rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let draft = date >= end() - Duration::days(6) && number % 4 == 0;
                let held = is_hp && date > end() - Duration::days(120);
                let locked = !draft && !held;
                // Payments keep their old cut-off, so the PRNG stream and the export's row counts do not change.
                let payable = date < end() - Duration::days(6) && !held;
                let txn = Txn { id: id.clone(), doc, date, invoice, lines, ship, memo: String::new(), locked };
                let total = Self::txn_total(&c, &txn);
                let paid = if invoice && payable { self.pay(&c, &txn, total, is_hp, &mut pay_no) } else { 0 };
                self.write_txn(&c, &txn, paid);
                self.customers[ci] = c;
            }
        }
    }

    fn pick_customer(&mut self, date: NaiveDate) -> Option<usize> {
        let w: Vec<f64> = self
            .customers
            .iter()
            .map(|c| {
                let active = c.since <= date && c.until.is_none_or(|u| u > date);
                // Halden & Pryce's 2026 transactions are the demo's; generated ones stop before July 2026.
                let hp_ok = c.party != ANCHOR_HP || date < NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
                if active && hp_ok {
                    c.weight
                } else {
                    0.0
                }
            })
            .collect();
        (w.iter().sum::<f64>() > 0.0).then(|| self.r.weighted(&w))
    }

    fn line(&self, item: &Item, desc: &str, qty: i64, price: i64) -> Line {
        Line {
            id: String::new(),
            item: item.id.clone(),
            desc: desc.into(),
            qty,
            price,
            taxable: item.taxable,
            amount_text: None,
            price_7dp: None,
        }
    }

    /// A line from the catalogue. `fee`: may be a typed fixed fee with a 7-decimal unit price (not on the anchor
    /// customer, whose balances must stay those of the demo seed).
    fn random_line(&mut self, receipt: bool, fee: bool) -> Line {
        let usable: Vec<Item> =
            self.items.iter().filter(|i| i.name != "Retainer" && (!receipt || i.price < 50000)).cloned().collect();
        let it = self.r.pick(&usable).clone();
        let (qty, price) = if it.hourly {
            (self.r.range(2, 48) * 2500, it.price)
        } else if it.name == "Mileage" {
            (self.r.range(20, 260) * 10000, it.price)
        } else if it.name.contains("sq ft") {
            (self.r.range(10, 140) * 10000, it.price)
        } else if it.price == 0 {
            (10000, self.r.range(8, 160) * 5000)
        } else if it.taxable {
            (self.r.range(1, 12) * 10000, it.price)
        } else {
            (10000, it.price)
        };
        let mut l = self.line(&it, it.name, qty, price);
        // FileMaker's line trigger back-computes the unit price to 7 decimals when the amount was typed (BR-SALES-01):
        // a fixed fee split over a quantity of 3. The import rounds the unit price to the cent and recomputes.
        if fee && self.r.chance(0.02) {
            let fee = self.r.range(3, 40) * 10000;
            l.qty = 30000;
            l.price = div_round(fee as i128, 3) as i64;
            l.amount_text = Some(fm_money(fee));
            l.price_7dp = Some(seven_dp(fee, 3));
            l.desc = format!("{} (fixed fee, 3 visits)", it.name);
        }
        l
    }

    /// Pays an invoice in one or two payments, or not (recent ones are often open). Returns the amount paid.
    fn pay(&mut self, c: &Customer, t: &Txn, total: i64, always: bool, pay_no: &mut u32) -> i64 {
        if total <= 0 {
            return 0;
        }
        let age = (end() - t.date).num_days();
        let p_paid = if always {
            1.0
        } else if age > 120 {
            0.985
        } else if age > 60 {
            0.85
        } else if age > 30 {
            0.55
        } else {
            0.25
        };
        if self.r.chance(p_paid) {
            let paid = self.pay_parts(c, t, total, always, age, false, pay_no);
            if paid > 0 {
                return paid;
            }
        }
        // Most of the rest come in late, or the demo reads as a business nobody pays: overdue was 57% of balance due
        // (owner, 2026-11-06: 10 to 20%). The draws are `late`'s, so every other row of the export stays the same.
        let p_late = if age > 60 {
            0.97
        } else if age > 30 {
            0.9
        } else if age > 15 {
            0.8
        } else {
            0.0
        };
        if !self.late.chance(p_late) {
            return 0;
        }
        std::mem::swap(&mut self.r, &mut self.late);
        let paid = self.pay_parts(c, t, total, always, age, true, pay_no);
        std::mem::swap(&mut self.r, &mut self.late);
        paid
    }

    /// The payments of an invoice that gets paid; returns the amount. `late`: all of it, by the export's last day.
    #[allow(clippy::too_many_arguments)]
    fn pay_parts(
        &mut self,
        c: &Customer,
        t: &Txn,
        total: i64,
        always: bool,
        age: i64,
        late: bool,
        pay_no: &mut u32,
    ) -> i64 {
        let mut paid = 0;
        let methods = [("Check", 0.45), ("ACH", 0.35), ("Credit Card", 0.2)];
        let mi = self.r.weighted(&methods.iter().map(|m| m.1).collect::<Vec<_>>());
        let method = methods[mi].0;
        let split = !late && !always && total > 200000 && self.r.chance(0.2);
        let parts = if split { vec![total / 2, total - total / 2] } else { vec![total] };
        let mut when = t.date + Duration::days(self.r.range(3, c.terms + 20));
        if late {
            when = when.min(end());
        }
        for (k, amount) in parts.iter().enumerate() {
            if when > end() || (!always && k == 1 && age < 75) {
                break;
            }
            *pay_no += 1;
            let rf = match method {
                "Check" => format!("Check #{}", 1000 + *pay_no % 9000),
                "ACH" => format!("ACH {}", 700000 + *pay_no),
                _ => format!("Card ending {:04}", self.r.range(1000, 9999)),
            };
            let pid = self.r.uuid();
            let lid = self.r.uuid();
            self.payment(&pid, c, when, *amount, method, &rf, &[(lid, t.id.clone(), *amount)]);
            paid += amount;
            when += Duration::days(self.r.range(14, 40));
        }
        paid
    }

    fn opportunities(&mut self) {
        let n = ((320.0 * self.scale).round() as usize).max(4);
        let names = [
            "Lobby wayfinding",
            "Storefront signage",
            "Office fit-out",
            "Brand refresh",
            "Patient room signage",
            "Menu boards",
            "Donor wall",
            "Exhibit graphics",
            "Window graphics",
            "Lighting upgrade",
            "Reception redesign",
            "Annual support plan",
            "Mural",
            "Campus map",
            "ADA audit",
            "Tasting room interior",
        ];
        for i in 0..n {
            let lead = !self.leads.is_empty() && self.r.chance(0.2);
            let (party, lead_id, pname) = if lead {
                let (p, r, name) = self.r.pick(&self.leads).clone();
                (p, Some(r), name)
            } else {
                let c = self.r.pick(&self.customers).clone();
                (c.party, None, c.name)
            };
            let drawn_start = self.since().max(start());
            let age = (end() - drawn_start).num_days();
            let drawn_status = if age > 200 {
                *self.r.pick(&[4, 4, 4, 5, 5, 6, 7])
            } else if age > 60 {
                *self.r.pick(&[2, 3, 3, 4, 5, 8])
            } else {
                *self.r.pick(&[1, 1, 2, 3])
            };
            // One in eight (about 40 of 320) started in the last 180 days, so the desktop has a live pipeline. These
            // come from the index, not new draws, so every other row of the export stays as it was. The status still
            // follows the age rule above.
            let recent = i % 8 == 0;
            let start_d = if recent { end() - Duration::days((i as i64 * 7) % 180) } else { drawn_start };
            let status = if recent {
                let age = (end() - start_d).num_days();
                if age > 60 {
                    [2, 3, 3, 4, 5, 8][i / 8 % 6]
                } else {
                    [1, 1, 2, 3][i / 8 % 4]
                }
            } else {
                drawn_status
            };
            let active = matches!(status, 1 | 2 | 3 | 8);
            let (emp_party, emp_role, _) = self.r.pick(&self.employees[..3]).clone();
            let _ = emp_party;
            let drawn_prob = match drawn_status {
                4 => "definite",
                1 | 5 | 6 | 7 => "unlikely",
                _ => *self.r.pick(&["unlikely", "likely", "likely"]),
            };
            let prob = if !recent {
                drawn_prob
            } else {
                match status {
                    4 => "definite",
                    1 | 5 | 6 | 7 => "unlikely",
                    _ => ["unlikely", "likely", "likely"][i / 8 % 3],
                }
            };
            let status_name =
                ["New", "In process", "Proposal sent", "Won", "Lost", "Unqualified", "Closed (other)", "On Hold"]
                    [status - 1];
            let id = self.r.uuid();
            let at = self.at(start_d);
            let opp_name = format!("{} {}", self.r.pick(&names), if i % 7 == 0 { "(phase 2)" } else { "" });
            let opp_value = fm_money(self.r.range(4, 400) * 10000);
            let closed = start_d + Duration::days(self.r.range(20, 150));
            let r = row(&[
                ("Id", id),
                ("Name", opp_name.trim().to_string()),
                ("PartyId", party),
                ("PartyNameCached", pname),
                ("LeadId", lead_id.unwrap_or_default()),
                ("OpportunityStatusId", status.to_string()),
                ("StatusCached", status_name.into()),
                ("Probability", prob.into()),
                ("Value", opp_value),
                ("SalespersonId", emp_role),
                ("StartDate", fm_date(start_d)),
                // 608 s108: an active opportunity has no date closed. The draw stays, and a closed one is never future.
                ("EndDate", if active { String::new() } else { fm_date(closed.min(end())) }),
                ("ActiveInd", fm_bool(active)),
            ]);
            self.push("Opportunity", r, at);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn activity_party(&mut self, activity: &str, party: &str, name: &str, role: i64, at: NaiveDateTime) {
        let id = self.r.uuid();
        let r = row(&[
            ("Id", id),
            ("ActivityId", activity.into()),
            ("PartyId", party.into()),
            ("PartyNameCached", name.into()),
            ("ActivityPartyRoleId", role.to_string()),
        ]);
        self.push("ActivityParty", r, at);
    }

    fn activities(&mut self) {
        let n = ((3000.0 * self.scale).round() as usize).max(10);
        // type id, weight
        let kinds = [(2, 0.30), (1, 0.25), (3, 0.15), (8, 0.12), (6, 0.08), (10, 0.05), (9, 0.05)];
        let weights: Vec<f64> = kinds.iter().map(|k| k.1).collect();
        let subjects = [
            "Follow up on proposal",
            "Site visit notes",
            "Sign schedule review",
            "Budget check-in",
            "Install date",
            "Material samples",
            "Invoice question",
            "Kick-off",
            "Revisions to concept B",
            "Permit drawings",
            "Lighting options",
            "Colour palette",
            "Punch list",
            "Thank-you note",
            "Quote request",
        ];
        let mut chats = 0;
        for i in 0..n {
            let kind = kinds[self.r.weighted(&weights)].0;
            let c = self.r.pick(&self.customers).clone();
            let lo = c.since.max(start());
            if lo >= end() {
                continue;
            }
            let date = lo + Duration::days(self.r.range(0, (end() - lo).num_days()));
            let at = self.at(date);
            let id = self.r.uuid();
            let (emp_party, _, emp_name) = self.r.pick(&self.employees[..3]).clone();
            let subject = self.r.pick(&subjects).to_string();
            let flag = |k: i64| fm_bool(kind == k);
            let mut r = row(&[
                ("Id", id.clone()),
                ("ActivityTypeId", kind.to_string()),
                ("Subject", subject.clone()),
                ("DateDisplay", fm_date(date)),
                ("TimeDisplay", fm_time(at)),
                ("EmailMessageInd", flag(1)),
                ("NoteInd", flag(2)),
                ("TaskInd", flag(3)),
                ("EventInd", fm_bool(kind == 6 || kind == 4)),
                ("CallInd", flag(8)),
                ("ChatInd", flag(9)),
                ("DiscussionInd", flag(10)),
                ("CreatorCached", emp_name.clone()),
                ("PartiesCached", c.name.clone()),
                ("LastUpdated", fm_timestamp(at)),
            ]);
            if kind == 10 {
                r.insert("DiscussionAllowedFor", if self.r.chance(0.7) { "Our team only" } else { "Client" }.into());
            }
            self.push("Activity", r, at);
            self.activity_party(&id, &emp_party, &emp_name, 50, at);
            self.activity_party(&id, &c.party, &c.name, 63, at);
            let message = match kind {
                1 => Some(format!("Hi,\n\nFollowing up on {}.\n\nBest,\n{emp_name}", subject.to_lowercase())),
                2 => Some(format!("{subject}.\nDiscussed next steps with {}.", c.name)),
                9 => Some(format!("Quick update: {}", subject.to_lowercase())),
                _ => None,
            };
            if let Some(m) = message {
                let r = row(&[("ActivityId", id.clone()), ("Id", self.r.uuid()), ("Message", fm_text(&m))]);
                self.push("ActivityMessage", r, at);
            }
            match kind {
                1 => {
                    if let Some(contact) = &c.contact {
                        self.activity_party(&id, contact, "", 55, at);
                    }
                    let r = row(&[
                        ("Id", id.clone()),
                        ("SentInd", "1".into()),
                        ("SmtpTimestamp", fm_timestamp(at)),
                        ("SmtpResult", "250 OK".into()),
                        ("UseSystemSettingsInd", "1".into()),
                    ]);
                    self.x.push("EmailMessage", r);
                    // An email about an invoice links to it (entity type 12).
                    if let Some((tid, ..)) = self
                        .txns
                        .iter()
                        .find(|t| t.1 == c.party && t.2 <= date && (date - t.2).num_days() < 30)
                        .cloned()
                    {
                        let r = row(&[
                            ("Id", self.r.uuid()),
                            ("ActivityId", id.clone()),
                            ("EntityTypeId", "12".into()),
                            ("EntityId", tid),
                        ]);
                        self.push("ActivityEntity", r, at);
                    }
                }
                2 => self.x.push("Note", row(&[("Id", id.clone())])),
                3 => {
                    let due = date + Duration::days(self.r.range(1, 21));
                    let drawn = due < end() - Duration::days(3) && self.r.chance(0.85);
                    // An old task is done, or the dashboard's "Tasks due" opens on years-old work. No PRNG draw here, so
                    // the other rows stay the same.
                    let stale = due < end() - Duration::days(30);
                    let done = drawn || stale;
                    let completed = if drawn {
                        Some(self.at(due.min(end())))
                    } else {
                        stale.then(|| due.and_hms_opt(17, 0, 0).unwrap())
                    };
                    self.activity_party(&id, &emp_party, &emp_name, 53, at);
                    let r = row(&[
                        ("Id", id.clone()),
                        ("DueDate", fm_date(due)),
                        ("Priority", self.r.pick(&["Low", "Normal", "Normal", "High"]).to_string()),
                        ("CompletionTimestamp", completed.map(fm_timestamp).unwrap_or_default()),
                        ("CompletedInd", fm_bool(done)), // D-29: replaced by task.completed_at
                        ("CompletedByCached", if done { emp_name.clone() } else { String::new() }),
                        ("FromCached", emp_name.clone()),
                        ("ToCached", emp_name.clone()),
                    ]);
                    self.x.push("Task", r);
                }
                6 => {
                    let r = row(&[
                        ("Id", id.clone()),
                        ("StartDate", fm_date(date)),
                        ("StartTime", fm_time(at)),
                        ("EndDate", fm_date(date)),
                        ("EndTime", fm_time(at + Duration::minutes(60))),
                        ("Location", if self.r.chance(0.5) { "On site".into() } else { "Studio".into() }),
                        ("MeetingInd", "1".into()),
                    ]);
                    self.push("Event", r, at);
                    self.x.push("Meeting", row(&[("Id", id.clone())]));
                }
                8 => {
                    let caller = c.contact.clone().map(|_| c.name.clone()).unwrap_or_default();
                    let r = row(&[
                        ("Id", id.clone()),
                        ("CallerName", caller),
                        ("CallbackNumber", self.phone_number()),
                        ("Result", self.r.pick(&["Left message", "Spoke", "No answer"]).to_string()),
                    ]);
                    self.x.push("Call", r);
                }
                10 => {
                    let r = row(&[
                        ("Id", id.clone()),
                        ("Title", subject.clone()),
                        ("Date", fm_date(date)),
                        ("Time", fm_time(at)),
                        ("ActiveInd", fm_bool(date > end() - Duration::days(60))),
                    ]);
                    self.push("Discussion", r, at);
                    self.discussions.push((id.clone(), subject.clone()));
                    for k in 0..self.r.range(1, 4) {
                        let when = at + Duration::hours(k * 20 + 2);
                        let r = row(&[
                            ("Id", self.r.uuid()),
                            ("ActivityId", id.clone()),
                            ("CreatorId", emp_party.clone()),
                            ("Comment", format!("Comment {} on {}", k + 1, subject.to_lowercase())),
                            ("Date", fm_date(when.date())),
                            ("Time", fm_time(when)),
                        ]);
                        self.push("ActivityComment", r, when);
                    }
                }
                9 => {
                    // SMS (activity type 9): the activity imports; the chat rows are skipped (OD-05-02).
                    chats += 1;
                    let r = row(&[
                        ("Id", self.r.uuid()),
                        ("ActivityId", id.clone()),
                        ("Message", format!("Quick update: {}", subject.to_lowercase())),
                        ("Direction", "outbound".into()),
                        ("OutgoingInd", "1".into()),
                        ("StatusDeliveredInd", "1".into()),
                    ]);
                    self.push("ChatMessage", r, at);
                }
                _ => {}
            }
            let _ = i;
        }
        for k in 0..(chats / 10).max(1) {
            let at = self.at(end() - Duration::days(k as i64 * 9));
            let r = row(&[("Id", self.r.uuid()), ("PhoneList", self.phone_number())]);
            self.push("Chat", r, at);
        }
        for k in 0..3 {
            let at = self.at(end() - Duration::days(30 + k));
            let r = row(&[("Id", self.r.uuid()), ("Phone", self.phone_number())]);
            self.push("ChatConsent", r, at);
        }
    }

    /// Six templates with merge placeholders in the source's form, `<<Name>>` (kontroller 478 s3, MergeFieldConfig
    /// field 31), named from the app's fixed list (BR-TEMPLATE-01). The import stores them as `{{Name}}` (OD-07-03). A
    /// greeting uses RecipientName: an invoice's Email button addresses the customer, usually an organization, which has
    /// no first name, and the send stops on a field with no value.
    fn templates(&mut self) {
        let list = [
            (
                "Invoice email",
                "Invoice <<InvoiceNumber>> from Ellison Studio",
                "Hi <<RecipientName>>,\n\nPlease find invoice <<InvoiceNumber>> attached.\n\nThank you,\n<<SenderName>>",
                1,
            ),
            (
                "Payment reminder",
                "Reminder: invoice <<InvoiceNumber>>",
                "Hi <<RecipientName>>,\n\nA friendly reminder that invoice <<InvoiceNumber>> is due on <<DueDate>>.",
                1,
            ),
            ("Thank you", "Thank you", "Thank you for choosing Ellison Studio.", 1),
            ("Site visit follow-up", "Site visit <<Date>>", "Notes from our site visit on <<Date>>.", 2),
            (
                "Proposal cover",
                "Proposal for <<RecipientName>>",
                "Please find our proposal for <<RecipientName>> attached.",
                1,
            ),
            ("Email signature", "", "<<SenderName>>\nEllison Studio\n(207) 555-0100", 1),
        ];
        let t0 = NaiveDate::from_ymd_opt(2022, 1, 10).unwrap().and_hms_opt(9, 0, 0).unwrap();
        for (i, (name, subject, message, kind)) in list.iter().enumerate() {
            let id = (i + 1) as u32;
            let r = row(&[
                ("Id", id.to_string()),
                ("Name", name.to_string()),
                ("Subject", subject.to_string()),
                ("Message", fm_text(message)),
                ("ActivityTypeId", kind.to_string()),
                ("UserId", "u-mara".into()),
            ]);
            self.push("Template", r, t0 + Duration::days(i as i64 * 30));
            for field in ["<<RecipientName>>", "<<InvoiceNumber>>", "<<DueDate>>", "<<SenderName>>", "<<Date>>"] {
                if message.contains(field) || subject.contains(field) {
                    let n = self.next_serial("MergeField", 1);
                    // MergeField::Name keeps the delimiters, as MergeFieldConfig field 31 does.
                    let r = row(&[
                        ("Id", n.to_string()),
                        ("TemplateId", id.to_string()),
                        ("Name", field.to_string()),
                        ("RequiredInd", fm_bool(field == "<<InvoiceNumber>>")),
                    ]);
                    self.push("MergeField", r, t0);
                }
            }
        }
    }

    fn files(&mut self) {
        let n = ((350.0 * self.scale).round() as usize).max(3);
        // ContentType and Category as kontroller 617 s117 and s120 set them: jpg is "image/jpg", xlsx has no type,
        // and the category is Image or Document.
        let kinds = [
            ("Signed proposal", "pdf", "application/pdf", 0.35),
            ("Site photos", "jpg", "image/jpg", 0.3),
            ("Sign schedule", "xlsx", "", 0.15),
            ("Concept boards", "pdf", "application/pdf", 0.2),
        ];
        let w: Vec<f64> = kinds.iter().map(|k| k.3).collect();
        for _ in 0..n {
            let (label, ext, ctype, _) = kinds[self.r.weighted(&w)];
            let on_txn = !self.txns.is_empty() && self.r.chance(0.4);
            let (entity, etype, date, owner) = if on_txn {
                let t = self.r.pick(&self.txns).clone();
                (t.0, 12, t.2, t.3)
            } else {
                let c = self.r.pick(&self.customers).clone();
                let lo = c.since.max(start());
                let d = lo + Duration::days(self.r.range(0, (end() - lo).num_days().max(0)));
                (c.party, 10, d, c.name)
            };
            let id = self.r.uuid();
            let name = format!("{owner} - {label}.{ext}").replace('/', "-");
            let path = format!("{}/{:02}/{:02}/{}.{ext}", date.year(), date.month(), date.day(), &id[..8]);
            let at = self.at(date);
            let r = row(&[
                ("Id", id.clone()),
                ("FileName", name.clone()),
                ("Extension", ext.into()),
                ("ContentType", ctype.into()),
                // FileSize is in KB, rounded to 2 decimals (617 s104).
                ("FileSize", format!("{:.2}", self.r.range(40_000, 9_000_000) as f64 / 1000.0)),
                ("Category", if ext == "jpg" { "Image".into() } else { "Document".into() }),
                ("Container", format!("Container/{path}")),
            ]);
            self.push("File", r, at);
            self.x.container(path, placeholder(&name, ext));
            let r = row(&[
                ("Id", self.r.uuid()),
                ("AttachmentId", id),
                ("EntityTypeId", etype.to_string()),
                ("EntityType", if etype == 12 { "SalesTransaction".into() } else { "Party".into() }),
                ("EntityId", entity),
                ("IncludeOnSend", fm_bool(etype == 12)),
            ]);
            self.push("FileAttachment", r, at);
        }
    }

    fn notifications(&mut self) {
        let n = ((200.0 * self.scale).round() as usize).max(2);
        for _ in 0..n {
            let date = end() - Duration::days(self.r.range(0, 400));
            let (party, _, _) = self.r.pick(&self.employees[..3]).clone();
            let at = self.at(date);
            // A comment notification (kontroller 422 s176, 670 s100-110): about a discussion (entity type 11), with
            // the discussion's title as its text.
            let (entity, title) = if self.discussions.is_empty() {
                (String::new(), String::new())
            } else {
                self.r.pick(&self.discussions).clone()
            };
            let r = row(&[
                ("Id", self.r.uuid()),
                ("PartyId", party),
                ("Date", fm_date(date)),
                ("Description", title),
                ("EntityId", entity),
                ("ReadInd", fm_bool(date < end() - Duration::days(14))),
                ("SentInd", "1".into()),
                ("EntityTypeId", "11".into()),
            ]);
            self.push("Notification", r, at);
        }
    }
}

struct Txn {
    id: String,
    doc: String,
    date: NaiveDate,
    invoice: bool,
    lines: Vec<Line>,
    ship: i64,
    memo: String,
    locked: bool,
}

struct Line {
    id: String,
    item: String,
    desc: String,
    /// Units of 0.0001.
    qty: i64,
    /// Cents (the rounded unit price when `price_7dp` is set).
    price: i64,
    taxable: bool,
    /// The amount FileMaker stored, when it is not qty x price (a typed fixed fee).
    amount_text: Option<String>,
    price_7dp: Option<String>,
}

impl Line {
    fn price_text(&self) -> String {
        self.price_7dp.clone().unwrap_or_else(|| fm_money(self.price))
    }
}

/// `cents / n` with 7 decimals, as FileMaker's Round(amount / qty; 7) writes it: 100 / 3 -> "33.3333333".
fn seven_dp(cents: i64, n: i64) -> String {
    let units = div_round(cents as i128 * 100_000, n as i128); // 1e-7 dollars
    let s = format!("{}.{:07}", units / 10_000_000, units % 10_000_000);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn div_round(n: i128, d: i128) -> i128 {
    let q = n / d;
    let r = n % d;
    if 2 * r.abs() >= d {
        q + n.signum()
    } else {
        q
    }
}

fn days_in_month(first: NaiveDate) -> i64 {
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .unwrap();
    (next - first).num_days()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_decimals_like_filemaker() {
        assert_eq!(seven_dp(10000, 3), "33.3333333");
        assert_eq!(seven_dp(20000, 3), "66.6666667");
        assert_eq!(seven_dp(30000, 3), "100");
    }
}
