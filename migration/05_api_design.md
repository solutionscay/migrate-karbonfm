# Server Operation / API Design

There is no server (D-36). The interface is a set of **Tauri commands**: Rust functions annotated `#[tauri::command]`, called from the frontend with `invoke("<name>", args)`. They are the only path to SQLite (AP-01). This file replaces HTTP endpoints with command contracts; `02_recommendations.md` confirms the choice.

## Evidence and boundary

| Item | Value |
|---|---|
| Explorer CSVs used | scripts, tables, graph, layouts, value-lists, menus (explorers/*/csv) |
| Selected interface style and reason | Tauri IPC commands (product decision under D-36): one in-process client, typed request/response, no network surface |
| Consumers/clients/integrations | the app's own webview only; outbound SMTP and optional HTTPS integrations from Rust |
| Authentication mechanism | none: single local user (D-04); OD-08-01 optional app lock |
| Authorization design reference | `08_auth_roles.md` (AP-01..AP-12) |
| Unavailable/open contracts | QBO is a stub only (EXT-04, OP-INTEG-04; out of scope, D-02); SMS/chat removed (EXT-05, out of scope, owner 2026-10-28; OD-05-02); binding generation (OD-05-01) |

## Use-case operation catalog

| Domain/use case | Operation | Command | Input | Output | Object files | Transaction/effects | Policy | Errors |
|---|---|---|---|---|---|---|---|---|
| PARTY: Create a person party with contact methods | OP-PARTY-01 | `party_create_person` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/246-wip_Create_Person_data.txt`; `objects/karbon_kontroller.fmp12/scripts/249-wip_Create_Party_V2_data.txt`; `objects/karbon_kontroller.fmp12/scripts/164-wip_Create_Employee_data.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| PARTY: Create an organization party with contact methods | OP-PARTY-02 | `party_create_organization` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/253-wip_Create_Organization_data.txt`; `objects/karbon_kontroller.fmp12/scripts/249-wip_Create_Party_V2_data.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| PARTY: Patch a person or organization and its contact methods | OP-PARTY-03 | `party_update` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/284-wip_Edit_Person_data.txt`; `objects/karbon_kontroller.fmp12/scripts/285-wip_Edit_Party_V2_data.txt`; `objects/karbon_kontroller.fmp12/scripts/224-wip_Edit_Employee_data.txt`; `objects/karbon_kontacts.fmp12/scripts/166-Update_Party_Cached_Data.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| PARTY: Soft or hard delete a party | OP-PARTY-04 | `party_delete` | {id, hard?, reason?, confirm?} | {deleted} | `objects/karbon_kontroller.fmp12/scripts/83-wip_Delete_Party.txt` | one SQLite transaction | AP-01, AP-07 | Validation, NotFound, ConfirmationRequired |
| PARTY: Search parties by name (duplicates, quick find, advanced find) | OP-PARTY-05 | `party_search` | {query, person\|org, full_word_search?, digits_per_word?} | PartySummary[] | `objects/karbon_kontroller.fmp12/scripts/186-Get_Party_data.txt`; `objects/karbon_kontacts.fmp12/scripts/388-Search_for_Duplicate_Organizations.txt`; `objects/karbon_kontacts.fmp12/scripts/389-Search_for_Duplicate_People.txt`; `objects/karbon_kontacts.fmp12/scripts/250-Perform_QuickFind.txt` (+8) | read only | AP-01 | Validation, NotFound |
| PARTY: Find parties by email or phone | OP-PARTY-06 | `party_find_by_contact` | {emails[]\|phones[], prefer_org?} | PartySummary[] | `objects/karbon_kontroller.fmp12/scripts/369-Find_Parties_By_Email_Prefer_Org_List_emails.txt`; `objects/karbon_kontroller.fmp12/scripts/370-Find_Person_By_Email_List_emails.txt`; `objects/karbon_kontroller.fmp12/scripts/371-Find_Current_Employee_By_Email_List_emails.txt`; `objects/karbon_kontroller.fmp12/scripts/653-wip_Find_Parties_By_Phone_List_phones.txt` (+3) | read only | AP-01 | Validation, NotFound |
| PARTY: Create a relation pair | OP-PARTY-07 | `party_relation_create` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/254-wip_Create_Party_Relation_V2_data.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| PARTY: Patch, end or restore a relation pair | OP-PARTY-08 | `party_relation_update` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/508-Edit_Party_Relation_data.txt`; `objects/karbon_kontacts.fmp12/scripts/423-btn_End_Party_Relation_from_Party.txt`; `objects/karbon_kontacts.fmp12/scripts/855-btn_Restore_Party_Relation_from_Party.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| PARTY: Delete a relation pair | OP-PARTY-09 | `party_relation_delete` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/720-Delete_Party_Relation_Id.txt`; `objects/karbon_kontacts.fmp12/scripts/856-btn_Delete_PartyRelation.txt` | one SQLite transaction | AP-01, AP-07 | Validation, NotFound, ConfirmationRequired |
| PARTY: Add customer, lead or employee role | OP-PARTY-10 | `party_role_add` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/705-wip_Add_Customer_to_Party_json.txt`; `objects/karbon_kontroller.fmp12/scripts/229-wip_Create_Customer_V3_data.txt`; `objects/karbon_kontroller.fmp12/scripts/610-wip_Add_Lead_to_Party_json.txt`; `objects/karbon_kontroller.fmp12/scripts/595-wip_Create_Lead_json.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| PARTY: Create organization/person/role in one step | OP-PARTY-11 | `party_create_wizard` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontacts.fmp12/scripts/393-Create_Parties.txt`; `objects/karbon_kontacts.fmp12/scripts/408-OnTimer_Handle_Create_Party_From_System.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| CONTACT: Create or update a phone, address or network address | OP-CONTACT-01 | `contact_method_save` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/197-Create_Contact_Method_data.txt`; `objects/karbon_kontroller.fmp12/scripts/194-Create_or_Edit_Address_data.txt`; `objects/karbon_kontroller.fmp12/scripts/206-Create_Phone_data.txt`; `objects/karbon_kontroller.fmp12/scripts/207-Create_or_Update_Network_Address_data.txt` (+8) | one SQLite transaction | AP-01 | Validation, NotFound |
| CONTACT: Make a contact method primary | OP-CONTACT-02 | `contact_method_set_primary` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/182-Set_Primary_Phone_Id_PartyId.txt`; `objects/karbon_kontroller.fmp12/scripts/183-Set_Primary_Address_Id_PartyId.txt`; `objects/karbon_kontroller.fmp12/scripts/184-Set_Primary_Network_Address_Id_PartyId.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| CONTACT: Toggle billing/shipping/mailing purpose | OP-CONTACT-03 | `contact_method_toggle_purpose` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/185-Toggle_Contact_Method_Purpose_ContactMethodId_ContactMethodP.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| CONTACT: Delete a contact method | OP-CONTACT-04 | `contact_method_delete` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/211-Delete_Address_Id_PartyId.txt`; `objects/karbon_kontroller.fmp12/scripts/212-Delete_Network_Address_Id_PartyId.txt`; `objects/karbon_kontroller.fmp12/scripts/213-Delete_Phone_Id_PartyId.txt` | one SQLite transaction | AP-01, AP-07 | Validation, NotFound, ConfirmationRequired |
| ACTIVITY: Create a note, task, event, meeting, call or email message | OP-ACTIVITY-01 | `activity_create` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/377-Create_Activity.txt`; `objects/karbon_kontroller.fmp12/scripts/389-Create_EmailMessage_data.txt`; `objects/karbon_kontroller.fmp12/scripts/390-Create_Note_data.txt`; `objects/karbon_kontroller.fmp12/scripts/391-Create_Task_data.txt` (+8) | one SQLite transaction | AP-01 | Validation, NotFound |
| ACTIVITY: Patch an activity and its subtype | OP-ACTIVITY-02 | `activity_update` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/541-Edit_Event.txt`; `objects/karbon_kontroller.fmp12/scripts/542-Edit_Activity.txt`; `objects/karbon_kontroller.fmp12/scripts/543-Edit_Note_data__deprecated.txt`; `objects/karbon_kontroller.fmp12/scripts/544-Edit_Task_data__deprecated.txt` (+4) | one SQLite transaction | AP-01, AP-08 | Validation, NotFound |
| ACTIVITY: Delete or discard an activity | OP-ACTIVITY-03 | `activity_delete` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/515-Delete_Activity_Id.txt`; `objects/karbon_kontacts.fmp12/scripts/668-Trigger_Close_Activity_Window.txt`; `objects/karbon_kontacts.fmp12/scripts/693-Button_Delete_and_Close.txt` | one SQLite transaction | AP-01, AP-07 | Validation, NotFound, ConfirmationRequired |
| ACTIVITY: Add, change or remove a party role on an activity | OP-ACTIVITY-04 | `activity_party_set` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/376-Create_ActivityParty_json.txt`; `objects/karbon_kontroller.fmp12/scripts/631-Edit_ActivityParty_json.txt`; `objects/karbon_kontroller.fmp12/scripts/418-Parse_ActivityParty_Array.txt`; `objects/karbon_kontroller.fmp12/scripts/638-Delete_ActivityParty_Id.txt` (+5) | one SQLite transaction | AP-01 | Validation, NotFound |
| ACTIVITY: Mark a task complete or incomplete | OP-ACTIVITY-05 | `task_set_complete` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/690-Mark_Task_Complete_data.txt`; `objects/karbon_kontroller.fmp12/scripts/691-Mark_Task_Incomplete_data.txt`; `objects/karbon_kontacts.fmp12/scripts/758-btn_Mark_Task_Complete.txt`; `objects/karbon_kontacts.fmp12/scripts/759-btn_Mark_Task_Incomplete.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| ACTIVITY: Create or edit a discussion on an entity | OP-ACTIVITY-06 | `discussion_create` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/421-Create_Discussion_v2.txt`; `objects/karbon_kontroller.fmp12/scripts/739-Edit_Discussion.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| ACTIVITY: Add a comment to a discussion | OP-ACTIVITY-07 | `discussion_comment_add` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/422-Create_ActivityComment.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| ACTIVITY: List activities for a party, role and status | OP-ACTIVITY-08 | `activity_list` | {party_id, role_id?, status?} | ActivitySummary[] | `objects/karbon_kontroller.fmp12/scripts/497-Get_ActivityList_by_ActivityParty.txt`; `objects/karbon_kontroller.fmp12/scripts/570-Find_Task_for_C3_Charts_data.txt` | read only | AP-01 | Validation, NotFound |
| ACTIVITY: List and mark notifications read | OP-ACTIVITY-09 | `notification_list_mark_read` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/670-wip_Create_Notification_data.txt`; `objects/karbon_kontroller.fmp12/scripts/445-wip_Mark_Notification_as_Read.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| SALES: Create or update an opportunity (including referring party) | OP-SALES-01 | `opportunity_save` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/602-wip_Create_Opportunity_json.txt`; `objects/karbon_kontacts.fmp12/scripts/722-Timer_Handle_Assign_Referring_Party.txt` | one SQLite transaction | AP-01 | Validation, NotFound, Locked, BookClosed |
| SALES: Change opportunity status | OP-SALES-02 | `opportunity_set_status` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/608-wip_Update_Opportunity_Status_json.txt` | one SQLite transaction | AP-01 | Validation, NotFound, Locked, BookClosed |
| SALES: Totals per status for the chart and list | OP-SALES-03 | `opportunity_status_totals` | {active?} | {status, total}[] | `objects/karbon_kontroller.fmp12/scripts/682-Get_Active_Opportunities_Status_Value.txt`; `objects/karbon_kontacts.fmp12/scripts/767-Set_up_Opportunity_List_View.txt`; `objects/karbon_kontacts.fmp12/scripts/789-Button_Show_Active_and_Sort_by_Status.txt` | read only | AP-01 | Validation, NotFound, Locked, BookClosed |
| SALES: Create or update an invoice with lines | OP-SALES-04 | `invoice_save` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/395-Create_or_Update_Invoice_invoice.txt`; `objects/karbon_kontroller.fmp12/scripts/585-Create_or_Update_Invoice_invoice.txt`; `objects/karbon_kontroller.fmp12/scripts/471-Create_or_Update_SalesLine.txt`; `objects/karbon_kontacts.fmp12/scripts/272-Create_Invoice_Customer_Id.txt` (+2) | one SQLite transaction | AP-01, AP-04, AP-05 | Validation, NotFound, Locked, BookClosed |
| SALES: Create or update a sales receipt with lines | OP-SALES-05 | `sales_receipt_save` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/428-Create_or_Update_SalesReceipt_SalesReceipt.txt`; `objects/karbon_kontroller.fmp12/scripts/584-Create_or_Update_SalesReceipt_SalesReceipt.txt`; `objects/karbon_kontacts.fmp12/scripts/514-Create_Sales_Receipt_Id.txt` | one SQLite transaction | AP-01, AP-04, AP-05 | Validation, NotFound, Locked, BookClosed |
| SALES: Lock (post) or unlock (unpost) a transaction | OP-SALES-06 | `sales_transaction_lock` | {id, lock: bool} | SalesTransaction | `objects/karbon_kontroller.fmp12/scripts/599-Lock_Unlock_Sales_Transaction.txt`; `objects/karbon_kontroller.fmp12/scripts/576-Post_Unpost_Internal_Id_Post.txt`; `objects/karbon_kontacts.fmp12/scripts/335-btn_Unlock_Sales_Transaction_Id.txt` | one SQLite transaction | AP-01, AP-04 | Validation, NotFound, Locked, BookClosed |
| SALES: Delete an unlocked transaction | OP-SALES-07 | `sales_transaction_delete` | {id, confirm} | {deleted} | `objects/karbon_kontroller.fmp12/scripts/560-Delete_SalesTransaction_SalesTransactionId.txt`; `objects/karbon_kontacts.fmp12/scripts/211-btn_Delete_SalesTransaction_From_Customer.txt` | one SQLite transaction | AP-01, AP-04, AP-05, AP-07 | Validation, NotFound, Locked, BookClosed, ConfirmationRequired |
| SALES: Remove lines from an unlocked transaction | OP-SALES-08 | `sales_lines_remove` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/587-Remove_Line_Items_From_Transaction_ListOfLineItemIds.txt`; `objects/karbon_kontroller.fmp12/scripts/586-Remove_Time_Entries_From_The_Invoice_Line.txt` | one SQLite transaction | AP-01, AP-04, AP-05, AP-07 | Validation, NotFound, Locked, BookClosed, ConfirmationRequired |
| SALES: Create or update a payment and its allocations | OP-SALES-09 | `payment_save` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/404-Create_or_Update_Payment_Payment.txt`; `objects/karbon_kontroller.fmp12/scripts/405-Create_or_Update_PaymentLine_PaymentLine.txt`; `objects/karbon_kontacts.fmp12/scripts/221-btn_New_Payment.txt`; `objects/karbon_kontacts.fmp12/scripts/278-Block_Payment_Commit.txt` (+1) | one SQLite transaction | AP-01, AP-06 | Validation, NotFound, Locked, BookClosed |
| SALES: Create, update or deactivate an item | OP-SALES-10 | `item_save` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/412-Create_or_Update_Item_Item.txt`; `objects/karbon_kontroller.fmp12/scripts/519-Get_Item_Object_Id.txt`; `objects/karbon_kontroller.fmp12/scripts/520-Toggle_Active_Item_Item.txt`; `objects/karbon_kontacts.fmp12/scripts/513-btn_Save_Card_-_UtilityItem.txt` | one SQLite transaction | AP-01 | Validation, NotFound, Locked, BookClosed |
| SALES: Monthly posted revenue (view) | OP-SALES-11 | `sales_summary_by_month` | {from?, to?} | {period, revenue, transactions}[] | `objects/karbon_kontroller.fmp12/scripts/583-Rebuild_Posted_Data.txt`; `objects/karbon_kontroller.fmp12/scripts/578-Summarize_Line_Items.txt` | read only | AP-01 | Validation, NotFound, Locked, BookClosed |
| FILE: Add a file from disk | OP-FILE-01 | `file_add` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/617-Create_File_data_v2.txt`; `objects/karbon_kontacts.fmp12/scripts/701-Button_Add_Activity_Attachment_v2.txt` | one SQLite transaction | AP-01, AP-09 | Validation, NotFound |
| FILE: Attach a file to an entity | OP-FILE-02 | `file_attach` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/616-Create_FileAttachment_data_v2.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| FILE: Remove an attachment link | OP-FILE-03 | `file_detach` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/612-Delete_FileAttachment_data.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| TEMPLATE: Merge a template for an activity | OP-TEMPLATE-01 | `template_merge` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/490-Return_Template_Merge_Result_data.txt`; `objects/karbon_kontroller.fmp12/scripts/478-Return_Merge_Field_Names_Data.txt`; `objects/karbon_kontacts.fmp12/scripts/343-Apply_Template.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| TEMPLATE: Create or patch merge fields | OP-TEMPLATE-02 | `merge_fields_save` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/480-Create_or_Patch_Merge_Field_MergeField.txt`; `objects/karbon_kontroller.fmp12/scripts/481-Create_or_Patch_Merge_Fields_MergeField.txt`; `objects/karbon_kontacts.fmp12/scripts/341-Update_Template_Merge_Fields.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| ADMIN: First-run setup | OP-ADMIN-01 | `setup_welcome` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontacts.fmp12/scripts/482-Create_Welcome_Initial_Parties.txt`; `objects/karbon_kontroller.fmp12/scripts/500-Create_User.txt`; `objects/karbon_kontroller.fmp12/scripts/294-Create_System_Record_data.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| ADMIN: Seed reference tables on a fresh database | OP-ADMIN-02 | `reference_seed` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/660-Update_Activity_Types.txt`; `objects/karbon_kontroller.fmp12/scripts/710-Config_ActivityPartyRole.txt`; `objects/karbon_kontroller.fmp12/scripts/607-Seed_Opportunity_Status_table.txt`; `objects/karbon_kontroller.fmp12/scripts/732-Seed_EntityType_table.txt` (+3) | one SQLite transaction | AP-01 | Validation, NotFound |
| ADMIN: Read and change system and user settings; set keychain secrets | OP-ADMIN-03 | `settings_get_update` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/502-Get_System.txt`; `objects/karbon_kontroller.fmp12/scripts/501-Get_User_Id.txt`; `objects/karbon_kontroller.fmp12/scripts/466-Update_Preferences_Preferences.txt` | one SQLite transaction | AP-01, AP-03 | Validation, NotFound |
| ADMIN: Return the single local user | OP-ADMIN-04 | `current_user` | {} | User | `objects/karbon_kontroller.fmp12/scripts/375-Get_User_Object_By_AccountName_AccountName.txt`; `objects/karbon_kontacts.fmp12/scripts/54-Store_UserID_On_Session.txt` | read only | AP-01 | Validation, NotFound |
| ADMIN: Write an error or info log line (internal) | OP-ADMIN-05 | `log_write` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_log.fmp12/scripts/336-Log_data.txt`; `objects/karbon_log.fmp12/scripts/343-Log_data.txt` | one SQLite transaction | AP-01 | Validation, NotFound |
| INTEG: Send an email message by SMTP | OP-INTEG-01 | `email_send` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/521-Send_Email_Message_Activity_json.txt`; `objects/karbon_kontroller.fmp12/scripts/337-Send_Email_Message.txt`; `objects/karbon_kontacts.fmp12/scripts/333-Send_Utility_Email_Message.txt`; `objects/karbon_kontacts.fmp12/scripts/775-Button_Send_Email_Message.txt` | one SQLite transaction | AP-01, AP-02, AP-03, AP-08 | Validation, NotFound |
| INTEG: Address suggestions (optional) | OP-INTEG-02 | `address_suggest` | typed JSON of the entity (camelCase) | the saved entity | `objects/Picker_Module.fmp12/scripts/115-HTTP_request.txt`; `objects/Picker_Module.fmp12/scripts/117-Get_Smarty_Street_Address_Suggestions.txt`; `objects/Picker_Module.fmp12/scripts/198-Get_Google_Places_Address_Suggestions.txt`; `objects/Picker_Module.fmp12/scripts/200-Select_Google_Place_Address.txt` (+2) | one SQLite transaction | AP-01, AP-02, AP-03, AP-12 | Validation, NotFound |
| INTEG: FullContact enrichment (optional) | OP-INTEG-03 | `party_enrich` | typed JSON of the entity (camelCase) | the saved entity | `objects/karbon_kontroller.fmp12/scripts/746-FullContact_Person_Enrich.txt`; `objects/karbon_kontroller.fmp12/scripts/754-FullContact_Company_Enrich.txt`; `objects/karbon_kontacts.fmp12/scripts/865-Search_for_FullContact_Person_Match.txt`; `objects/karbon_kontacts.fmp12/scripts/868-Search_for_FullContact_Company_Match.txt` | one SQLite transaction | AP-01, AP-02, AP-03, AP-12 | Validation, NotFound |
| INTEG: Stub: accounting (QBO) integration, not implemented; no-op | OP-INTEG-04 | `accounting_integration_status` | typed JSON of the entity (camelCase) | the saved entity | new (D-34) | one SQLite transaction | AP-01 | Validation, NotFound |
| IMPORT: Import a FileMaker export into a new database | OP-IMPORT-01 | `import_filemaker_export` | {export_dir (dialog), me_user_id?} | ImportReport | new (D-34) | one SQLite transaction | AP-01, AP-09 | Validation, NotFound |
| IMPORT: Read the rejected-row report of an import | OP-IMPORT-02 | `import_report_get` | {batch_id} | RejectedRow[] | new (D-34) | read only | AP-01 | Validation, NotFound |

No generic per-table CRUD command exists. Full source-script list per command: `05_api_design_appendix.md`.

## Query/read contracts

| Query | Fields/projection | Filters/sort/page | Record policy | Field restrictions | Source found-set semantics | Limits |
|---|---|---|---|---|---|---|
| party_search | id, name_cached, person_ind, organization_ind, primary phone/email/address caches | name words (BR-PARTY-02), kind; sort name; page 50 | deleted_ind = 1 hidden unless include_deleted | none | Get Party find on cached name (K186) | 500 rows max |
| activity_list | id, type, subject, dates, alert, completed, parties | party, role, status (BR-ACTIVITY-10); sort date desc | none | none | K497 find by ActivityParty | page 100 |
| opportunity_status_totals | status, sum(value) | active_ind (BR-SALES-14) | none | none | K682 sub-summaries | n/a |
| sales_summary_by_month | period, revenue, transactions | period range | posted only | none | replaces SumaryByMonth (D-13) | n/a |
| settings_get | system_setting and user columns | n/a | n/a | secrets omitted, has_secret flags (AP-03) | K502 Get System | n/a |
| entity get commands (party, activity, sales transaction, opportunity) | entity + children needed by the screen | by id | n/a | none | Get ... Object scripts (K356, K364, K365, K519) | n/a |

## Mutation/command contracts

| Command | Preconditions | Allowed fields | Invariant owner | Atomic writes | Audit | Forbidden cases/tests |
|---|---|---|---|---|---|---|
| `party_create_person` | BR-PARTY-10 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `party_create_organization` | BR-PARTY-10 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `party_update` | BR-PARTY-06; BR-PARTY-08 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `party_delete` | BR-PARTY-01 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-07 |
| `party_relation_create` | BR-PARTY-03 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `party_relation_update` | BR-PARTY-03; BR-PARTY-04 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `party_relation_delete` | BR-PARTY-03 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-07 |
| `party_role_add` | BR-PARTY-05 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `party_create_wizard` | BR-PARTY-09 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `contact_method_save` | BR-CONTACT-03; BR-CONTACT-04 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `contact_method_set_primary` | BR-PARTY-06; BR-CONTACT-01 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `contact_method_toggle_purpose` | BR-CONTACT-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `contact_method_delete` | BR-CONTACT-05 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-07 |
| `activity_create` | BR-ACTIVITY-01; BR-ACTIVITY-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `activity_update` | BR-ACTIVITY-05; BR-ACTIVITY-08 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-08 |
| `activity_delete` | BR-ACTIVITY-05 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-07 |
| `activity_party_set` | BR-ACTIVITY-02; BR-ACTIVITY-03 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `task_set_complete` | BR-ACTIVITY-04; BR-ACTIVITY-09 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `discussion_create` | BR-ACTIVITY-07 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `discussion_comment_add` | BR-ACTIVITY-06 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `notification_list_mark_read` | BR-ACTIVITY-11 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `opportunity_save` | BR-SALES-13 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `opportunity_set_status` | BR-SALES-12 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `invoice_save` | BR-SALES-01; BR-SALES-02; BR-SALES-03; BR-SALES-04; BR-SALES-05; BR-SALES-08 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-04, T-08-05 |
| `sales_receipt_save` | BR-SALES-01; BR-SALES-02; BR-SALES-03; BR-SALES-04; BR-SALES-05; BR-SALES-08 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-04, T-08-05 |
| `sales_transaction_lock` | BR-SALES-04; BR-SALES-06; BR-SALES-07; BR-SALES-15 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-04 |
| `sales_transaction_delete` | BR-SALES-04; BR-SALES-05; BR-SALES-11 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-04, T-08-05, T-08-07 |
| `sales_lines_remove` | BR-SALES-11 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-04, T-08-05, T-08-07 |
| `payment_save` | BR-SALES-02; BR-SALES-09; BR-SALES-10 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-06 |
| `item_save` | BR-SALES-16 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `file_add` | BR-FILE-01; BR-FILE-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-09 |
| `file_attach` | BR-FILE-03 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `file_detach` | BR-FILE-03 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `template_merge` | BR-TEMPLATE-01 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `merge_fields_save` | BR-TEMPLATE-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `setup_welcome` | BR-ADMIN-01 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `reference_seed` | BR-ACTIVITY-01; BR-ACTIVITY-02; BR-ADMIN-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `settings_get_update` | BR-ADMIN-05 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-03 |
| `log_write` | BR-ADMIN-03 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `email_send` | BR-ACTIVITY-08; BR-ACTIVITY-09; BR-INTEG-01 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-02, T-08-03, T-08-08 |
| `address_suggest` | BR-INTEG-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-02, T-08-03, T-08-12 |
| `party_enrich` | BR-INTEG-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-02, T-08-03, T-08-12 |
| `accounting_integration_status` | BR-INTEG-03 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | validation tests |
| `import_filemaker_export` | BR-PARTY-06; BR-IMPORT-01; BR-IMPORT-02 | typed input struct (serde, deny_unknown_fields) | Rust command | all rows of the operation | BR-PARTY-07 stamps | T-08-09 |

There are no bulk mutations. `Replace Field Contents` has no translation (07).

## Identity and account operations

| Operation | Provider/owner | Flow | Authorization | Audit/negative tests |
|---|---|---|---|---|
| Invite/reset/map user | none | setup_welcome creates the single user; import maps "me" (OP-IMPORT-01) | ROLE-OWNER | T-08-07 |
| Activate/disable account | none | n/a (single user) | n/a | n/a |
| Assign/revoke roles | none | n/a | n/a | n/a |
| Provision/rotate service principal | none | n/a: no service principal | n/a | n/a |
| Store/rotate integration secrets | OS keychain | settings_get_update writes secrets | AP-03 | T-08-03 |

## Authorization matrix

| Operation | Policy ids (08) | Access channel | Deny behavior |
|---|---|---|---|
| `party_create_person` | AP-01 | Tauri IPC from the app window | typed error |
| `party_create_organization` | AP-01 | Tauri IPC from the app window | typed error |
| `party_update` | AP-01 | Tauri IPC from the app window | typed error |
| `party_delete` | AP-01, AP-07 | Tauri IPC from the app window | typed error |
| `party_search` | AP-01 | Tauri IPC from the app window | typed error |
| `party_find_by_contact` | AP-01 | Tauri IPC from the app window | typed error |
| `party_relation_create` | AP-01 | Tauri IPC from the app window | typed error |
| `party_relation_update` | AP-01 | Tauri IPC from the app window | typed error |
| `party_relation_delete` | AP-01, AP-07 | Tauri IPC from the app window | typed error |
| `party_role_add` | AP-01 | Tauri IPC from the app window | typed error |
| `party_create_wizard` | AP-01 | Tauri IPC from the app window | typed error |
| `contact_method_save` | AP-01 | Tauri IPC from the app window | typed error |
| `contact_method_set_primary` | AP-01 | Tauri IPC from the app window | typed error |
| `contact_method_toggle_purpose` | AP-01 | Tauri IPC from the app window | typed error |
| `contact_method_delete` | AP-01, AP-07 | Tauri IPC from the app window | typed error |
| `activity_create` | AP-01 | Tauri IPC from the app window | typed error |
| `activity_update` | AP-01, AP-08 | Tauri IPC from the app window | typed error |
| `activity_delete` | AP-01, AP-07 | Tauri IPC from the app window | typed error |
| `activity_party_set` | AP-01 | Tauri IPC from the app window | typed error |
| `task_set_complete` | AP-01 | Tauri IPC from the app window | typed error |
| `discussion_create` | AP-01 | Tauri IPC from the app window | typed error |
| `discussion_comment_add` | AP-01 | Tauri IPC from the app window | typed error |
| `activity_list` | AP-01 | Tauri IPC from the app window | typed error |
| `notification_list_mark_read` | AP-01 | Tauri IPC from the app window | typed error |
| `opportunity_save` | AP-01 | Tauri IPC from the app window | typed error |
| `opportunity_set_status` | AP-01 | Tauri IPC from the app window | typed error |
| `opportunity_status_totals` | AP-01 | Tauri IPC from the app window | typed error |
| `invoice_save` | AP-01, AP-04, AP-05 | Tauri IPC from the app window | typed error |
| `sales_receipt_save` | AP-01, AP-04, AP-05 | Tauri IPC from the app window | typed error |
| `sales_transaction_lock` | AP-01, AP-04 | Tauri IPC from the app window | typed error |
| `sales_transaction_delete` | AP-01, AP-04, AP-05, AP-07 | Tauri IPC from the app window | typed error |
| `sales_lines_remove` | AP-01, AP-04, AP-05, AP-07 | Tauri IPC from the app window | typed error |
| `payment_save` | AP-01, AP-06 | Tauri IPC from the app window | typed error |
| `item_save` | AP-01 | Tauri IPC from the app window | typed error |
| `sales_summary_by_month` | AP-01 | Tauri IPC from the app window | typed error |
| `file_add` | AP-01, AP-09 | Tauri IPC from the app window | typed error |
| `file_attach` | AP-01 | Tauri IPC from the app window | typed error |
| `file_detach` | AP-01 | Tauri IPC from the app window | typed error |
| `template_merge` | AP-01 | Tauri IPC from the app window | typed error |
| `merge_fields_save` | AP-01 | Tauri IPC from the app window | typed error |
| `setup_welcome` | AP-01 | Tauri IPC from the app window | typed error |
| `reference_seed` | AP-01 | Tauri IPC from the app window | typed error |
| `settings_get_update` | AP-01, AP-03 | Tauri IPC from the app window | typed error |
| `current_user` | AP-01 | Tauri IPC from the app window | typed error |
| `log_write` | AP-01 | Tauri IPC from the app window | typed error |
| `email_send` | AP-01, AP-02, AP-03, AP-08 | Tauri IPC from the app window | typed error |
| `address_suggest` | AP-01, AP-02, AP-03, AP-12 | Tauri IPC from the app window | typed error |
| `party_enrich` | AP-01, AP-02, AP-03, AP-12 | Tauri IPC from the app window | typed error |
| `accounting_integration_status` | AP-01 | Tauri IPC from the app window | typed error |
| `import_filemaker_export` | AP-01, AP-09 | Tauri IPC from the app window | typed error |
| `import_report_get` | AP-01 | Tauri IPC from the app window | typed error |

## External integrations

| Id | Integration | Direction | Command | Contract | Secret reference | Retry/idempotency | Status |
|---|---|---|---|---|---|---|---|
| EXT-01 | SMTP email | outbound | OP-INTEG-01 | lettre (SMTP, STARTTLS/TLS); user or system account | keychain: smtp/<user> | no retry; failure leaves message unsent and logged | in scope (I4) |
| EXT-02 | Google Places / SmartyStreets address suggestions | outbound | OP-INTEG-02 | HTTPS JSON | keychain: google_places, smartystreets | no retry; UI shows no suggestions | optional (I6) |
| EXT-03 | FullContact enrichment | outbound | OP-INTEG-03 | HTTPS JSON, bearer key | keychain: fullcontact | no retry | optional (I6) |
| EXT-04 | QuickBooks Online (accounting) | none | OP-INTEG-04 | stubbed, not implemented: no-op command returning NotImplemented | none | n/a | stubbed, not implemented (owner, D-02) |
| EXT-05 | Twilio SMS / chat | none | none | removed, not implemented | none | n/a | removed: out of scope (owner, 2026-10-28; OD-05-02) |

There is no inbound endpoint. Twilio SMS/chat needed one for its status and inbound-message webhooks (kontroller 731), which a desktop app does not have, so it is removed (owner, 2026-10-28; OD-05-02). No command, type or stub exists for it.

## Accounting integration stub (QBO)

QuickBooks Online is out of scope (owner, 2026-10-27; D-02). The only trace is a no-op so a later integration has a place to attach:

```rust
/// OP-INTEG-04, EXT-04: stubbed, not implemented.
#[tauri::command]
pub fn accounting_integration_status() -> AccountingStatus {
    AccountingStatus { provider: None, implemented: false, message: "Accounting integration is not implemented".into() }
}

/// Extension point for a later integration (not implemented; no implementor ships).
pub trait AccountingProvider {
    fn push_sales_transaction(&self, id: &str) -> Result<ExternalRef, CommandError>;
    fn push_customer(&self, party_role_id: &str) -> Result<ExternalRef, CommandError>;
    fn push_payment(&self, id: &str) -> Result<ExternalRef, CommandError>;
}
```

A later integration implements `AccountingProvider` and records external ids in `external_link` (its type table already has a QBO type, id 5). No QBO table, column, job or screen exists in the plan.

## Background jobs

None (D-10). No `JOB-` ids: the QBO sync scripts with no caller (LedgerLink 243, 708) are removed with QBO (out of scope, D-02). Cache recomputation runs inside the import command.

## Confirmations

Two-step destructive commands (AP-07): the first call without `confirm` returns `ConfirmationRequired { summary }` (for example "deletes 1 invoice, 3 lines, 1 attachment"); the UI shows it and resends with `confirm: true`. This models the FileMaker "Delete and Close" and "discard empty activity" dialogs (BR-ACTIVITY-05) and the payment allocation alert (BR-SALES-09 is a hard refusal, not a confirmation).

## Errors, concurrency, and observability

| Concern | Contract |
|---|---|
| Validation/domain errors | `CommandError { code, message, field? }`, code in Validation, NotFound, Locked, BookClosed, AlreadySent, ConfirmationRequired, Integration, Storage, Internal (D-25) |
| Authentication/authorization denial | unregistered commands are rejected by Tauri before reaching Rust (AP-01) |
| Conflict/locking/retry | single user, single writer: SQLite `BEGIN IMMEDIATE`, busy_timeout 5 s; no optimistic locking needed |
| Partial external failure/compensation | email send: the DB marks sent only after SMTP success; no other external writes |
| Idempotency | post/unpost and role add are idempotent (BR-SALES-06, BR-PARTY-05); creates take a client-generated UUID so a retried create is a no-op |
| Rate/size/pagination limits | file 20 MB (BR-FILE-01); list pages 50-100 |
| Audit actor/reason/correlation | creation/modification stamps (BR-PARTY-07); log line per error with command name (BR-ADMIN-03) |
| Sensitive-field logging/masking | secrets never logged (AP-03, D-30); tax ids are ordinary text fields with no masking (owner, 2026-10-28; D-09) |
| Money values | every money field of every command input and output is an integer number of cents (`i64`, JSON number); the UI converts dollars to cents and back; no command takes or returns a decimal (OD-04-02, owner 2026-10-28) |

## Traceability and tests

Each command lists its source scripts in the appendix and its rules in 07. Required tests: happy path and boundaries per rule, the AP negative tests (T-08-*), transaction rollback on a failing second write, idempotent retry, SMTP contract test against a local test server, and response projection tests for secrets.

## Unresolved operations

| Source behavior | Missing evidence | Risk | Required proof/owner |
|---|---|---|---|
| Whether commands use typed bindings generated from Rust (specta/tauri-specta) or hand-written TypeScript types. | not in the DDR | feature gap | OD-05-01, Tech lead |
| btn: Apply Retainer (`objects/karbon_kontacts.fmp12/scripts/223-btn_Apply_Retainer.txt`) | no command assigned; the called kontroller script 328 is deleted and no other object holds the logic (09, G-01) | behavior lost | OD-07-04, Accounting owner |
