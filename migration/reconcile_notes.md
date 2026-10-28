# Reconcile notes

Written by the reconcile pass over 00-08 (2026-10-27) and rerun over 00-09 on 2026-10-28 after three owner decisions. Rerun after any artifact change.

## QuickBooks Online removal (owner decision, 2026-10-27; D-02)

- karbon_LedgerLink: all 38 QBO mirror tables and the rest of the file are not migrated (03 "Removed" lists every object kind).
- 28 QBO sync-state columns dropped from the core tables:
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
- QBO scripts, functions, layouts and screen controls: removed (07, 06 appendices; 06 "QBO controls not built").
- Rules changed: BR-SALES-02, BR-SALES-04, BR-SALES-05, BR-SALES-11, BR-SALES-15; policy AP-05. Rule added: BR-INTEG-03 (stub).
- Stub kept: EXT-04 "stubbed, not implemented", OP-INTEG-04 `accounting_integration_status` (I6). OD-02-01 closed.

## Owner decisions, 2026-10-28

- Money storage (OD-04-02, closed): integer minor units. The 22 money columns in 04 are INTEGER cents (customer 4, employee 2, item 2, opportunity 1, payment 1, payment_line 1, sales_line 3, sales_transaction 7, vendor 1). Arithmetic in Rust on integers; conversion only at the UI and import edges (BR-IMPORT-01, half away from zero). BR-SALES-01 changed: unit prices are cents, so the source's 7-decimal unit price is not kept. 02 R-02/S-03, 03 tests and validation matrix, 05 money contract, 01 D-18 updated.
- Twilio SMS/chat (OD-05-02, closed; EXT-05): dropped, out of scope, removed like QuickBooks (03 "Removed: Twilio SMS/chat"). 04: tables chat, chat_consent, chat_message, 2 indexes and 3 system_setting Twilio columns removed; 04 appendix: 3 tables, 51 fields, 12 relationships marked removed. 07 appendix: 15 scripts and 24 table-logic lines marked removed. 06 appendix: 6 layouts marked removed. 08 appendix: 3 table rows removed. activity.chat_ind, activity_type 9 and phone.sms_ind stay as data.
- Tax ids (organization.federal_tax_id_number, vendor.tax_id): plain TEXT, no encryption, masking or extra access rule (demo). Recorded in 01 D-09, 04, 07 BR-IMPORT-02 and an 08 field-level policy row; the security-review question on them is closed.
- UI stack (owner decision, 2026-10-28): server-rendered HTML, no React or TypeScript. Tauri 2 window on a local axum server on 127.0.0.1 (askama, htmx, Alpine.js for small bits, Tailwind + DaisyUI, Chart.js; rusqlite with plain SQL). 05: every one of the 51 commands has a route (3 are internal) and returns a page or an htmx fragment. 06: every screen UI-01..UI-21 has a page route; UI-21 is the static chat mock. 08: AP-01/02/03/09/10 rewritten for routes; new AP-13 (local server guard, I1); test T-08-14. OD-05-01 closed. 02: React/TypeScript items removed from the stack, the risks and the spikes; new R-07, R-08 and S-05.
- Owner change (2026-10-28): no LAN sharing. The host-mode increment I7 and everything added for it are removed (AP-14, BR-ADMIN-06, OP-ADMIN-06/07, UI-22, T-08-15, OD-08-02, the LAN risk). The app is single-user on one computer; the server listens on 127.0.0.1 only.
- Owner answers (2026-10-28): 27 D- questions answered (D-03, D-07, D-09, D-10..D-13, D-15..D-30, D-33, D-37..D-39) and every open OD closed (OD-02-02, OD-03-01, OD-04-01, OD-04-03, OD-06-01, OD-07-01..OD-07-07, OD-08-01). Still open: D-14 for the non-Twilio integrations, D-31, D-32. Schema: foreign keys enforced (9 needs-profile ones added; 6 reversed primary-key references and 4 references on polymorphic or list columns removed); merge_field_config dropped; book_close_date and alert_ind/completed_ind dropped; task.completed_at, tax_rate_bp (settings and per transaction) added; *_sha256 became *_path. Rules: BR-SALES-04 and AP-04 removed (kept as removed rows); BR-SALES-18 and OP-SALES-12 (Apply Retainer) added; OP-PARTY-04 is party_archive. Demo seed: 04_demo_seed.sql, checked against design/.
- Platforms (owner decision, 2026-10-28): macOS, Windows and Linux from one Tauri codebase, unsigned demo builds; D-37 and every "macOS first" / "Linux later" mention updated in 01, 02, 03, 06, 09 and the README.

## Check results (2026-10-28, over 00-09, after the platform decision)

- unknown ids: 0
- ODs missing from their owner: 0
- closed ODs still listed open in 03: 0
- open ODs missing from the 03 list: 0
- 03 register rows still "open" for closed ODs: 0
- table.column refs not in 04: 0
- object paths missing: 0
- tier1 rules not in exactly one increment: 0
- policies not in exactly one increment: 0
- removed policies still in an increment: 0
- removed rules still in an increment: 0
- commands not in an increment: 0
- screens not in an increment: 0
- QBO mentions not marked removed/stub: 0
- SMS/chat mentions not marked removed: 0
- "deferred" SMS/chat dispositions left: 0
- chat tables or Twilio columns in 04: 0
- QBO columns left in 04: 0
- stub present: 0
- money columns not INTEGER: 0
- decimal-money wording left (rust_decimal, REAL, 7-dp storage): 0
- tax id columns not plain TEXT: 0
- React/TypeScript/IPC wording left: 0
- OPs without a route in the 05 catalog: 0
- screens without a page route: 0
- ops with no rule in 07: 0
- host-mode / LAN wording left (owner change 2026-10-28): 0
- book-close / hard-delete wording left: 0
- active rows citing AP-04 / BR-SALES-04 / T-08-04: 0
- needs-profile FKs still pending in 04: 0
- sha256 / alert_ind / completed_ind / merge_field_config columns in 04: 0
- demo seed (04_demo_seed.sql) applies and matches design/: 0
- platform wording naming only some platforms: 0
- Windows missing from the stack/plan: 0

The platform checks (no wording that names only some platforms; Windows present in 01, 02, 03, 06 and the README) were added on 2026-10-28.
