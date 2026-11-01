//! The dirty rows: a small, documented set of rows that BR-IMPORT-01 must reject, skip or report. Each has an
//! expected outcome, which the importer's tests check one by one.
//!
//! | Key | Table | What is wrong | Expected outcome |
//! |---|---|---|---|
//! | dirty-line-orphan | SalesLine | SalesTransactionId names no transaction | rejected: missing parent |
//! | dirty-txn-bad-date | SalesTransaction | date "2/30/2025" | rejected: bad date |
//! | dirty-activity-bad-date | Activity | DateDisplay "13/01/2024" (D/M/Y) | rejected: bad date |
//! | p-hp | Party | a second row with the key of Halden & Pryce | rejected: duplicate key (the first row stays) |
//! | dirty-message-orphan | ActivityMessage | ActivityId names no activity | rejected: missing parent |
//! | dirty-item-tbd | Item | unitPrice "TBD" in a Number field | rejected: not a number |
//! | dirty-vendor-role | Vendor | CreditLimit "call first" (Text in the source) | loaded, credit_limit NULL, listed |
//! | dirty-cust-role | PartyRole | customer role with an empty PartyRoleTypeId (BR-PARTY-05) | loaded, listed |
//! | dirty-pay-round | Payment | amount "100.005" changes on cent rounding | loaded as 10001 cents, listed |
//! | dirty-line-qty | SalesLine | qty "1.33333" (more than 4 decimals) | loaded as 1.3333, amount recomputed, listed |
//! | dirty-file-missing | File | Container names a file the export does not have | loaded, container_path NULL, listed |
//! | dirty-fa-chat | FileAttachment | attachment of a chat message (entity type 14) | skipped (OD-05-02) |
//! | p-ellison | Party | NameCached "Ellison Studio LLC" is stale | loaded, recomputed to "Ellison Studio", listed |
//! | t-inv-1027 | SalesTransaction | totalAmt "4000" was passed in, the lines say 4180 | loaded, recomputed, listed |
//! | (3 rows) | Activity | OpportunityId_DEPRECATED holds a value | loaded; the report counts the values (CHECK-01) |
//! | (all) | Chat, ChatMessage, ChatConsent | Twilio SMS/chat | skipped (OD-05-02); SMS activities (type 9) load |
//!
//! The clean part also carries values the import must change and list without calling them dirty: unit prices
//! back-computed to 7 decimals (BR-SALES-01), balances that the payments recompute (BR-SALES-02), and the secrets of
//! D-09 (System::SystemEmailAccountPassword, Employee::SocialSecurityNumber), which must not reach the database.

use chrono::NaiveDate;

use crate::story::row;
use crate::{fm_date, Export};

/// What the import must do with a dirty row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// In import_rejected_row with a reason that contains this text.
    Rejected(&'static str),
    /// Loaded; import_changed_value has a row for this source field.
    Changed(&'static str),
    /// Not loaded and not an error: counted as skipped in the report.
    Skipped,
}

/// One dirty row: FileMaker table, source key, expected outcome.
pub const ROWS: &[(&str, &str, Outcome)] = &[
    ("SalesLine", "dirty-line-orphan", Outcome::Rejected("missing parent")),
    ("SalesTransaction", "dirty-txn-bad-date", Outcome::Rejected("bad date")),
    ("Activity", "dirty-activity-bad-date", Outcome::Rejected("bad date")),
    ("Party", "p-hp", Outcome::Rejected("duplicate key")),
    ("ActivityMessage", "dirty-message-orphan", Outcome::Rejected("missing parent")),
    ("Item", "dirty-item-tbd", Outcome::Rejected("not a number")),
    ("Vendor", "dirty-vendor-role", Outcome::Changed("CreditLimit")),
    ("PartyRole", "dirty-cust-role", Outcome::Changed("PartyRoleTypeId")),
    ("Payment", "dirty-pay-round", Outcome::Changed("amount")),
    ("SalesLine", "dirty-line-qty", Outcome::Changed("qty")),
    ("File", "dirty-file-missing", Outcome::Changed("Container")),
    ("FileAttachment", "dirty-fa-chat", Outcome::Skipped),
    ("Party", "p-ellison", Outcome::Changed("NameCached")),
    ("SalesTransaction", "t-inv-1027", Outcome::Changed("totalAmt")),
];

/// Activity rows whose deprecated OpportunityId holds a value.
pub const DEPRECATED_VALUES: usize = 3;

pub fn add(x: &mut Export) {
    let d = NaiveDate::from_ymd_opt(2025, 5, 5).unwrap();
    let ts = "5/5/2025 9:30:00 AM".to_string();
    let stamp = |mut r: crate::Row| {
        r.insert("CreationTimestamp", ts.clone());
        r.insert("CreationAccountName", "mara".into());
        r
    };

    // A test customer whose name needs CSV quoting, with the empty-role-type customer role (BR-PARTY-05).
    let org = "dirty-org";
    x.push(
        "Party",
        stamp(row(&[
            ("Id", org.into()),
            ("NameCached", "Dirtywater \"Test\" Co, Inc.".into()),
            ("OrganizationInd", "1".into()),
            ("PersonInd", "0".into()),
        ])),
    );
    x.push(
        "Organization",
        stamp(row(&[("Id", org.into()), ("OrganizationName", "Dirtywater \"Test\" Co, Inc.".into())])),
    );
    x.push(
        "PartyRole",
        stamp(row(&[
            ("Id", "dirty-cust-role".into()),
            ("PartyId", org.into()),
            ("CustomerInd", "1".into()),
            ("CurrentInd", "1".into()),
            ("FromDate", fm_date(d)),
        ])),
    );
    x.push("Customer", stamp(row(&[("Id", "dirty-cust-role".into()), ("DisplayName", "Dirtywater Test Co".into())])));
    x.push(
        "PartyRole",
        stamp(row(&[
            ("Id", "dirty-vendor-role".into()),
            ("PartyId", org.into()),
            ("PartyRoleTypeId", "3".into()),
            ("VendorInd", "1".into()),
            ("CurrentInd", "1".into()),
        ])),
    );
    x.push("Vendor", stamp(row(&[("Id", "dirty-vendor-role".into()), ("CreditLimit", "call first".into())])));

    // An invoice with a quantity of more than 4 decimals: loaded, quantity rounded, amount recomputed.
    let txn = |id: &str, date: &str| {
        row(&[
            ("id", id.into()),
            ("docNumber", id.to_uppercase()),
            ("date", date.into()),
            ("id_Customer", "dirty-cust-role".into()),
            ("InvoiceInd", "1".into()),
            ("SalesReceiptInd", "0".into()),
            ("LockedInd", "1".into()),
            ("subtotal", "40".into()),
            ("totalAmt", "40".into()),
            ("balance", "40".into()),
        ])
    };
    x.push("SalesTransaction", txn("dirty-txn-qty", "5/5/2025"));
    x.push("Invoice", row(&[("Id", "dirty-txn-qty".into())]));
    x.push(
        "SalesLine",
        row(&[
            ("id", "dirty-line-qty".into()),
            ("SalesTransactionId", "dirty-txn-qty".into()),
            ("ItemId", "i-svc-hourly".into()),
            ("description", "Design revision hours".into()),
            ("qty", "1.33333".into()),
            ("unitPrice", "30".into()),
            ("amount", "40".into()),
        ]),
    );
    x.push("SalesTransaction", txn("dirty-txn-bad-date", "2/30/2025"));
    x.push(
        "SalesLine",
        row(&[
            ("id", "dirty-line-orphan".into()),
            ("SalesTransactionId", "missing-txn".into()),
            ("ItemId", "i-svc-hourly".into()),
            ("qty", "1".into()),
            ("unitPrice", "95".into()),
            ("amount", "95".into()),
        ]),
    );
    x.push(
        "Payment",
        row(&[
            ("id", "dirty-pay-round".into()),
            ("id_Customer", "dirty-cust-role".into()),
            ("date", fm_date(d)),
            ("amount", "100.005".into()),
            ("id_PaymentMethod", "Check".into()),
        ]),
    );
    x.push(
        "Item",
        row(&[("id", "dirty-item-tbd".into()), ("name", "Custom fabrication".into()), ("unitPrice", "TBD".into())]),
    );

    // Activities: a D/M/Y date and a message whose activity is gone.
    x.push(
        "Activity",
        stamp(row(&[
            ("Id", "dirty-activity-bad-date".into()),
            ("ActivityTypeId", "2".into()),
            ("Subject", "Imported from the old diary".into()),
            ("DateDisplay", "13/01/2024".into()),
        ])),
    );
    x.push(
        "ActivityMessage",
        row(&[
            ("Id", "dirty-message-orphan".into()),
            ("ActivityId", "missing-activity".into()),
            ("Message", "Orphan".into()),
        ]),
    );

    // A second Halden & Pryce row: the first one wins, this one is rejected.
    x.push(
        "Party",
        row(&[
            ("Id", "p-hp".into()),
            ("NameCached", "Halden and Pryce (duplicate)".into()),
            ("OrganizationInd", "1".into()),
        ]),
    );

    // Files: one whose container file is missing, one attached to a chat message.
    let file = |id: &str, container: &str| {
        row(&[
            ("Id", id.into()),
            ("FileName", format!("{id}.pdf")),
            ("Extension", "pdf".into()),
            ("Container", container.into()),
            ("CreationTimestamp", ts.clone()),
            ("CreatedBy", "mara".into()),
        ])
    };
    x.push("File", file("dirty-file-missing", "Container/2024/01/01/missing.pdf"));
    x.push("File", file("dirty-file-chat", ""));
    x.push(
        "FileAttachment",
        row(&[
            ("Id", "dirty-fa-chat".into()),
            ("AttachmentId", "dirty-file-chat".into()),
            ("EntityTypeId", "14".into()),
            ("EntityId", "some-chat-message".into()),
        ]),
    );

    // Stale values the import recomputes.
    x.find_mut("Party", "Id", "p-ellison").unwrap().insert("NameCached", "Ellison Studio LLC".into());
    x.find_mut("SalesTransaction", "id", "t-inv-1027").unwrap().insert("totalAmt", "4000".into());
    for r in x.rows_mut("Activity").iter_mut().take(DEPRECATED_VALUES) {
        r.insert("OpportunityId_DEPRECATED", "legacy-opportunity".into());
    }
}
