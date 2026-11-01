# DDR schema audit: every DDR table and field against the SQLite schema

KarbonFM, audited 2026-11-01 before I3 (import) and I2 (parties and contacts). Question: was everything in the FileMaker files truly migrated to schema 04? Scope: every base table and every field of the 13 DDR files (`objects/<file>.fmp12/tables/*.txt`), checked against the database that the refinery migrations build (`app/server/migrations/`), the source-to-target register (`04_database_schema_appendix.md`) and the decisions in 01, 02, 03 and 07.

Labels follow 09: **fact** (from an object file or the built database), **inference**, **product decision**. Object files are cited as `objects/<path>`.

## Summary

| Check | Result |
|---|---|
| DDR base tables | **195** in 13 files. 53 become target tables (56 karbon_data tables, 3 of them folded into a parent by D-15). The other 142 have a decision that drops them (table below). |
| DDR fields | **3,071**. **689 migrated** to a column (all karbon_data), **64 computed** (unstored calculations and summary fields, recomputed, D-39), **2,318 dropped** by a decision (414 in karbon_data, 1,904 in the other 12 files). Every field has one row in `10_ddr_schema_audit.tsv`. |
| Fields with data value and no column or decision | **0**. Every stored, non-global field of karbon_data has a column or a decision id. |
| Column types | **0 wrong.** 447 Text to TEXT, 128 Number to NUMERIC or INTEGER (21 are money, INTEGER cents, OD-04-02), 107 Date/Time/Timestamp to ISO TEXT (D-05), 5 containers to a `*_path` TEXT (D-17). Two accepted differences: vendor.credit_limit is Text in the source and INTEGER cents in the target (BR-IMPORT-01), and ActivityEntity.EntityTypeId is Text in the source and NUMERIC like entity_type.id (D-16). |
| Repeating fields | 12 fields repeat (2 to 100). All are in dropped tables: kontroller DBTransactions (D-08) and tester TestData (D-01). karbon_data has none, so OD-04-03 makes no child table. |
| Calculated and summary fields | 145 calculations and 31 summaries in karbon_data. 11 stored calculations keep a column that the commands and the import recompute; 34 unstored calculations and 30 summaries (64) are computed in a command or a view (D-39); the other 101 are FileMaker plumbing (D-08) or belong to dropped tables. |
| Auto-enter | 56 primary keys keep their source values: 35 UUID auto-enters, 10 serials (the next id is MAX(id) + 1) and 11 subtype or validated keys (D-15). 225 creation and modification stamps (timestamp, account name, user id, created/modified by) are columns (D-39). 11 constants and 5 lookups have no database default: the commands set them (D-27). |
| Foreign keys and indexes | 54 enforced foreign keys (OD-04-01), every one indexed. The tables I2 touches have every key they need. **3 indexes were missing** for screens and the import report: fixed in V2. |
| Value lists | **The seed was missing**: value_list_item had no rows, but 07 says a custom value list refuses an unknown value. Fixed in V2: 9 lists, 24 values. 4 lists the appendix had marked "seeded" have no target column and are now marked not seeded. |
| GAPs found and fixed | **5**: 3 fixed in the schema by migration V2, 2 in the documentation (below). |
| CHECK items for the owner | **4** (below). None blocks I2 or I3. |
| Schema confirmed for I2 | **Yes.** party, person, organization, party_role (customer, vendor, employee, lead), party_relation, contact_method (address, phone, network_address), contact_method_purpose, party_role_type, contact_method_purpose_type and user: every field accounted for, keys and indexes in place, and V2 changes none of them. One owner question, CHECK-03 (party_role_type ids for employee and vendor), affects what I2 writes, not the schema. |

How this is kept true: `app/server/tests/ddr_schema_audit.rs` parses every object file and checks that each field has exactly one register row; that each migrated row names a column of the migrated database with the recorded type, NOT NULL and foreign key; that every other column is app-only; that every dropped row names a D-, OD- or BR- id; that the seeded value lists equal the DDR; and that schema 04 and the migrations build the same database.

## Method

- **Parse (fact).** All 195 `tables/*.txt` object files: table id and name, DDR record count, and for each field its id, name, data type, field type (Normal, Calculated, Summary), repetitions, global storage and auto-enter options (serial, UUID, stamps, constants, lookups, calculations).
- **Register.** Each karbon_data field was joined to its row in the 04 appendix field register (1,167 of 1,167 have one). Fields of the other 12 files take the disposition of their table: the appendix base-table register for kontacts, kontroller, Picker, charts, log and LedgerLink, and D-01 or D-03 for the 54 tables of the out-of-scope files, which the appendix did not list (GAP-04).
- **Target (fact).** The database built by refinery V1 + V2 was read with `PRAGMA table_info`, `foreign_key_list` and `index_list`. For each migrated field: the column exists, and its type, NOT NULL and foreign key are recorded in the register. For each column: a source field exists, or the column is app-only (app_meta, the import tables, value_list_item, system_setting.singleton, the two tax_rate_bp columns of OD-07-06).
- **Use.** Every `*_id` column without a foreign key was searched in 05, 06 and 07 and in the route SQL (`app/server/src/routes/`, `invoice.rs`) for a screen or rule that needs it. Every custom value list was traced to the layout fields it is bound to (`objects/*/layouts/*.txt`, `ValueList:` lines).
- **Output.** `10_ddr_schema_audit.tsv`: one row per field (3,071). Columns: file, table id, table, field id, field, data type, field type, repetitions, global, auto-enter, status (column, computed, dropped), target, target type, target NOT NULL, target foreign key, decision.

## GAP (must fix): 5 found, 5 fixed

| Id | Gap (fact) | Fix | Where |
|---|---|---|---|
| GAP-01 | value_list_item is empty. The appendix marks 13 custom value lists "value_list_item rows (seeded)", and 07 ("Custom value lists (enum)") says an unknown value is refused, but no code or seed writes a row. | V2 seeds the 9 lists whose values a target column stores, in DDR order: AuthMethods (system_setting.authentication_method), DiscussionView Choices (activity.discussion_allowed_for), ItemType (item.item_type; kontacts "Item Type" is the same list), OpportunityProbability (opportunity.probability), OpportunityType (opportunity.type_display), SMTP Authentication (system_setting.system_email_smtp_server_authentication, user.smtp_server_authentication), SMTP Encryption (system_setting.system_email_smtp_server_encryption_type), System Email Sender (system_setting.system_email_sender; the source value "FileMakar SMTP" is kept as stored), SystemServerType (system_setting.server_type). Not seeded, appendix corrected: TwilioFormatOptions (OD-05-02), AddressServiceProvider (Picker fields only; D-23, D-14), Code Location (charts Widgets only; D-23), Task Status (a search global; status comes from task.completed_at, D-29). | V2; appendix value lists; test `seeded_value_lists_equal_the_ddr` |
| GAP-02 | The import cannot record what the runbook and UI-18 need. 03 "Extract and stage" records "each file's name, size, SHA-256, row count and encoding in import_batch", but import_batch has no such columns. UI-18 step 4 shows rows loaded per table, and BR-IMPORT-01 lists every value that changes in the conversion, but neither has a table. import_rejected_row has no index on batch_id. | V2 adds `import_file` (one row per export file: name, size, SHA-256, encoding, rows read, loaded and rejected) and `import_changed_value` (table, source key, field, exported value, stored value, reason), plus indexes on batch_id for both report tables. | V2; 04; appendix "Schema changes after V1" |
| GAP-03 | Missing indexes for screens. The invoice page reads a transaction by its number (`invoice.rs`: `WHERE st.doc_number = ?1`), and the sales lists and the dashboard order and filter by date; sales_transaction has neither index. | V2: `ix_sales_transaction_doc_number`, `ix_sales_transaction_date`. | V2; 04 |
| GAP-04 | The appendix base-table register leaves out the 54 tables of karbon_Generator (32), karbon_Build (12), karbon_tester (7), karbon_dev, ZeitGeist and karbon_konnector (1 each). 04 says it lists "one row per base table". | 54 rows added: D-01 (developer tooling files) or D-03 (time-tracking shell files). | appendix "Base tables" |
| GAP-05 | 13 field dispositions say "FileMaker plumbing (join anchor, JSON serializer or counter)" but are not. 11 are deprecated data fields (Activity::OpportunityId_DEPRECATED, Activity::CompletionTimestamp_deprecated, CompletionAccountName_deprecated, CompletedByCached_deprecated; ActivityComment::UserId_DEPRECATED; Discussion::EntityTypeId_Deprecated, EntityId_Deprecated; Notification::UserId_Deprecated, CommentId_Deprecated, TaskId_Deprecated; Discussion::DisplayActiveUsers_Deprecated), and 2 are karbon_tester batch markers (ActivityGroup::CreatedByTestBatchID, System::CreatedByTestBatchID). | Relabeled: "deprecated in the source, superseded (D-12)" and "karbon_tester batch marker (D-01)". Whether old records still hold values there is CHECK-01. | appendix field register; register TSV |

The schema change is migration V2 (`app/server/migrations/V2__ddr_audit.sql`). V1 is never edited: it is now the frozen file `app/server/migrations/V1__schema_04.sql`, byte for byte the 04 that I1 applied. `04_database_schema.sql` holds V2 word for word and shows the schema after the last migration (tests `schema_04_equals_the_migrations` and `v1_is_the_first_schema_04_and_v2_is_inside_04`). The demo seed is unchanged and loads on V1 + V2 with an empty `foreign_key_check`.

## CHECK (needs an owner decision): 4

| Id | Question | Facts | Recommendation (default if no answer) |
|---|---|---|---|
| CHECK-01 | Do the deprecated fields hold history worth keeping? | Activity::OpportunityId_DEPRECATED links an activity to an opportunity, and Discussion::EntityTypeId_Deprecated/EntityId_Deprecated link a discussion to an entity: both are now done by activity_entity rows. Activity::CompletionTimestamp_deprecated is the completion time from before Task::CompletionTimestamp (task.completed_at). On an old file these may hold values that the new fields do not. | Not migrated (D-12). The import counts the non-empty values of each deprecated field in its report, so an install with real data can see what it would lose. If the owner wants them: OpportunityId becomes an activity_entity row with entity_type 13 (Opportunity), and CompletionTimestamp fills an empty task.completed_at. |
| CHECK-02 | Are the SMTP value lists swapped? | "SMTP Authentication" holds None, SSL, TLS (connection security) and is bound to the authentication fields. "SMTP Encryption" holds None, Plain Password, Cram-MD5 (authentication methods) and is bound to the "Connection Encryption" field (`objects/karbon_kontacts.fmp12/layouts/133-Welcome_Wizard.txt`). A D-07 style copy error. | V2 seeds both as the source has them, so imported values match. Before a settings screen offers them as choices, decide whether to swap the two lists. |
| CHECK-03 | Which party_role_type ids do employee and vendor roles use? | The DDR has 4 PartyRoleType records but no values for them. The scripts hard-code only 2 Customer and 4 Lead (BR-ADMIN-02). Setup writes the owner's employee role with an empty party_role_type_id; the employee_ind and vendor_ind flags carry the role. | I2 keeps that: employee and vendor roles have an empty party_role_type_id and set employee_ind or vendor_ind. The import keeps the user's own rows, so an export brings the real ids 1 and 3. The synthetic export (I3) uses 1 Employee and 3 Vendor. |
| CHECK-04 | Should the id columns without a DDR relationship become foreign keys? | 15 columns hold another table's id but have no relationship in the DDR, so they are not constraints (D-16: "keep schema 04 as written"): notification.party_id, opportunity.salesperson_id, opportunity.referring_party_id, activity_party.contact_method_id, event.address_id, activity.activity_parent_id, discussion.resolution_id, template.activity_type_id, template.activity_purpose_id, template.user_id, merge_field.activity_party_role_id, merge_field.activity_id, payment_line.id_customer, system_setting.country_id, file_attachment.entity_type_id. No rule or screen in 05/06/07 reads them yet. Three hold text where the target id is a number (template.activity_type_id, merge_field.activity_party_role_id; activity_purpose.activity_purpose_type_id is the reverse), as D-16 lists. Adding a foreign key in SQLite means rebuilding the table. | Leave as written. The increment that first reads one (notifications, opportunities) adds the foreign key in its own migration, after an import profile shows the values resolve. |

## OK (checked, no change)

- **Folded subtypes (D-15).** ActivityMessage's Message and ManuallyEditedInd go to activity.message and activity.manually_edited_ind; Note and Meeting have only their key, marked by activity.note_ind and event.meeting_ind. Their stamps fold into the parent's columns.
- **Subtype keys.** customer, vendor, employee and lead reference party_role; person and organization reference party; address, phone and network_address reference contact_method; call, discussion, task, email_message and event reference activity; invoice and sales_receipt reference sales_transaction. A parent does not reference its subtype (the 2026-10-28 FK pass).
- **Money (OD-04-02).** All 21 money fields of the source are INTEGER cents, and so is vendor.credit_limit: 22 columns. No money field is NUMERIC or REAL.
- **Containers (D-17).** The 5 containers (Party::Photo, Employee::Avatar, System::Logo, File::Container, File::Thumbnail) are `*_path` TEXT columns. The other 12 Binary fields are in dropped files.
- **Secrets (D-09, BR-IMPORT-02).** Employee::SocialSecurityNumber, System::SystemEmailAccountPassword, SmartyStreetsAuthId, SmartyStreetsAuthToken and FullContactAPIKey have no column; User::SmtpPassword is a global. Tax ids are plain TEXT (owner, 2026-10-28).
- **Globals (D-23).** All 66 karbon_data globals (z_xJoin, Invoice::currentPaymentId, User::SmtpPassword) have no column.
- **Stamps (D-39).** Every CreationTimestamp, ModificationTimestamp, CreationAccountName, ModificationAccountName, CreationUserId and ModificationUserId of a migrated table is a column. ModificationCount (a FileMaker counter) is not migrated.
- **Constants without a database default (D-27).** No column has a DEFAULT. The commands set the source constants. For I2: party_role.current_ind = 1 and from_date (the source constant is 1/1/0001; BR-PARTY-05 sets today), party_relation.current_ind = 1, phone.voice_ind = 1. Others: item.active, item.taxable, sales_transaction.status and locked_ind, merge_field.required_ind, file_attachment.include_on_send, user.email_signature. Lookups (activity_party.party_id, party_name_cached, contact_method_id, activity_party_email.email_address, sales_line.unit_price) are filled by the commands.
- **Reference rows (BR-ADMIN-02).** The DDR holds record counts but no values for country (179), external_link_type (6), phone_format (3), party_role_type (4) and contact_method_purpose_type (3). Those rows come from the user's export. The reference seed has the hard-coded ids (44 rows); the demo seed adds its own.
- **Type joins (D-16).** activity_purpose.activity_purpose_type_id (NUMERIC) references activity_purpose_type.id (TEXT), as in the source. SQLite applies the parent column's affinity when it checks the key, so a text id still resolves. The import coerces each key to the type of its target column.
- **Redundant indexes.** 24 indexes repeat a primary key (for example ix_party_id). They are harmless, and removing them is not worth a migration.

## Counts by decision

| Decision | karbon_data fields | Other files' fields | What it covers |
|---|---:|---:|---|
| column | 689 | 0 | migrated |
| computed (D-39) | 64 | 0 | unstored calculations and summaries |
| D-08 | 136 | 94 | FileMaker plumbing (ONE, JSON serializers, counters); kontroller DBTransactions |
| D-11 | 93 | 0 | ChartingLibrary, PartyImport (FileMaker staging) |
| D-23 | 57 | 467 | globals and screen state; kontacts, Picker, charts, log settings |
| OD-05-02 | 51 | 0 | Chat, ChatConsent, ChatMessage (Twilio removed) |
| D-02 | 28 | 788 | QBO sync columns; karbon_LedgerLink |
| D-21 | 23 | 0 | MergeFieldConfig |
| D-12 | 11 | 0 | deprecated fields (CHECK-01) |
| D-09 | 5 | 0 | secrets |
| D-30 | 5 | 11 | HTTPRequestLog; karbon_log Log |
| D-29 | 2 | 0 | Activity::AlertInd, Task::CompletedInd (replaced by task.completed_at) |
| D-01 | 2 | 542 | test batch markers; Generator, Build, tester, dev |
| D-20 | 1 | 0 | System::BookCloseDate |
| D-03 | 0 | 2 | ZeitGeist, konnector |
| **Total** | **1,167** | **1,904** | **3,071** |

## Tables by file

"Column", "Computed" and "Dropped" count the table's fields by status. DDR records is the record count of the DDR file (D-05: the DDR files are almost empty). Field-level detail is in `10_ddr_schema_audit.tsv`.

### karbon_data.fmp12

65 tables, 1167 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/142-Activity.txt` Activity | 38 | 0 | 28 | 1 | 9 | activity | table |
| `tables/174-ActivityComment.txt` ActivityComment | 14 | 0 | 10 | 0 | 4 | activity_comment | table |
| `tables/201-ActivityEntity.txt` ActivityEntity | 10 | 0 | 8 | 0 | 2 | activity_entity | table |
| `tables/155-ActivityGroup.txt` ActivityGroup | 12 | 0 | 8 | 0 | 4 | activity_group | table |
| `tables/143-ActivityMessage.txt` ActivityMessage | 14 | 0 | 10 | 1 | 3 | activity | folded into activity (D-15) |
| `tables/144-ActivityParty.txt` ActivityParty | 20 | 0 | 14 | 2 | 4 | activity_party | table |
| `tables/173-ActivityPartyEmail.txt` ActivityPartyEmail | 8 | 0 | 3 | 2 | 3 | activity_party_email | table |
| `tables/172-ActivityPartyEvent.txt` ActivityPartyEvent | 9 | 0 | 5 | 2 | 2 | activity_party_event | table |
| `tables/145-ActivityPartyRole.txt` ActivityPartyRole | 16 | 16 | 12 | 0 | 4 | activity_party_role | table |
| `tables/146-ActivityPurpose.txt` ActivityPurpose | 13 | 0 | 10 | 0 | 3 | activity_purpose | table |
| `tables/147-ActivityPurposeType.txt` ActivityPurposeType | 12 | 0 | 8 | 0 | 4 | activity_purpose_type | table |
| `tables/148-ActivityType.txt` ActivityType | 12 | 10 | 8 | 0 | 4 | activity_type | table |
| `tables/131-Address.txt` Address | 22 | 1 | 16 | 2 | 4 | address | table |
| `tables/202-Call.txt` Call | 6 | 0 | 4 | 0 | 2 | call | table |
| `tables/197-ChartingLibrary.txt` ChartingLibrary | 11 | 0 | 0 | 0 | 11 | - | dropped: chart library storage, no user (D-11) |
| `tables/203-Chat.txt` Chat | 15 | 0 | 0 | 0 | 15 | - | removed: Twilio SMS/chat, out of scope (owner, 2026-10-28; OD-05-02) |
| `tables/206-ChatConsent.txt` ChatConsent | 8 | 0 | 0 | 0 | 8 | - | removed: Twilio SMS/chat, out of scope (owner, 2026-10-28; OD-05-02) |
| `tables/205-ChatMessage.txt` ChatMessage | 24 | 0 | 0 | 0 | 24 | - | removed: Twilio SMS/chat, out of scope (owner, 2026-10-28; OD-05-02) |
| `tables/132-ContactMethod.txt` ContactMethod | 22 | 5 | 16 | 2 | 4 | contact_method | table |
| `tables/158-ContactMethodPurpose.txt` ContactMethodPurpose | 13 | 0 | 9 | 0 | 4 | contact_method_purpose | table |
| `tables/159-ContactMethodPurposeType.txt` ContactMethodPurposeType | 8 | 3 | 4 | 0 | 4 | contact_method_purpose_type | table |
| `tables/133-Country.txt` Country | 11 | 179 | 8 | 0 | 3 | country | table |
| `tables/140-Customer.txt` Customer | 39 | 0 | 30 | 3 | 6 | customer | table |
| `tables/184-Discussion.txt` Discussion | 18 | 0 | 11 | 1 | 6 | discussion | table |
| `tables/168-EmailMessage.txt` EmailMessage | 9 | 0 | 7 | 0 | 2 | email_message | table |
| `tables/141-Employee.txt` Employee | 26 | 1 | 17 | 3 | 6 | employee | table |
| `tables/193-EntityType.txt` EntityType | 8 | 4 | 6 | 0 | 2 | entity_type | table |
| `tables/171-Event.txt` Event | 20 | 0 | 15 | 1 | 4 | event | table |
| `tables/134-ExternalLink.txt` ExternalLink | 17 | 0 | 10 | 2 | 5 | external_link | table |
| `tables/160-ExternalLinkType.txt` ExternalLinkType | 13 | 6 | 9 | 0 | 4 | external_link_type | table |
| `tables/195-File.txt` File | 23 | 0 | 17 | 0 | 6 | file | table |
| `tables/196-FileAttachment.txt` FileAttachment | 13 | 0 | 10 | 0 | 3 | file_attachment | table |
| `tables/198-Globals.txt` Globals | 0 | 1 | 0 | 0 | 0 | - | dropped: globals only, screen state (D-23) |
| `tables/176-HTTPRequestLog.txt` HTTPRequestLog | 5 | 0 | 0 | 0 | 5 | - | dropped: replaced by the local log file (D-30) |
| `tables/190-Invoice.txt` Invoice | 16 | 0 | 10 | 2 | 4 | invoice | table |
| `tables/183-Item.txt` Item | 27 | 0 | 17 | 0 | 10 | item | table |
| `tables/162-Lead.txt` Lead | 15 | 0 | 11 | 0 | 4 | lead | table |
| `tables/175-Meeting.txt` Meeting | 3 | 0 | 1 | 0 | 2 | event | folded into event (D-15) |
| `tables/188-MergeField.txt` MergeField | 17 | 17 | 14 | 0 | 3 | merge_field | table |
| `tables/189-MergeFieldConfig.txt` MergeFieldConfig | 23 | 7 | 0 | 0 | 23 | - | not migrated: templates use fixed placeholders (owner, 2026-10-28; D-21) |
| `tables/129-meta.txt` meta | 2 | 1 | 0 | 0 | 2 | - | dropped: FileMaker file version and session user id (D-23) |
| `tables/135-NetworkAddress.txt` NetworkAddress | 20 | 2 | 11 | 4 | 5 | network_address | table |
| `tables/149-Note.txt` Note | 3 | 0 | 1 | 0 | 2 | activity | folded into activity (D-15) |
| `tables/186-Notification.txt` Notification | 17 | 0 | 12 | 0 | 5 | notification | table |
| `tables/199-Opportunity.txt` Opportunity | 31 | 0 | 24 | 3 | 4 | opportunity | table |
| `tables/200-OpportunityStatus.txt` OpportunityStatus | 9 | 7 | 6 | 0 | 3 | opportunity_status | table |
| `tables/157-Organization.txt` Organization | 16 | 1 | 10 | 1 | 5 | organization | table |
| `tables/136-Party.txt` Party | 39 | 2 | 27 | 4 | 8 | party | table |
| `tables/194-PartyImport.txt` PartyImport | 82 | 0 | 0 | 0 | 82 | - | dropped: staging table of a FileMaker import (D-11; the importer replaces it, BR-IMPORT-01) |
| `tables/137-PartyRelation.txt` PartyRelation | 20 | 2 | 15 | 1 | 4 | party_relation | table |
| `tables/151-PartyRole.txt` PartyRole | 20 | 1 | 14 | 1 | 5 | party_role | table |
| `tables/154-PartyRoleType.txt` PartyRoleType | 5 | 4 | 2 | 0 | 3 | party_role_type | table |
| `tables/179-Payment.txt` Payment | 18 | 0 | 8 | 3 | 7 | payment | table |
| `tables/180-PaymentLine.txt` PaymentLine | 13 | 0 | 6 | 3 | 4 | payment_line | table |
| `tables/156-Person.txt` Person | 27 | 1 | 22 | 1 | 4 | person | table |
| `tables/138-Phone.txt` Phone | 28 | 2 | 16 | 7 | 5 | phone | table |
| `tables/139-PhoneFormat.txt` PhoneFormat | 14 | 3 | 10 | 0 | 4 | phone_format | table |
| `tables/178-SalesLine.txt` SalesLine | 19 | 0 | 10 | 3 | 6 | sales_line | table |
| `tables/181-SalesReceipt.txt` SalesReceipt | 5 | 0 | 3 | 0 | 2 | sales_receipt | table |
| `tables/177-SalesTransaction.txt` SalesTransaction | 57 | 0 | 45 | 6 | 6 | sales_transaction | table |
| `tables/165-System.txt` System | 42 | 1 | 27 | 1 | 14 | system_setting | table |
| `tables/150-Task.txt` Task | 11 | 0 | 8 | 0 | 3 | task | table |
| `tables/187-Template.txt` Template | 17 | 1 | 14 | 0 | 3 | template | table |
| `tables/130-User.txt` User | 26 | 1 | 19 | 2 | 5 | user | table |
| `tables/153-Vendor.txt` Vendor | 16 | 0 | 13 | 0 | 3 | vendor | table |

### Picker_Module.fmp12

6 tables, 81 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/141-connector.txt` connector | 2 | 1 | 0 | 0 | 2 | - | no table: utility / drop-candidate (UI, session, transaction or config of Picker Module.fmp12; D-23) |
| `tables/142-meta.txt` meta | 2 | 0 | 0 | 0 | 2 | - | no table: settings-globals / config (UI, session, transaction or config of Picker Module.fmp12; D-23) |
| `tables/136-Picker.txt` Picker | 34 | 1 | 0 | 0 | 34 | - | no table: settings-globals / session-state (UI, session, transaction or config of Picker Module.fmp12; D-23) |
| `tables/143-selector.txt` selector | 3 | 0 | 0 | 0 | 3 | - | no table: utility / drop-candidate (UI, session, transaction or config of Picker Module.fmp12; D-23) |
| `tables/139-VIRTUALLISTS.txt` VIRTUALLISTS | 2 | 1 | 0 | 0 | 2 | - | no table: utility / drop-candidate (UI, session, transaction or config of Picker Module.fmp12; D-23) |
| `tables/140-VIRTUALLISTS_Rows.txt` VIRTUALLISTS_Rows | 38 | 200 | 0 | 0 | 38 | - | no table: utility / drop-candidate (UI, session, transaction or config of Picker Module.fmp12; D-23) |

### ZeitGeist.fmp12

1 tables, 1 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/130-Timesheet.txt` Timesheet | 1 | 0 | 0 | 0 | 1 | - | out of scope: time-tracking shell file (owner, 2026-10-28; D-03) |

### karbon_Build.fmp12

12 tables, 130 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/132-AppBuild.txt` AppBuild | 10 | 0 | 0 | 0 | 10 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/141-Application.txt` Application | 9 | 0 | 0 | 0 | 9 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/133-Change.txt` Change | 17 | 0 | 0 | 0 | 17 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/136-connector.txt` connector | 2 | 1 | 0 | 0 | 2 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/142-DBTransactions.txt` DBTransactions | 24 | 0 | 0 | 0 | 24 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/134-File.txt` File | 17 | 0 | 0 | 0 | 17 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/135-FileBuild.txt` FileBuild | 10 | 0 | 0 | 0 | 10 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/129-meta.txt` meta | 5 | 1 | 0 | 0 | 5 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/137-selector.txt` selector | 11 | 1 | 0 | 0 | 11 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/140-System.txt` System | 9 | 1 | 0 | 0 | 9 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/138-VirtualList.txt` VirtualList | 5 | 1 | 0 | 0 | 5 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/139-VirtualListRows.txt` VirtualListRows | 11 | 1000 | 0 | 0 | 11 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |

### karbon_Generator.fmp12

32 tables, 367 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/149-ApiGenerator.txt` ApiGenerator | 16 | 0 | 0 | 0 | 16 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/150-ApiGeneratorLine.txt` ApiGeneratorLine | 28 | 0 | 0 | 0 | 28 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/151-ApiGeneratorLineValueOption.txt` ApiGeneratorLineValueOption | 4 | 0 | 0 | 0 | 4 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/137-CONNECTOR.txt` CONNECTOR | 1 | 1 | 0 | 0 | 1 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/135-FieldMap.txt` FieldMap | 14 | 0 | 0 | 0 | 14 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/136-FieldMapItems.txt` FieldMapItems | 16 | 0 | 0 | 0 | 16 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/142-Fields.txt` Fields | 6 | 0 | 0 | 0 | 6 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/139-Files.txt` Files | 2 | 0 | 0 | 0 | 2 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/145-Generators.txt` Generators | 13 | 0 | 0 | 0 | 13 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/146-GeneratorTokens.txt` GeneratorTokens | 9 | 0 | 0 | 0 | 9 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/147-GLOBALS.txt` GLOBALS | 30 | 0 | 0 | 0 | 30 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/132-Headers.txt` Headers | 9 | 0 | 0 | 0 | 9 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/154-JSNObject.txt` JSNObject | 13 | 0 | 0 | 0 | 13 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/155-JSNObjectLine.txt` JSNObjectLine | 12 | 0 | 0 | 0 | 12 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/157-JSNObjectLineRuntime.txt` JSNObjectLineRuntime | 15 | 0 | 0 | 0 | 15 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/156-JSNObjectType.txt` JSNObjectType | 8 | 0 | 0 | 0 | 8 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/143-meta.txt` meta | 2 | 0 | 0 | 0 | 2 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/131-QueryParameters.txt` QueryParameters | 7 | 0 | 0 | 0 | 7 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/133-RequestHistory.txt` RequestHistory | 7 | 0 | 0 | 0 | 7 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/129-Requests.txt` Requests | 28 | 0 | 0 | 0 | 28 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/148-RESOURCES.txt` RESOURCES | 6 | 0 | 0 | 0 | 6 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/141-SCHEMA.txt` SCHEMA | 2 | 0 | 0 | 0 | 2 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/158-Script.txt` Script | 5 | 0 | 0 | 0 | 5 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/138-SELECTOR.txt` SELECTOR | 13 | 1 | 0 | 0 | 13 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/140-Tables.txt` Tables | 2 | 0 | 0 | 0 | 2 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/159-Test.txt` Test | 22 | 0 | 0 | 0 | 22 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/164-TestGlobals.txt` TestGlobals | 11 | 0 | 0 | 0 | 11 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/162-TestGroup.txt` TestGroup | 7 | 0 | 0 | 0 | 7 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/160-TestLine.txt` TestLine | 26 | 0 | 0 | 0 | 26 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/161-TestLineResult.txt` TestLineResult | 13 | 0 | 0 | 0 | 13 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/165-TestLineType.txt` TestLineType | 9 | 0 | 0 | 0 | 9 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/144-VIRTUALLIST.txt` VIRTUALLIST | 11 | 0 | 0 | 0 | 11 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |

### karbon_LedgerLink.fmp12

38 tables, 788 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/130-Account.txt` Account | 21 | 0 | 0 | 0 | 21 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/163-Attachable.txt` Attachable | 20 | 0 | 0 | 0 | 20 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/164-AttachableRef.txt` AttachableRef | 11 | 0 | 0 | 0 | 11 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/154-Bill.txt` Bill | 23 | 0 | 0 | 0 | 23 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/155-BillLine.txt` BillLine | 21 | 0 | 0 | 0 | 21 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/131-Class.txt` Class | 7 | 0 | 0 | 0 | 7 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/132-CompanyInfo.txt` CompanyInfo | 50 | 0 | 0 | 0 | 50 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/133-Customer.txt` Customer | 43 | 0 | 0 | 0 | 43 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/134-Department.txt` Department | 7 | 0 | 0 | 0 | 7 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/165-Deposit.txt` Deposit | 10 | 0 | 0 | 0 | 10 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/166-DepositLine.txt` DepositLine | 15 | 0 | 0 | 0 | 15 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/157-Estimate.txt` Estimate | 44 | 0 | 0 | 0 | 44 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/158-EstimateItem.txt` EstimateItem | 13 | 0 | 0 | 0 | 13 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/156-GLOBAL.txt` GLOBAL | 3 | 1 | 0 | 0 | 3 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/148-HTTPRequestLog.txt` HTTPRequestLog | 5 | 0 | 0 | 0 | 5 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/135-Invoice.txt` Invoice | 52 | 0 | 0 | 0 | 52 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/136-InvoiceItem.txt` InvoiceItem | 15 | 0 | 0 | 0 | 15 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/137-Item.txt` Item | 24 | 0 | 0 | 0 | 24 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/150-JournalEntry.txt` JournalEntry | 9 | 0 | 0 | 0 | 9 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/151-JournalEntryItem.txt` JournalEntryItem | 12 | 0 | 0 | 0 | 12 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/162-meta.txt` meta | 2 | 1 | 0 | 0 | 2 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/138-Payment.txt` Payment | 17 | 0 | 0 | 0 | 17 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/139-PaymentLine.txt` PaymentLine | 12 | 0 | 0 | 0 | 12 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/140-PaymentMethod.txt` PaymentMethod | 6 | 0 | 0 | 0 | 6 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/141-Preferences.txt` Preferences | 9 | 0 | 0 | 0 | 9 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/167-Purchase.txt` Purchase | 14 | 0 | 0 | 0 | 14 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/168-PurchaseLine.txt` PurchaseLine | 23 | 0 | 0 | 0 | 23 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/152-PurchaseOrder.txt` PurchaseOrder | 39 | 0 | 0 | 0 | 39 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/153-PurchaseOrderLine.txt` PurchaseOrderLine | 17 | 0 | 0 | 0 | 17 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/147-QBO.txt` QBO | 65 | 1 | 0 | 0 | 65 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/142-QBOConfig.txt` QBOConfig | 16 | 1 | 0 | 0 | 16 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/146-QBOEntity.txt` QBOEntity | 29 | 19 | 0 | 0 | 29 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/160-SalesReceipt.txt` SalesReceipt | 49 | 0 | 0 | 0 | 49 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/161-SalesReceiptItem.txt` SalesReceiptItem | 15 | 0 | 0 | 0 | 15 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/143-TaxCode.txt` TaxCode | 12 | 0 | 0 | 0 | 12 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/144-TaxRate.txt` TaxRate | 10 | 0 | 0 | 0 | 10 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/145-Term.txt` Term | 14 | 0 | 0 | 0 | 14 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |
| `tables/149-Vendor.txt` Vendor | 34 | 0 | 0 | 0 | 34 | - | removed: QuickBooks Online mirror, out of scope (owner, 2026-10-27; D-02) |

### karbon_charts.fmp12

4 tables, 23 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/132-charts.txt` charts | 6 | 1 | 0 | 0 | 6 | - | no table: interface-resources / config (UI, session, transaction or config of karbon_charts.fmp12; D-23) |
| `tables/135-Globals.txt` Globals | 1 | 0 | 0 | 0 | 1 | - | no table: settings-globals / session-state (UI, session, transaction or config of karbon_charts.fmp12; D-23) |
| `tables/129-meta.txt` meta | 1 | 0 | 0 | 0 | 1 | - | no table: settings-globals / config (UI, session, transaction or config of karbon_charts.fmp12; D-23) |
| `tables/133-Widgets.txt` Widgets | 15 | 0 | 0 | 0 | 15 | - | no table: interface-resources / config (UI, session, transaction or config of karbon_charts.fmp12; D-23) |

### karbon_dev.fmp12

1 tables, 1 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/130-Timesheet.txt` Timesheet | 1 | 0 | 0 | 0 | 1 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |

### karbon_konnector.fmp12

1 tables, 1 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/130-Timesheet.txt` Timesheet | 1 | 0 | 0 | 0 | 1 | - | out of scope: time-tracking shell file (owner, 2026-10-28; D-03) |

### karbon_kontacts.fmp12

23 tables, 355 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/130-connector.txt` connector | 2 | 1 | 0 | 0 | 2 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/136-Globals.txt` Globals | 16 | 1 | 0 | 0 | 16 | - | no table: settings-globals / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/129-Karbon.txt` Karbon | 2 | 1 | 0 | 0 | 2 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/147-meta.txt` meta | 2 | 0 | 0 | 0 | 2 | - | no table: settings-globals / config (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/137-SalesUtility.txt` SalesUtility | 3 | 0 | 0 | 0 | 3 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/131-selector.txt` selector | 34 | 1 | 0 | 0 | 34 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/132-session.txt` session | 7 | 1 | 0 | 0 | 7 | - | no table: settings-globals / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/148-ThemeData.txt` ThemeData | 9 | 0 | 0 | 0 | 9 | - | no table: interface-resources / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/149-ThemeDataRelated.txt` ThemeDataRelated | 9 | 0 | 0 | 0 | 9 | - | no table: interface-resources / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/151-UtilityCall.txt` UtilityCall | 13 | 0 | 0 | 0 | 13 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/152-UtilityChat.txt` UtilityChat | 4 | 1 | 0 | 0 | 4 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/135-UtilityContactMethod.txt` UtilityContactMethod | 16 | 0 | 0 | 0 | 16 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/142-UtilityDiscussion.txt` UtilityDiscussion | 17 | 1 | 0 | 0 | 17 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/143-UtilityEmailMessage.txt` UtilityEmailMessage | 25 | 1 | 0 | 0 | 25 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/150-UtilityEmailMessage.BAK.txt` UtilityEmailMessage.BAK | 24 | 0 | 0 | 0 | 24 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/146-UtilityItem.txt` UtilityItem | 20 | 0 | 0 | 0 | 20 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/140-UtilityMeeting.txt` UtilityMeeting | 15 | 1 | 0 | 0 | 15 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/139-UtilityNote.txt` UtilityNote | 12 | 1 | 0 | 0 | 12 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/138-UtilityParty.txt` UtilityParty | 78 | 1 | 0 | 0 | 78 | - | no table: staging / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/141-UtilityTask.txt` UtilityTask | 16 | 1 | 0 | 0 | 16 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/144-UtilityUser.txt` UtilityUser | 14 | 0 | 0 | 0 | 14 | - | no table: utility / session-state (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/133-VirtualList.txt` VirtualList | 6 | 1 | 0 | 0 | 6 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |
| `tables/134-VirtualListRows.txt` VirtualListRows | 11 | 1000 | 0 | 0 | 11 | - | no table: utility / drop-candidate (UI, session, transaction or config of karbon_kontacts.fmp12; D-23) |

### karbon_kontroller.fmp12

2 tables, 97 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/130-DBTransactions.txt` DBTransactions | 94 | 4 | 0 | 0 | 94 | - | no table: FileMaker transaction log; one SQLite transaction per operation (D-08) |
| `tables/129-meta.txt` meta | 3 | 1 | 0 | 0 | 3 | - | no table: settings-globals / config (UI, session, transaction or config of karbon_kontroller.fmp12; D-23) |

### karbon_log.fmp12

3 tables, 16 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/131-Log.txt` Log | 11 | 0 | 0 | 0 | 11 | - | no table: replaced by the local log file (D-30) |
| `tables/129-meta.txt` meta | 2 | 1 | 0 | 0 | 2 | - | no table: settings-globals / config (UI, session, transaction or config of karbon_log.fmp12; D-23) |
| `tables/132-Settings.txt` Settings | 3 | 0 | 0 | 0 | 3 | - | no table: settings-globals / config (UI, session, transaction or config of karbon_log.fmp12; D-23) |

### karbon_tester.fmp12

7 tables, 44 fields.

| Source table | Fields | DDR records | Column | Computed | Dropped | Target | Decision |
|---|---:|---:|---:|---:|---:|---|---|
| `tables/136-connector.txt` connector | 1 | 0 | 0 | 0 | 1 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/134-Deleter.txt` Deleter | 3 | 2 | 0 | 0 | 3 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/133-Globals.txt` Globals | 3 | 1 | 0 | 0 | 3 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/135-meta.txt` meta | 2 | 0 | 0 | 0 | 2 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/132-Rows.txt` Rows | 15 | 529 | 0 | 0 | 15 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/130-Test.txt` Test | 6 | 0 | 0 | 0 | 6 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
| `tables/131-TestData.txt` TestData | 14 | 1 | 0 | 0 | 14 | - | out of scope: developer tooling file (owner, 2026-10-27; D-01) |
