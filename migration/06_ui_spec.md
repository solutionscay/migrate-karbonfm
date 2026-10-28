# UI Evidence and Target Specification

Desktop app for macOS and Linux, offline, one window with a sidebar (D-36, D-37). The window is a Tauri webview on the app's local axum server, and every screen is server-rendered HTML (owner decision, 2026-10-28): askama templates, htmx for interaction (each button or form requests an HTML fragment and htmx swaps it in), Alpine.js only for small client-side bits, Tailwind + DaisyUI for styling, Chart.js for charts. There is no React or TypeScript. Routes are listed in 05. No screenshots or recordings exist, so no screen is implementation-ready from source evidence; UI-18 (new import screen) is specified here in full.

## Evidence

| Source | Coverage | Missing |
|---|---|---|
| Layout object files | 382 layouts; 713 hide rules, 140 conditional formats, 209 triggers, 1589 buttons | theme styling, look |
| Layout explorer CSVs | purpose, context, rules per layout | item-by-item reading of 2 shares (Step 3 caveat) |
| Script explorer CSVs | what buttons and triggers do | callers may be wrong in places |
| Menu explorer CSVs | menu commands and overrides | removed items not knowable |
| Screenshots/recordings | none | all visual design (OD-06-01) |
| Discovery/design decisions | D-23, D-26, D-28, D-37, D-38 | design system |

Overall readiness: behavior-ready for the core workflows, inventory-only for list and admin screens.

## Source screen inventory

Main screens below; every layout has a row in `06_ui_spec_appendix.md`.

| Layout file | Context | Purpose | Portals/buttons | Hide/CF | Screenshot | Readiness | Disposition |
|---|---|---|---|---|---|---|---|
| `objects/karbon_kontacts.fmp12/layouts/133-Welcome_Wizard.txt` | UtilityParty (base UtilityParty, karbon_kontacts.fmp12) | form | 0 portals, 4 buttons | 4 hide, 0 CF | none | behavior-ready | UI-01 |
| `objects/karbon_kontacts.fmp12/layouts/10-Home.txt` | session (base session, karbon_kontacts.fmp12) | navigation | 0 portals, 5 buttons | 2 hide, 1 CF | none | inventory-only | UI-02 |
| `objects/karbon_kontacts.fmp12/layouts/101-User_Home.txt` | User (base User, karbon_data.fmp12) | dashboard | 3 portals, 22 buttons | 8 hide, 2 CF | none | inventory-only | UI-02 |
| `objects/karbon_kontacts.fmp12/layouts/21-Main_Menu.txt` | connector (base connector, karbon_kontacts.fmp12) | navigation | 0 portals, 9 buttons | 0 hide, 1 CF | none | inventory-only | UI-02 |
| `objects/karbon_kontacts.fmp12/layouts/47-Parties.txt` | Party (base Party, karbon_data.fmp12) | list | 0 portals, 20 buttons | 7 hide, 2 CF | none | behavior-ready | UI-03 |
| `objects/karbon_kontacts.fmp12/layouts/40-Customers.txt` | Party_PartyRole_Customer (base Customer, karbon_data.fmp12) | list | 0 portals, 22 buttons | 4 hide, 2 CF | none | behavior-ready | UI-03 |
| `objects/karbon_kontacts.fmp12/layouts/177-Party_Find.txt` | Party (base Party, karbon_data.fmp12) | utility | 0 portals, 8 buttons | 5 hide, 1 CF | none | behavior-ready | UI-03 |
| `objects/karbon_kontacts.fmp12/layouts/178-Customer_Find.txt` | Party_PartyRole_Customer (base Customer, karbon_data.fmp12) | utility | 0 portals, 8 buttons | 5 hide, 1 CF | none | behavior-ready | UI-03 |
| `objects/karbon_kontacts.fmp12/layouts/50-Party_Detail.txt` | Party (base Party, karbon_data.fmp12) | form | 4 portals, 45 buttons | 22 hide, 2 CF | none | behavior-ready | UI-04 |
| `objects/karbon_kontacts.fmp12/layouts/51-Customer_Detail.txt` | Party_PartyRole_Customer (base Customer, karbon_data.fmp12) | form | 5 portals, 39 buttons | 23 hide, 2 CF | none | behavior-ready | UI-04 |
| `objects/karbon_kontacts.fmp12/layouts/49-Contact_Card.txt` | Party (base Party, karbon_data.fmp12) | dialog | 3 portals, 33 buttons | 10 hide, 10 CF | none | behavior-ready | UI-04 |
| `objects/karbon_kontacts.fmp12/layouts/124-Card_-_UtilityParty.txt` | UtilityParty (base UtilityParty, karbon_kontacts.fmp12) | dialog | 2 portals, 27 buttons | 41 hide, 0 CF | none | behavior-ready | UI-05 |
| `objects/Picker_Module.fmp12/layouts/1-Contact_Picker_-_No_Results_With_Selection.txt` | Party (base Party, karbon_data.fmp12) | dialog | 1 portals, 6 buttons | 1 hide, 0 CF | none | behavior-ready | UI-06 |
| `objects/Picker_Module.fmp12/layouts/13-Contact_Picker_-_No_Results_No_Selection.txt` | Party (base Party, karbon_data.fmp12) | dialog | 0 portals, 5 buttons | 1 hide, 0 CF | none | behavior-ready | UI-06 |
| `objects/Picker_Module.fmp12/layouts/14-Contact_Picker_-_Results_With_Selection.txt` | Party (base Party, karbon_data.fmp12) | dialog | 1 portals, 8 buttons | 2 hide, 0 CF | none | behavior-ready | UI-06 |
| `objects/Picker_Module.fmp12/layouts/15-Contact_Picker_-_Results_No_Selection.txt` | Party (base Party, karbon_data.fmp12) | dialog | 0 portals, 7 buttons | 3 hide, 0 CF | none | behavior-ready | UI-06 |
| `objects/Picker_Module.fmp12/layouts/34-Address_Picker.txt` | VirtualListAddressPicker (base VIRTUALLISTS_Rows, Picker_Mod | dialog | 0 portals, 3 buttons | 0 hide, 0 CF | none | behavior-ready | UI-06 |
| `objects/Picker_Module.fmp12/layouts/37-Email_Address_Picker.txt` | VIRTUALLISTS_Rows (base VIRTUALLISTS_Rows, Picker_Module.fmp | dialog | 0 portals, 14 buttons | 7 hide, 0 CF | none | behavior-ready | UI-06 |
| `objects/Picker_Module.fmp12/layouts/49-Phone_Picker.txt` | VIRTUALLISTS_Rows (base VIRTUALLISTS_Rows, Picker_Module.fmp | dialog | 0 portals, 5 buttons | 1 hide, 0 CF | none | behavior-ready | UI-06 |
| `objects/karbon_kontacts.fmp12/layouts/78-Card_-_Utility_Note.txt` | UtilityNote (base UtilityNote, karbon_kontacts.fmp12) | dialog | 0 portals, 3 buttons | 0 hide, 0 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/79-Note_Detail.txt` | Note (base Note, karbon_data.fmp12) | dialog | 1 portals, 8 buttons | 4 hide, 1 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/82-Card_-_Utility_Meeting.txt` | UtilityMeeting (base UtilityMeeting, karbon_kontacts.fmp12) | dialog | 0 portals, 3 buttons | 0 hide, 0 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/87-Card_-_Utility_Task.txt` | UtilityTask (base UtilityTask, karbon_kontacts.fmp12) | dialog | 0 portals, 4 buttons | 0 hide, 0 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/91-Task_Detail.txt` | Task (base Task, karbon_data.fmp12) | dialog | 3 portals, 11 buttons | 6 hide, 1 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/92-Meeting_Detail.txt` | Meeting (base Meeting, karbon_data.fmp12) | dialog | 2 portals, 8 buttons | 2 hide, 1 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/171-Call_Detail.txt` | Call (base Call, karbon_data.fmp12) | dialog | 1 portals, 8 buttons | 3 hide, 1 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/125-Activity_List.txt` | Activity (base Activity, karbon_data.fmp12) | list | 0 portals, 19 buttons | 9 hide, 2 CF | none | behavior-ready | UI-07 |
| `objects/karbon_kontacts.fmp12/layouts/105-Card_-_Utility_EmailMessage.txt` | UtilityEmailMessage (base UtilityEmailMessage, karbon_kontac | dialog | 3 portals, 17 buttons | 5 hide, 1 CF | none | behavior-ready | UI-08 |
| `objects/karbon_kontacts.fmp12/layouts/106-Card_-_Utility_EmailMessage_-_Send.txt` | UtilityEmailMessage (base UtilityEmailMessage, karbon_kontac | dialog | 0 portals, 1 buttons | 0 hide, 0 CF | none | behavior-ready | UI-08 |
| `objects/karbon_kontacts.fmp12/layouts/118-Card_-_Utility_EmailMessage_-_Read_Only.txt` | UtilityEmailMessage (base UtilityEmailMessage, karbon_kontac | dialog | 0 portals, 1 buttons | 1 hide, 0 CF | none | behavior-ready | UI-08 |
| `objects/karbon_kontacts.fmp12/layouts/167-Email_Message_-_View_Only.txt` | EmailMessage (base EmailMessage, karbon_data.fmp12) | dialog | 1 portals, 11 buttons | 3 hide, 0 CF | none | behavior-ready | UI-08 |
| `objects/karbon_kontacts.fmp12/layouts/176-Email_Message_-_Edit.txt` | EmailMessage (base EmailMessage, karbon_data.fmp12) | dialog | 3 portals, 27 buttons | 8 hide, 6 CF | none | behavior-ready | UI-08 |
| `objects/karbon_kontacts.fmp12/layouts/93-Discussion_Detail.txt` | Discussion (base Discussion, karbon_data.fmp12) | dialog | 1 portals, 5 buttons | 0 hide, 0 CF | none | behavior-ready | UI-09 |
| `objects/karbon_kontacts.fmp12/layouts/187-Card_-_Manage_Subscribers.txt` | UtilityDiscussion (base UtilityDiscussion, karbon_kontacts.f | dialog | 1 portals, 6 buttons | 1 hide, 0 CF | none | behavior-ready | UI-09 |
| `objects/karbon_kontacts.fmp12/layouts/153-Opportunities.txt` | Opportunity (base Opportunity, karbon_data.fmp12) | list | 0 portals, 25 buttons | 7 hide, 2 CF | none | behavior-ready | UI-10 |
| `objects/karbon_kontacts.fmp12/layouts/165-Opportunity_Find.txt` | Opportunity (base Opportunity, karbon_data.fmp12) | utility | 0 portals, 8 buttons | 5 hide, 1 CF | none | behavior-ready | UI-10 |
| `objects/karbon_kontacts.fmp12/layouts/184-Chat.txt` | Chat (base Chat, karbon_data.fmp12) | form | 0 portals, 7 buttons | 11 hide, 3 CF | none | static mock | UI-21 |
| `objects/karbon_kontacts.fmp12/layouts/156-Opportunity_Detail.txt` | Opportunity (base Opportunity, karbon_data.fmp12) | form | 4 portals, 37 buttons | 25 hide, 3 CF | none | behavior-ready | UI-11 |
| `objects/karbon_kontacts.fmp12/layouts/162-Opportunity_Detail_-_Card.txt` | Opportunity (base Opportunity, karbon_data.fmp12) | dialog | 0 portals, 1 buttons | 0 hide, 0 CF | none | behavior-ready | UI-11 |
| `objects/karbon_kontacts.fmp12/layouts/69-Invoice.txt` | Invoice (base Invoice, karbon_data.fmp12) | form | 3 portals, 25 buttons | 18 hide, 5 CF | none | behavior-ready | UI-12 |
| `objects/karbon_kontacts.fmp12/layouts/94-SalesReceipt.txt` | SalesReceipt (base SalesReceipt, karbon_data.fmp12) | form | 2 portals, 18 buttons | 13 hide, 4 CF | none | behavior-ready | UI-12 |
| `objects/karbon_kontacts.fmp12/layouts/143-Invoice_-_View_Only.txt` | Invoice (base Invoice, karbon_data.fmp12) | form | 3 portals, 11 buttons | 13 hide, 0 CF | none | behavior-ready | UI-12 |
| `objects/karbon_kontacts.fmp12/layouts/145-SalesReceipt_-_View_Only.txt` | SalesReceipt (base SalesReceipt, karbon_data.fmp12) | form | 2 portals, 9 buttons | 8 hide, 2 CF | none | behavior-ready | UI-12 |
| `objects/karbon_kontacts.fmp12/layouts/84-Sales_List.txt` | SalesTransaction (base SalesTransaction, karbon_data.fmp12) | list | 0 portals, 26 buttons | 9 hide, 2 CF | none | inventory-only | UI-13 |
| `objects/karbon_kontacts.fmp12/layouts/126-Payments_List.txt` | Payment (base Payment, karbon_data.fmp12) | list | 0 portals, 19 buttons | 4 hide, 2 CF | none | inventory-only | UI-13 |
| `objects/karbon_kontacts.fmp12/layouts/85-Card_-_New_Payment.txt` | Payment (base Payment, karbon_data.fmp12) | dialog | 2 portals, 7 buttons | 5 hide, 0 CF | none | behavior-ready | UI-14 |
| `objects/karbon_kontacts.fmp12/layouts/86-Items.txt` | Item (base Item, karbon_data.fmp12) | list | 1 portals, 21 buttons | 5 hide, 3 CF | none | inventory-only | UI-15 |
| `objects/karbon_kontacts.fmp12/layouts/135-Card_-_Items.txt` | Item (base Item, karbon_data.fmp12) | dialog | 1 portals, 1 buttons | 0 hide, 1 CF | none | inventory-only | UI-15 |
| `objects/karbon_kontacts.fmp12/layouts/136-Card_-_UtilityItem.txt` | UtilityItem (base UtilityItem, karbon_kontacts.fmp12) | dialog | 0 portals, 1 buttons | 2 hide, 0 CF | none | inventory-only | UI-15 |
| `objects/karbon_kontacts.fmp12/layouts/111-Templates.txt` | Template (base Template, karbon_data.fmp12) | list | 0 portals, 19 buttons | 2 hide, 2 CF | none | inventory-only | UI-16 |
| `objects/karbon_kontacts.fmp12/layouts/112-Template_Detail.txt` | Template (base Template, karbon_data.fmp12) | form | 1 portals, 3 buttons | 1 hide, 1 CF | none | inventory-only | UI-16 |
| `objects/karbon_kontacts.fmp12/layouts/114-MergeField.txt` | MergeField (base MergeFieldConfig, karbon_data.fmp12) | form | 0 portals, 0 buttons | 0 hide, 0 CF | none | inventory-only | UI-16 |
| `objects/karbon_kontacts.fmp12/layouts/70-Admin_Dashboard.txt` | session (base session, karbon_kontacts.fmp12) | form | 0 portals, 13 buttons | 6 hide, 1 CF | none | inventory-only | UI-17 |
| `objects/karbon_kontacts.fmp12/layouts/71-Admin_User_List.txt` | User (base User, karbon_data.fmp12) | list | 0 portals, 19 buttons | 2 hide, 2 CF | none | inventory-only | UI-17 |
| `objects/karbon_kontacts.fmp12/layouts/128-Admin_Integrations.txt` | System (base System, karbon_data.fmp12) | form | 0 portals, 12 buttons | 2 hide, 1 CF | none | inventory-only | UI-17 |
| `objects/karbon_kontacts.fmp12/layouts/134-Admin_User_Detail.txt` | User (base User, karbon_data.fmp12) | form | 0 portals, 22 buttons | 6 hide, 2 CF | none | inventory-only | UI-17 |
| `objects/karbon_kontacts.fmp12/layouts/122-Card_-_Utility_User.txt` | UtilityUser (base UtilityUser, karbon_kontacts.fmp12) | dialog | 1 portals, 7 buttons | 5 hide, 0 CF | none | inventory-only | UI-17 |
| `objects/karbon_log.fmp12/layouts/20-Settings.txt` | Settings (base Settings, karbon_log.fmp12) | form | 0 portals, 0 buttons | 0 hide, 0 CF | none | inventory-only | UI-17 |
| `objects/karbon_kontacts.fmp12/layouts/142-Card_-_FilesList.txt` | File (base File, karbon_data.fmp12) | dialog | 0 portals, 5 buttons | 0 hide, 0 CF | none | inventory-only | UI-19 |
| `objects/karbon_kontacts.fmp12/layouts/185-Thumbnails.txt` | File (base File, karbon_data.fmp12) | dialog | 0 portals, 3 buttons | 0 hide, 0 CF | none | inventory-only | UI-19 |
| `objects/karbon_kontacts.fmp12/layouts/103-SalesLine_pdf.txt` | SalesLine (base SalesLine, karbon_data.fmp12) | report-print | 0 portals, 0 buttons | 4 hide, 0 CF | none | inventory-only | UI-20 |
| `objects/karbon_charts.fmp12/layouts/25-SalesOpp_Chart.txt` | charts (base charts, karbon_charts.fmp12) | dashboard | 0 portals, 0 buttons | 0 hide, 0 CF | none | inventory-only | UI-20 |
| `objects/karbon_charts.fmp12/layouts/22-Opportunity_Referral_Report.txt` | f_Opportunity (base Opportunity, karbon_data.fmp12) | report-print | 0 portals, 0 buttons | 0 hide, 0 CF | none | inventory-only | UI-20 |

## Navigation and workflows

| User workflow | Entry/route | Steps and states | Commands (routes in 05) | Access | Source trace | Acceptance evidence |
|---|---|---|---|---|---|---|
| First run | /setup (UI-01) | organization -> person -> settings -> done | setup_welcome, reference_seed | ROLE-OWNER | `objects/karbon_kontacts.fmp12/scripts/482-Create_Welcome_Initial_Parties.txt` | BR-ADMIN-01 tests |
| Import | /import (UI-18) | pick export folder -> map "me" -> run -> report | import_filemaker_export, import_report_get | AP-09 | new (D-34) | BR-IMPORT-01 tests |
| Create a party | /parties/new (UI-05) | type name -> duplicate check -> details -> role -> save | party_search, party_create_wizard | ROLE-OWNER | `objects/karbon_kontacts.fmp12/scripts/393-Create_Parties.txt` | BR-PARTY-09 |
| Log a task and complete it | /parties/:id -> task card (UI-07) | new -> assign -> complete/incomplete | activity_create, task_set_complete | ROLE-OWNER | `objects/karbon_kontroller.fmp12/scripts/690-Mark_Task_Complete_data.txt` | BR-ACTIVITY-04 |
| Send an email | email card (UI-08) | compose -> template -> send -> read only | activity_create, template_merge, email_send | AP-08 | `objects/karbon_kontroller.fmp12/scripts/521-Send_Email_Message_Activity_json.txt` | BR-INTEG-01 |
| Invoice and payment | /sales/:id (UI-12) -> payment (UI-14) | draft -> lines -> lock -> payment | invoice_save, sales_transaction_lock, payment_save | AP-04..AP-06 | `objects/karbon_kontacts.fmp12/scripts/270-SalesLine_Amount_OnObjectExit_Trigger.txt` | BR-SALES-01..09 |
| Opportunity pipeline | /opportunities (UI-10) | list -> chart -> status change | opportunity_status_totals, opportunity_set_status | ROLE-OWNER | `objects/karbon_kontroller.fmp12/scripts/608-wip_Update_Opportunity_Status_json.txt` | BR-SALES-12, 14 |

## Target page specifications

### UI-01 Welcome setup

- Readiness: behavior-ready
- Use case and users: First-run wizard: organization, first person, user, settings; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/133-Welcome_Wizard.txt`
- Page: `GET /setup` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `GET /setup; POST /setup` (OP-ADMIN-01 `setup_welcome`), `internal: runs at first start, no route` (OP-ADMIN-02 `reference_seed`)
- Rules shown or enforced: BR-ACTIVITY-01, BR-ACTIVITY-02, BR-ADMIN-01, BR-ADMIN-02
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-02 Home

- Readiness: inventory-only
- Use case and users: User home: my tasks, recent activities, notifications; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/10-Home.txt`, `objects/karbon_kontacts.fmp12/layouts/101-User_Home.txt`, `objects/karbon_kontacts.fmp12/layouts/21-Main_Menu.txt`
- Page: `GET /` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `GET /activities?party_id=&role_id=&status=&page=` (OP-ACTIVITY-08 `activity_list`), `GET /notifications; POST /notifications/read` (OP-ACTIVITY-09 `notification_list_mark_read`), `POST /tasks/{id}/complete` (OP-ACTIVITY-05 `task_set_complete`)
- Rules shown or enforced: BR-ACTIVITY-04, BR-ACTIVITY-09, BR-ACTIVITY-10, BR-ACTIVITY-11
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-03 Parties

- Readiness: behavior-ready
- Use case and users: Party and customer lists with quick find and advanced find; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/47-Parties.txt`, `objects/karbon_kontacts.fmp12/layouts/40-Customers.txt`, `objects/karbon_kontacts.fmp12/layouts/177-Party_Find.txt`, `objects/karbon_kontacts.fmp12/layouts/178-Customer_Find.txt`
- Page: `GET /parties` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `GET /parties?q=&kind=&page=` (OP-PARTY-05 `party_search`)
- Rules shown or enforced: BR-PARTY-02
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-04 Party detail

- Readiness: behavior-ready
- Use case and users: Person or organization: contact methods, roles, relations, activities, opportunities, sales, files; customer view; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/50-Party_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/51-Customer_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/49-Contact_Card.txt`
- Page: `GET /parties/{id}` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `PATCH /parties/{id}` (OP-PARTY-03 `party_update`), `DELETE /parties/{id}` (OP-PARTY-04 `party_delete`), `POST /parties/{id}/relations` (OP-PARTY-07 `party_relation_create`), `PATCH /relations/{id}` (OP-PARTY-08 `party_relation_update`), `DELETE /relations/{id}` (OP-PARTY-09 `party_relation_delete`), `POST /parties/{id}/roles` (OP-PARTY-10 `party_role_add`), `POST /parties/{id}/contact-methods; PUT /contact-methods/{id}` (OP-CONTACT-01 `contact_method_save`), `POST /contact-methods/{id}/primary` (OP-CONTACT-02 `contact_method_set_primary`), `POST /contact-methods/{id}/purposes/{type_id}` (OP-CONTACT-03 `contact_method_toggle_purpose`), `DELETE /contact-methods/{id}` (OP-CONTACT-04 `contact_method_delete`), `GET /activities?party_id=&role_id=&status=&page=` (OP-ACTIVITY-08 `activity_list`), `POST /attachments` (OP-FILE-02 `file_attach`)
- Rules shown or enforced: BR-ACTIVITY-10, BR-CONTACT-01, BR-CONTACT-02, BR-CONTACT-03, BR-CONTACT-04, BR-CONTACT-05, BR-FILE-03, BR-PARTY-01, BR-PARTY-03, BR-PARTY-04, BR-PARTY-05, BR-PARTY-06, BR-PARTY-08
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment) / confirmation (AP-07)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: two-step route: a confirmation dialog fragment, then the same request with confirm=true (AP-07)

### UI-05 Create party

- Readiness: behavior-ready
- Use case and users: Create-party wizard with duplicate check; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/124-Card_-_UtilityParty.txt`
- Page: `GET /parties/new` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /parties/wizard` (OP-PARTY-11 `party_create_wizard`), `GET /parties?q=&kind=&page=` (OP-PARTY-05 `party_search`)
- Rules shown or enforced: BR-PARTY-02, BR-PARTY-09
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-06 Pickers

- Readiness: behavior-ready
- Use case and users: Contact, email, phone and address pickers; the single local user (ROLE-OWNER).
- Source layouts: `objects/Picker_Module.fmp12/layouts/1-Contact_Picker_-_No_Results_With_Selection.txt`, `objects/Picker_Module.fmp12/layouts/13-Contact_Picker_-_No_Results_No_Selection.txt`, `objects/Picker_Module.fmp12/layouts/14-Contact_Picker_-_Results_With_Selection.txt`, `objects/Picker_Module.fmp12/layouts/15-Contact_Picker_-_Results_No_Selection.txt`, `objects/Picker_Module.fmp12/layouts/34-Address_Picker.txt`, `objects/Picker_Module.fmp12/layouts/37-Email_Address_Picker.txt`, `objects/Picker_Module.fmp12/layouts/49-Phone_Picker.txt`
- Page: `GET /pickers/{kind} (dialog fragment)` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `GET /parties/by-contact?email=&phone=` (OP-PARTY-06 `party_find_by_contact`), `GET /address-suggestions?q=` (OP-INTEG-02 `address_suggest`)
- Rules shown or enforced: BR-INTEG-02, BR-PARTY-11
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-07 Activity detail

- Readiness: behavior-ready
- Use case and users: Note, task, meeting and call cards and details; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/78-Card_-_Utility_Note.txt`, `objects/karbon_kontacts.fmp12/layouts/79-Note_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/82-Card_-_Utility_Meeting.txt`, `objects/karbon_kontacts.fmp12/layouts/87-Card_-_Utility_Task.txt`, `objects/karbon_kontacts.fmp12/layouts/91-Task_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/92-Meeting_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/171-Call_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/125-Activity_List.txt`
- Page: `GET /activities/{id}` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /activities` (OP-ACTIVITY-01 `activity_create`), `PATCH /activities/{id}` (OP-ACTIVITY-02 `activity_update`), `DELETE /activities/{id}` (OP-ACTIVITY-03 `activity_delete`), `PUT /activities/{id}/parties` (OP-ACTIVITY-04 `activity_party_set`), `POST /tasks/{id}/complete` (OP-ACTIVITY-05 `task_set_complete`)
- Rules shown or enforced: BR-ACTIVITY-01, BR-ACTIVITY-02, BR-ACTIVITY-03, BR-ACTIVITY-04, BR-ACTIVITY-05, BR-ACTIVITY-08, BR-ACTIVITY-09
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment) / confirmation (AP-07)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: two-step route: a confirmation dialog fragment, then the same request with confirm=true (AP-07)

### UI-08 Email message

- Readiness: behavior-ready
- Use case and users: Compose, send and read email messages; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/105-Card_-_Utility_EmailMessage.txt`, `objects/karbon_kontacts.fmp12/layouts/106-Card_-_Utility_EmailMessage_-_Send.txt`, `objects/karbon_kontacts.fmp12/layouts/118-Card_-_Utility_EmailMessage_-_Read_Only.txt`, `objects/karbon_kontacts.fmp12/layouts/167-Email_Message_-_View_Only.txt`, `objects/karbon_kontacts.fmp12/layouts/176-Email_Message_-_Edit.txt`
- Page: `GET /email-messages/{id}` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /activities` (OP-ACTIVITY-01 `activity_create`), `PATCH /activities/{id}` (OP-ACTIVITY-02 `activity_update`), `PUT /activities/{id}/parties` (OP-ACTIVITY-04 `activity_party_set`), `POST /email-messages/{id}/send` (OP-INTEG-01 `email_send`), `POST /templates/{id}/merge` (OP-TEMPLATE-01 `template_merge`)
- Rules shown or enforced: BR-ACTIVITY-01, BR-ACTIVITY-02, BR-ACTIVITY-03, BR-ACTIVITY-05, BR-ACTIVITY-08, BR-ACTIVITY-09, BR-INTEG-01, BR-TEMPLATE-01
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-09 Discussion

- Readiness: behavior-ready
- Use case and users: Discussion detail, comments and subscribers; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/93-Discussion_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/187-Card_-_Manage_Subscribers.txt`
- Page: `GET /discussions/{id}` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /discussions` (OP-ACTIVITY-06 `discussion_create`), `POST /discussions/{id}/comments` (OP-ACTIVITY-07 `discussion_comment_add`), `PUT /activities/{id}/parties` (OP-ACTIVITY-04 `activity_party_set`)
- Rules shown or enforced: BR-ACTIVITY-02, BR-ACTIVITY-03, BR-ACTIVITY-06, BR-ACTIVITY-07
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-10 Opportunities

- Readiness: behavior-ready
- Use case and users: Opportunity list, find and status chart; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/153-Opportunities.txt`, `objects/karbon_kontacts.fmp12/layouts/165-Opportunity_Find.txt`
- Page: `GET /opportunities` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `GET /opportunities/status-totals?active=` (OP-SALES-03 `opportunity_status_totals`), `POST /opportunities/{id}/status` (OP-SALES-02 `opportunity_set_status`)
- Rules shown or enforced: BR-SALES-12, BR-SALES-14
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-11 Opportunity detail

- Readiness: behavior-ready
- Use case and users: Opportunity detail page and card; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/156-Opportunity_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/162-Opportunity_Detail_-_Card.txt`
- Page: `GET /opportunities/{id}` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /opportunities; PUT /opportunities/{id}` (OP-SALES-01 `opportunity_save`), `POST /opportunities/{id}/status` (OP-SALES-02 `opportunity_set_status`), `POST /activities` (OP-ACTIVITY-01 `activity_create`)
- Rules shown or enforced: BR-ACTIVITY-01, BR-ACTIVITY-02, BR-SALES-12, BR-SALES-13
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-12 Invoice and sales receipt

- Readiness: behavior-ready
- Use case and users: Invoice and sales receipt editors and view-only views; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/69-Invoice.txt`, `objects/karbon_kontacts.fmp12/layouts/94-SalesReceipt.txt`, `objects/karbon_kontacts.fmp12/layouts/143-Invoice_-_View_Only.txt`, `objects/karbon_kontacts.fmp12/layouts/145-SalesReceipt_-_View_Only.txt`
- Page: `GET /sales/{id}` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /invoices; PUT /invoices/{id}` (OP-SALES-04 `invoice_save`), `POST /sales-receipts; PUT /sales-receipts/{id}` (OP-SALES-05 `sales_receipt_save`), `POST /sales-transactions/{id}/lock` (OP-SALES-06 `sales_transaction_lock`), `DELETE /sales-transactions/{id}` (OP-SALES-07 `sales_transaction_delete`), `DELETE /sales-transactions/{id}/lines` (OP-SALES-08 `sales_lines_remove`)
- Rules shown or enforced: BR-SALES-01, BR-SALES-02, BR-SALES-03, BR-SALES-04, BR-SALES-05, BR-SALES-06, BR-SALES-07, BR-SALES-08, BR-SALES-11, BR-SALES-15
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment) / locked (AP-04, AP-05) / confirmation (AP-07)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: two-step route: a confirmation dialog fragment, then the same request with confirm=true (AP-07)

### UI-13 Sales and payments lists

- Readiness: inventory-only
- Use case and users: Sales list and payments list; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/84-Sales_List.txt`, `objects/karbon_kontacts.fmp12/layouts/126-Payments_List.txt`
- Page: `GET /sales; GET /payments` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /invoices; PUT /invoices/{id}` (OP-SALES-04 `invoice_save`), `POST /payments; PUT /payments/{id}` (OP-SALES-09 `payment_save`)
- Rules shown or enforced: BR-SALES-01, BR-SALES-02, BR-SALES-03, BR-SALES-05, BR-SALES-08, BR-SALES-09, BR-SALES-10
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-14 New payment

- Readiness: behavior-ready
- Use case and users: Payment card with allocations; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/85-Card_-_New_Payment.txt`
- Page: `GET /payments/new` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /payments; PUT /payments/{id}` (OP-SALES-09 `payment_save`)
- Rules shown or enforced: BR-SALES-02, BR-SALES-09, BR-SALES-10
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment) / locked (AP-04, AP-05)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-15 Items

- Readiness: inventory-only
- Use case and users: Item list and item card; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/86-Items.txt`, `objects/karbon_kontacts.fmp12/layouts/135-Card_-_Items.txt`, `objects/karbon_kontacts.fmp12/layouts/136-Card_-_UtilityItem.txt`
- Page: `GET /items` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /items; PUT /items/{id}` (OP-SALES-10 `item_save`)
- Rules shown or enforced: BR-SALES-16
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-16 Templates

- Readiness: inventory-only
- Use case and users: Template list and detail with merge fields; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/111-Templates.txt`, `objects/karbon_kontacts.fmp12/layouts/112-Template_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/114-MergeField.txt`
- Page: `GET /templates` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /templates/{id}/merge` (OP-TEMPLATE-01 `template_merge`), `PUT /templates/{id}/merge-fields` (OP-TEMPLATE-02 `merge_fields_save`)
- Rules shown or enforced: BR-TEMPLATE-01, BR-TEMPLATE-02
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-17 Settings

- Readiness: inventory-only
- Use case and users: System settings, user email settings, integrations, book close date; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/70-Admin_Dashboard.txt`, `objects/karbon_kontacts.fmp12/layouts/71-Admin_User_List.txt`, `objects/karbon_kontacts.fmp12/layouts/128-Admin_Integrations.txt`, `objects/karbon_kontacts.fmp12/layouts/134-Admin_User_Detail.txt`, `objects/karbon_kontacts.fmp12/layouts/122-Card_-_Utility_User.txt`, `objects/karbon_log.fmp12/layouts/20-Settings.txt`
- Page: `GET /settings` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `GET /settings; PUT /settings` (OP-ADMIN-03 `settings_get_update`), `internal: request extractor used by every handler, no route` (OP-ADMIN-04 `current_user`); later (I7): host mode section `PUT /settings/host-mode` (OP-ADMIN-06 `host_mode_set`, desktop window only)
- Rules shown or enforced: BR-ADMIN-04, BR-ADMIN-05
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-18 Import

- Readiness: implementation-ready
- Use case and users: Import wizard and rejected-row report; the single local user (ROLE-OWNER).
- Source layouts: new screen (no FileMaker layout)
- Page: `GET /import` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /import/folder (desktop window only: opens the native folder dialog); POST /import; GET /import/{batch_id}/status` (OP-IMPORT-01 `import_filemaker_export`), `GET /import/{batch_id}` (OP-IMPORT-02 `import_report_get`)
- Rules shown or enforced: BR-IMPORT-01, BR-IMPORT-02, BR-PARTY-06
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-19 Files

- Readiness: inventory-only
- Use case and users: Files list, thumbnails and attachment viewer; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/142-Card_-_FilesList.txt`, `objects/karbon_kontacts.fmp12/layouts/185-Thumbnails.txt`
- Page: `GET /files` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /files (multipart upload)` (OP-FILE-01 `file_add`), `POST /attachments` (OP-FILE-02 `file_attach`), `DELETE /attachments/{id}` (OP-FILE-03 `file_detach`)
- Rules shown or enforced: BR-FILE-01, BR-FILE-02, BR-FILE-03
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-20 Invoice PDF and charts

- Readiness: inventory-only
- Use case and users: Printable invoice/receipt (PDF) and chart widgets; the single local user (ROLE-OWNER).
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/103-SalesLine_pdf.txt`, `objects/karbon_charts.fmp12/layouts/25-SalesOpp_Chart.txt`, `objects/karbon_charts.fmp12/layouts/22-Opportunity_Referral_Report.txt`
- Page: `GET /sales/{id}/print; GET /reports/sales-by-month` (full page; the same route returns the fragment to htmx)
- Fragments and actions: `POST /invoices; PUT /invoices/{id}` (OP-SALES-04 `invoice_save`), `GET /opportunities/status-totals?active=` (OP-SALES-03 `opportunity_status_totals`), `GET /reports/sales-by-month?from=&to=` (OP-SALES-11 `sales_summary_by_month`)
- Rules shown or enforced: BR-SALES-01, BR-SALES-02, BR-SALES-03, BR-SALES-05, BR-SALES-08, BR-SALES-14, BR-SALES-17
- States: loading (htmx request indicator) / empty / populated / validation (form re-rendered with field errors, D-25) / error (error fragment)
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-21 Chat (static mock)

- Readiness: implementation-ready (static)
- Use case and users: Shows what the FileMaker chat screen looked like, as a static mock with sample data (owner decision, 2026-10-28). It sends and receives nothing: Twilio SMS/chat is out of scope (OD-05-02). A banner reads "Sample data. SMS is not available in this version."
- Source layouts: `objects/karbon_kontacts.fmp12/layouts/184-Chat.txt`
- Page: `GET /chat` (one askama template with hard-coded sample conversations; no route reads or writes the database)
- Fragments and actions: none. The message box and send button are disabled.
- Rules shown or enforced: none
- States: populated (sample data only)
- Keyboard/focus/labels: every control labeled; disabled controls announced as disabled; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### UI-22 Login (host mode, later)

- Readiness: behavior-ready (new screen, no FileMaker layout)
- Use case and users: A browser on another computer on the LAN logs in to a host-mode session (I7, AP-14). The desktop window never shows it.
- Source layouts: new screen (no FileMaker layout); nearest source analogue is FileMaker network sharing
- Page: `GET /login (host mode only)`
- Fragments and actions: `POST /login`, `POST /logout` (OP-ADMIN-07 `host_session`); host mode is turned on in UI-17 with `PUT /settings/host-mode` (OP-ADMIN-06 `host_mode_set`)
- Rules shown or enforced: BR-ADMIN-06
- States: empty / validation (wrong password) / locked out after repeated failures / error
- Keyboard/focus/labels: every control labeled; full keyboard operation; WCAG 2.1 AA contrast (D-38).
- Destructive action confirmation: none

### QBO controls not built (owner, 2026-10-27; D-02)

The kontacts layouts carry QuickBooks buttons and fields that the target does not build: Send Invoice/SalesReceipt to QBO, Refresh From QBO, Remove Sales Transaction From QBO, View in QBO (UI-12, UI-13), the QBO authorize web viewer and the Using QBO switch on Admin Integrations (UI-17), and qbo_error / qbo_send displays. UI-17 shows one read-only line for the accounting stub (OP-INTEG-04 `accounting_integration_status`: "not implemented").

### UI-18 Import (implementation-ready)

- Step 1: choose the folder that holds the FileMaker export. The button posts to `/import/folder`; the app opens the native folder dialog itself (desktop window only; AP-09) and returns a fragment with the tables found and row counts.
- Step 2: choose which `user` row is "me" (BR-ADMIN-04), or create one.
- Step 3: run; a progress fragment, polled by htmx every second, shows each table; cancel rolls back the whole import.
- Step 4: report: rows loaded per table, rows rejected with reasons (import_rejected_row), files copied, caches recomputed. Export the report as CSV.
- Error states: unreadable file, unknown table, missing required table (party), disk full. Each shows the typed error.

## Conditional presentation

Per-layout hide rules and conditional formats are listed in the layout explorer CSVs and summarized in the appendix. Targets: rules testing record state (locked, sent, empty) become template conditionals on the command result (askama `{% if %}`); constant or findMode-only rules are not built (D-26); LedgerLink layouts are removed with QBO (D-02).

## Design system and responsive decisions

| Decision | Value | Source fact or product decision | Acceptance check |
|---|---|---|---|
| Typography/color/spacing | Tailwind + DaisyUI themes (one light, one dark); DaisyUI components (table, modal, drawer, tabs, form controls) | owner decision 2026-10-28; look per OD-06-01 | contrast AA |
| Breakpoints/input devices | desktop window, min 1024 x 700; mouse and keyboard | D-37 | resizes without horizontal scroll |
| Data-density/table behavior | dense DaisyUI tables, sorted, filtered and paged on the server (50-100 rows); htmx swaps the table body | Karbon list layouts | a page of a 10,000-row list renders in under 200 ms |
| Modal/tab/navigation behavior | FileMaker cards become DaisyUI modals on `<dialog>` (fragments loaded by htmx) or side drawers; one main window; tabs and dropdowns use Alpine.js | Card - * layouts | focus moves into the dialog and returns on close |
| Print/PDF behavior | invoice/receipt print page (askama template with print CSS) turned into a PDF locally (S-02) | SalesLine_pdf layout (D-38) | PDF matches totals |
| Charts | Chart.js on a `<canvas>`; the fragment carries the data as JSON in a data attribute | karbon_charts layouts | chart totals equal the query |
| Accessibility target | WCAG 2.1 AA; semantic HTML (labels, buttons, landmarks) | D-38 | route tests check labels and roles in the HTML; axe check by hand per screen |

## Missing evidence

| Layout/object | Missing evidence | Implementation risk | Required screenshot/decision | Owner/status |
|---|---|---|---|---|
| all inventory-only screens | screenshots, visual hierarchy | look and density differ from FileMaker | OD-06-01 | Product owner / open |

## Verification

- [x] Every in-scope layout file has a target disposition (appendix).
- [x] Every in-scope target page has a readiness level.
- [x] Hide rules, formats, triggers and buttons map to template conditionals, htmx actions or the called script disposition (07 appendix).
- [x] Negative tests for hidden or restricted operations exist at the route and command layers (08 T-08-*).
- [ ] Critical workflows walked against the current app or screenshots: blocked, none supplied (D-37, OD-06-01).
