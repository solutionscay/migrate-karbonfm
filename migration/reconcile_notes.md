# Reconcile notes

Written by the reconcile pass over 00-08. Rerun after any artifact change.

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

## Check results

- unknown ids: 0
- ids missing from their owner: 0
- table.column refs not in 04: 5 - ('04_database_schema_appendix.md', 'activity.activity_id'); ('04_database_schema_appendix.md', 'activity_comment.user_id_deprecated'); ('04_database_schema_appendix.md', 'customer.one'); ('04_database_schema_appendix.md', 'discussion.entity_id_deprecated'); ('04_database_schema_appendix.md', 'discussion.one')
- object paths missing: 0
- tier1 rules not in exactly one increment: 0
- policies not in exactly one increment: 0
- commands not in an increment: 0
- screens not in an increment: 0
- rules citing unknown commands: 0
- screens citing unknown commands: 0
- commands with no rule: 0
- QBO mentions not marked removed/stub: 0
- QBO columns left in 04: 0
- stub present: 0
