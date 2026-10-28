# Step 6 audit: the plan against the objects

KarbonFM, audited 2026-10-27 by a reader who did not write `07_business_logic.md`. Scope: Tauri 2 + SQLite, one user per install; QuickBooks Online out of scope and stubbed (D-02).

Labels follow the skill: **fact** (from an object file), **inference**, **product decision**, **unavailable**. Object files are cited as `objects/<path>`.

## Summary

| Check | Result |
|---|---|
| Tier1 rules re-read against their objects | 29 rules. **8 wrong** (28%), 8 incomplete, 3 with wrong or missing citations only, 10 correct. All 19 were fixed in 07 under their own ids (A-01 to A-20; A-13 is not used). |
| Graph rows (`meaning`, `fk_candidate`) | 30 sampled, **2 wrong (6.7%)**. A full re-derivation of all 683 relationships found about 8 wrong rows. None feeds a foreign key in 04. 4 cells fixed. |
| Layout rows (hide rules, conditional formats) | 30 lines sampled, **0 wrong (0%)**. Line counts match the objects on all 382 layouts. |
| Script `callers` cells | 30 sampled (15 from the panes that shared the overwritten `callers.json`), **1 wrong (3.3%)**, and not from those panes. A sweep of all 309 `none found` cells found 2 more with a live caller. 3 cells and 2 appendix rows fixed. |
| Gaps the plan still does not understand | **56 listed** (59 at the audit; OD-04-02, OD-05-01 and OD-05-02 were closed by owner decisions on 2026-10-28): 30 unanswered D- questions, 13 open OD- decisions (3 new from this audit), 13 object gap groups (G-01 to G-13). |
| Blocks the plan? | Nothing blocks I1, I2 or I4. **I5 (sales) should not start** until the Accounting owner confirms or replaces the defaults of OD-07-05, OD-07-06 and OD-07-07. The I3 recompute of retainer balances also waits on OD-07-07. See "What should block". |

## Method

- Tier1: for each of the 29 tier1 rules, every cited object file was read at the cited steps and fields, plus any script those steps call that carries the rule (for example 395 around 585, 576 around 599, and the karbon_data cascade relationships behind the deletes). The verdicts:
  - *wrong*: the rule states as source fact something the object contradicts, or its boundary or definition differs.
  - *incomplete*: the rule leaves out source behavior that changes the target or its tests.
  - *citation*: the rule is right but its steps or files are wrong or missing.
- Graph: 30 relationships drawn at random (seed 6). `meaning` and `fk_candidate` were re-derived from the relationship file and the field options in the base table files, using the rules in `fm-graph-explorer.md`. Every mismatch was then read by hand. The same checker then ran over all 683 relationships, and its flagged rows were read by hand.
- Layouts: 30 hide-rule and conditional-format lines drawn at random (seed 11) from the 484 lines on core-file layouts. Each was compared with the `HideCondition` or `ConditionalFormatting` block of its object key. The count of `HideCondition` and `Item` blocks was then compared with the CSV line counts for all 382 layouts.
- Callers: an independent caller index was built from every `Script id=N name=...` reference in `objects/`. Quoted and unquoted names both count, `in=` and `ref=external` resolve to their file, and `ref=missing` and `ref=none` are skipped. 15 scripts were drawn from the panes that reported the shared `/private/tmp/claude-501/s` folder (agents 07, 08, 12) and 15 from the other panes (seeds 21, 22), all from core files. Every `none found` cell in the catalog was also checked against the index.

## 1. Tier1 rules

| Rule | Verdict | What the object says (fact unless marked) | Fix in 07 |
|---|---|---|---|
| BR-PARTY-01 | **wrong** | The rule (and OD-07-01) said the DDR does not show which children a hard delete removes. It does: the karbon_data graph cascades a Party delete to ContactMethod, then Address, Phone, NetworkAddress and ContactMethodPurpose; to PartyRelation in both directions; to PartyRole, then Customer, Lead, Vendor, Employee and ExternalLink; and to Person or Organization (`objects/karbon_data.fmp12/relationships/3-ContactMethod_Party.txt`, `12-`, `13-`, `27-`, `35-`, `36-`, `28-`, `29-`, `30-`, `34-`, `39-`, `4-`, `5-`, `6-`, `37-`). ActivityParty, FileAttachment, Opportunity and SalesTransaction are not cascaded. The soft delete reads the reason from the unset `$json` (`objects/karbon_kontroller.fmp12/scripts/83-wip_Delete_Party.txt` step 91), so it is always empty. No find or list reads DeletedInd, so "hidden from search" is a product decision. Inference: whether a cascade defined in the karbon_data graph fires for a delete made through the kontroller graph is FileMaker run-time behavior the DDR cannot prove. | A-01. OD-07-01 reworded to the uncascaded children. |
| BR-PARTY-03 | incomplete | Edit writes the unset `$Relationship` and `$Role` (`508-Edit_Party_Relation_data.txt` steps 133-137, 177-181), which clears them. Create never sets billing or shipping on the To-to-From row (`254-...` steps 90, 151, 157). The delete runs from step 87 to step 130, not 87-104. | A-02 |
| BR-PARTY-05 | **wrong** | "party_role_type_id 2 for customer": the default is 2 (`705-...` step 69), but step 172 writes the unset `$PartyRoleTypeId`. Customer roles are stored with an empty type (field 46 has no auto-enter, `objects/karbon_data.fmp12/tables/151-PartyRole.txt`). Lead (610) parses it correctly. Create Lead (595), which the rule cites as idempotent, makes a new party every time. | A-03 |
| BR-PARTY-09 | **wrong** | The roles were wrong. The wizard gives the person an employee role (164), and gives a lead only when no organization was created. Customer and employee on an existing person are `@todo` (`objects/karbon_kontacts.fmp12/scripts/393-Create_Parties.txt` steps 244-252). Each call is its own transaction, and a failure leaves the earlier records (steps 95-98, 224-226, 282-284). "A failure leaves nothing behind" is the D-08 product decision, now labeled. | A-04 |
| BR-CONTACT-01 | correct | `182-`, `183-`, `184-` clear and set primary as stated. (Note for BR-PARTY-06, tier2: 184 writes the network-address label into PhoneLabelCached, step 101, and the primary flag covers web addresses too.) | none |
| BR-CONTACT-02 | citation | Steps 72-111, not 58-79. Purpose ids 1 billing, 2 shipping, 3 mailing (steps 91-96). | A-05 |
| BR-ACTIVITY-01 | citation | The ids match 660. A Meeting sets EventInd and makes a Meeting row; there is no meeting flag (`392-Create_Event.txt` steps 101, 121, 137). | A-06 |
| BR-ACTIVITY-02 | incomplete | The creator is added only when the request names none, and a user with no party fails with a validation error (`377-Create_Activity.txt` steps 120-135). | A-07 |
| BR-ACTIVITY-03 | incomplete | It also refuses a duplicate contact method in one role on an email or task (`376-...` steps 155-163). | A-08 |
| BR-ACTIVITY-04 | incomplete | Complete also sets DateDisplay to today and requires a valid party (`690-...` steps 91-104). Incomplete sets AlertInd = 1 and DateDisplay to the due date or creation date (`691-...` steps 72, 102-109). | A-09 |
| BR-ACTIVITY-08 | correct | `522-...` steps 124-127; `389-...` step 65. | none |
| BR-SALES-01 | incomplete | An empty qty counts as 1, and the recompute runs only when the unit price is not empty (`objects/karbon_kontacts.fmp12/scripts/270-...` steps 6-13). | A-10 |
| BR-SALES-02 | incomplete | The layout triggers compute from the lines, but the API save stores the totals it is given, with subtotal = totalAmt - totalTax (`585-...` steps 137, 202-205). **No core script computes totalTax:** 270 steps 22-28 ("let QBO calc taxes") are disabled. With QBO out, nothing sets tax. | A-11; new OD-07-06 |
| BR-SALES-04 | **wrong** | Only the Unlock button checks the close date (`335-...` steps 9-11). Delete (560) and the API save (395, 428) do not, so the source does not block editing or deleting an unlocked old transaction. The rule's "edited or deleted" is now labeled a product decision. | A-12; BR-SALES-04 added to OP-SALES-04/05 in 07 and 05 |
| BR-SALES-05 | correct | 585 step 157 and 560 step 55 refuse a locked transaction. A cross-reference was added: the 395 wrapper unposts first and bypasses the check. | note only |
| BR-SALES-06 | **wrong** | The balance definition. The incremental post (`576-...` step 86) counts posted transactions only, but the rebuild the rule cites (`696-...` step 69) sums **all** of the customer's transactions, posted or not (`SalesTransaction#ByCustomer` has no LockedInd predicate, `objects/karbon_kontroller.fmp12/relationships/197-DBTransactions_SalesTransaction_ByCustomer.txt`). Also stated now: the PostSalesTransactions gate (step 39) and the SumaryByMonth writes to the missing file (steps 100-124). | A-14; new OD-07-07 |
| BR-SALES-07 | correct | `178-SalesLine.txt` field 17, `177-SalesTransaction.txt` field 65 (inference: FileMaker compares "Retainer" without regard to case). | none |
| BR-SALES-08 | **wrong** | Its steps were in the wrong script: 585 steps 21/96/188 show no unposting. The wrapper 395 unposts an existing transaction, saves, then **posts every saved transaction, new ones too** (steps 84-92, 97, 108-112). It is three transactions, not one, and 428 does the same. 585 marks removed lines `qbo_deleted` and does not delete them. As written, the rule contradicted BR-SALES-05, AP-05 and the UI-12 flow "draft -> lines -> lock". | A-15; new OD-07-05 |
| BR-SALES-09 | correct | `221-...` step 3-5, `283-...` steps 1-5, `278-...`. (283 also sends to QBO at steps 17-24, which D-02 removes.) | none |
| BR-SALES-11 | citation | Lines, the invoice or receipt row and attachments are removed by cascade (`objects/karbon_data.fmp12/relationships/59-`, `76-`, `77-`, `82-`), not by steps. Button 211 has no caller and tests an unset `$result` (step 8). | A-16 |
| BR-SALES-12 | incomplete | An inactive status also closes the lead role (`608-...` steps 120-124). | A-17 |
| BR-SALES-13 | incomplete | Every new opportunity is then set to status 1 New through 608 (`602-...` steps 145-146). | A-18 |
| BR-FILE-01 | **wrong** | The boundary. The size is Round(bytes / 1000; 2) KB and the limit is > 20480, so 20,480,005 bytes or more is refused, not "20 MB". The caller may pass the size (`617-...` steps 82, 103-109). | A-19 |
| BR-ADMIN-01 | correct | It is a target invariant (D-08). The source welcome wizard is three separate calls (`482-...` steps 20, 49, 67). | none |
| BR-ADMIN-02 | **wrong** | 298, 301 and 302 are create commands that take a parameter and hold no seed values. The ids that exist are hard-coded in scripts: party_role_type 2 and 4, purpose 1-3. Opportunity status (607) has 8 rows; the missing comma is confirmed before id 8. | A-20 |
| BR-ADMIN-04 | correct | `54-...`, `375-...` | none |
| BR-INTEG-01 | correct | `521-...` steps 92-125. A failed send exits with an error and End Transaction reverts SentInd. | none |
| BR-IMPORT-01 | correct | Target rule; the cited tables exist. | none |
| BR-IMPORT-02 | correct | User 118, System 37, 55, 68, 69, 76 and Employee SocialSecurityNumber hold the secrets and PII; 04 has none of them. | none |

The 28% wrong rate is above the 17% the skill reports from an earlier run. The tier2 and tier3 rules were not re-read, so expect errors of the same kind there. The most common causes were unset variables (`$json`, `$PartyRoleTypeId`, `$Relationship`) that the rule read as working steps, and a wrapper script that changes what the cited inner script means.

## 2. Spot-checks of Step 3 rows

### Graph rows

| Row | CSV | Object | Fix |
|---|---|---|---|
| `objects/karbon_kontacts.fmp12/relationships/66-Activity_ActivityGroupActivity_ActivityGroup_Activity_Activi.txt` | filter / no | one Equal predicate on a deleted field (`ref=missing`), base table missing | `unresolved / unresolved`, as rows 19 and 28 of kontroller are |
| `objects/karbon_kontacts.fmp12/relationships/67-Activity_ActivityGroupActivity_Activity.txt` | filter / no | same | same |
| (full sweep) `objects/karbon_kontroller.fmp12/relationships/54-System_DBTransactions.txt` | identity / needs-profile | System::ONE = DBTransactions::ONE, both fixed calcs | `constant / no` |
| (full sweep) `objects/karbon_kontroller.fmp12/relationships/134-Customer_Customer_ContactMethod_URL_NetworkAddress.txt` | identity / needs-profile | Customer::ONE (fixed) = NetworkAddress::WebAddressInd | `filter / no` |

Sample error rate: 2 of 30 (6.7%). Population: about 8 of 683 (1.2%). The other unfixed ones are a Generator row and 2 kontacts self-joins of the same deleted-field pattern, none in a plan table. No 04 foreign key or index rests on a wrong row: all four are deleted-table or constant joins.

### Layout rows

30 of 30 lines match their objects, including condition meaning, effect (`style`, `disabled`, `none`) and the constant-true hides. Every layout has as many hide and format lines as its object has `HideCondition` and `Item` blocks. Error rate 0%. One open point stays with D-26: the CSVs read `findMode=False` as "not applied in Find mode", which the LEGEND does not decode.

### Script `callers` cells

| Row | CSV | Object | Fix |
|---|---|---|---|
| `objects/karbon_kontacts.fmp12/scripts/26-Virtual_List_On_Startup.txt` | adds `karbon_Build.../2-Trigger_OnFirstWindowOpen.txt (by name)` | that step calls karbon_Build's own script 354 of the same name | caller removed |
| (sweep) `objects/Picker_Module.fmp12/scripts/26-Configure.txt` | none found | called by `29-OnFirstWindowOpen.txt` and `67-`, `128-` (unquoted `name=Configure`, skill problem B5) | callers filled; 07 appendix reason corrected (retire stays) |
| (sweep) `objects/karbon_kontacts.fmp12/scripts/188-Test.txt` | none found | OnObjectKeystroke trigger on `objects/karbon_kontacts.fmp12/layouts/49-Contact_Card.txt` key 195 | callers filled; 07 appendix reason corrected (retire stays; the script is empty) |

Sample error rate: 1 of 30 (3.3%). The 15 rows from the panes that hit the overwritten `callers.json` (agents 07, 08, 12) had 0 errors, so that collision left no trace in this sample. Of the 309 `none found` cells, 2 (0.6%) have a live caller. Neither changes a disposition. `catalog_contract.py verify` passes for scripts, graph and layouts after the fixes.

## 3. What the plan still does not understand

Each gap names the part of the plan it affects. The operator decides which gaps matter. An accepted gap stays listed.

### 3a. Unanswered discovery questions (30)

All 30 have a recorded default in `01_discovery_answers.md`. The plan uses those defaults.

| Id | Topic | Plan parts affected |
|---|---|---|
| D-03 | Time-tracking module (retired by default) | 04, 06, 07 appendix; BR-SALES-11 time unlinking; G-04 |
| D-07 | Source defects: build intended behavior | Cited by 9 tier1 rules (BR-PARTY-01, -03, -05, BR-SALES-02, -05, -06, -08, -11, BR-FILE-01); every "source defect" note in 07 |
| D-09 | Secrets and PII (keychain, excluded columns); tax ids answered 2026-10-28: plain text | BR-IMPORT-02, AP-03, 04 exclusions |
| D-10 | No-caller entry points, external clients | 05 (no external API), G-09 extended privileges |
| D-11 | Drop objects with no caller | 532 retired scripts in the 07 appendix, 04/06 appendices |
| D-12 | Live objects pointing at deleted targets | OD-07-01, OD-07-04; G-01, G-02, G-05 |
| D-13 | Run-time data sources, karbon_reports | BR-SALES-06, BR-SALES-17; G-03 |
| D-14 | Integrations kept (SMTP yes; Twilio SMS/chat dropped by the owner, 2026-10-28) | BR-INTEG-01/02, EXT list in 05, I6 |
| D-15 | Supertype/subtype tables | 04 core design, 05 resource shapes |
| D-16 | needs-profile joins (100) | OD-04-01, 04 foreign keys, I3 import profile |
| D-17 | Container files | BR-FILE-01/02, I3 file copy |
| D-18 | Money rules (storage decided 2026-10-28: integer cents, OD-04-02) | **BR-SALES-01..08, OD-07-05/06/07, I5** |
| D-19 | Hard-coded ids as seeds | BR-ACTIVITY-01/02, BR-ADMIN-02 (three seed tables without values), I1 |
| D-20 | Client-only rules become command invariants | BR-SALES-04, BR-SALES-09, AP-04..06 |
| D-21 | Data-driven merge (Perform Script by name, Evaluate) | BR-TEMPLATE-01, OD-07-03, G-08 |
| D-22 | Custom-function copies | 07 calculations section only |
| D-23 | UI-only globals are transient | 04 dropped tables, 06 client state |
| D-24 | Plugins; PostSalesTransactions taken as on | **BR-SALES-06 posting gate**, G-13 |
| D-25 | Error contract | 05 errors, 07 error branches |
| D-26 | Layout codes (`findMode`, dead objects) | 06 hide rules |
| D-27 | Constant plus auto-enter, StrictValidation | 04 defaults and checks |
| D-28 | Custom menus to screen actions | 06 actions; G-02 (menus to deleted scripts) |
| D-29 | Meaning of Activity::AlertInd | BR-ACTIVITY-04, -09; 04 activity columns |
| D-30 | Logging and audit | BR-ADMIN-03, 02 observability |
| D-31 | Party_Merge_Utility (no DDR) | Whether 07 needs an OP-PARTY merge operation; G-11 |
| D-32 | Karbon.fmp12 launcher (no DDR) | 06 entry point (OD-00-01) |
| D-33 | Per-object notes for Step 5 writers | single rows of 04, 06, 07 |
| D-37 | Devices and design input | OD-06-01, 06 inventory-only screens |
| D-38 | Print and reports | 05 report commands, 06 print views |
| D-39 | History and cached values | BR-PARTY-06, BR-IMPORT-01 recompute |

`00_app_summary.md` still lists OD-00-01 to OD-00-06 as open. OD-00-04, -05 and -06 are settled by D-01, D-02 and D-05. OD-00-01, -02 and -03 are D-32, D-31 and D-13.

### 3b. Open decisions (13)

OD-04-02 (money storage: integer cents) and OD-05-02 (SMS/chat: dropped) were decided by the owner on 2026-10-28 and are no longer gaps (03). The UI-stack decision of the same day (server-rendered HTML, no TypeScript) closed OD-05-01, so 13 remain.

| Id | Decision | Affects |
|---|---|---|
| OD-02-02 | Code signing, Linux packages, update channel | 02, I6 |
| OD-03-01 | FileMaker export format and container export | I3, BR-IMPORT-01 |
| OD-04-01 | needs-profile foreign keys as constraints | 04 (15 FKs), I3 |
| OD-04-03 | Repeating fields | 04 |
| OD-06-01 | Visual design (no screenshots) | every inventory-only screen in 06 |
| OD-07-01 | Hard party delete: the uncascaded children (reworded, A-01) | BR-PARTY-01, OP-PARTY-04, I2 |
| OD-07-02 | Non-US phone prefix | BR-CONTACT-04 |
| OD-07-03 | Merge resolvers in real exports | BR-TEMPLATE-01, I4 |
| OD-07-04 | Apply Retainer calls deleted script 328 | UI-12 invoice screen, I5; G-01 |
| **OD-07-05** (new) | Saving a posted invoice or receipt: refuse (default) or unpost, save and repost as the source does | BR-SALES-05, BR-SALES-08, AP-05, OP-SALES-04/05, UI-12, I5 |
| **OD-07-06** (new) | How total_tax is computed with QBO out | BR-SALES-02, invoice and receipt totals, I5 |
| **OD-07-07** (new) | Retainer balance: posted only (default) or all transactions | BR-SALES-06, BR-IMPORT-01 recompute, I3 and I5 |
| OD-08-01 | Optional passcode or SQLCipher | 08, I1 |

### 3c. Object gaps: unresolved targets and `objects/unresolved.tsv`

The catalogs mark 86 targets `unresolved` (tables 7, layouts 28, value lists 6, security 14, file 3, graph 25, scripts 3), 45 of them in core files. `objects/unresolved.tsv` has 488 rows: 304 in core files and 184 in out-of-scope files. Every row falls in exactly one group below.

| Gap | What is not known | Evidence | unresolved.tsv rows | Affects |
|---|---|---|---|---|
| G-01 | **Apply Retainer** (OD-07-04). The invoice button bar (key 333, shown when no line holds a retainer and the balance is over 0) opens a popover with Retainer Balance and Accounts Receivable and an OK button. OK calls kontacts 223, which commits and calls kontroller script 328. No script with id 328 exists, and no other object mentions applying a retainer. The logic (which line it adds, how it moves retainer_balance_cached) is **unavailable**. | `objects/karbon_kontacts.fmp12/scripts/223-btn_Apply_Retainer.txt` step 7; `objects/karbon_kontacts.fmp12/layouts/69-Invoice.txt` keys 333-340 | 1 | UI-12, BR-SALES-06/07, I5. The only `unresolved` script disposition in 07 |
| G-02 | Other live core objects that call deleted scripts. The customer list "Advanced Find" button (795, layouts 51 and 186). Files list "Add" (539, layout 142). The MSGraph "Authorize" button (karbon_konnector 271, layout 128). Billing Report for Invoice (ZeitGeist 197, script 228). Export HTML (charts 371, script 737). Save Card UtilityNote (262, script 257). Charts "copy param calc" (layout 32). Custom menus "Requests (Party/Opportunity Find)" (6). Create Employee to a deleted layout (173). | `objects/karbon_kontacts.fmp12/layouts/51-Customer_Detail.txt`, `.../142-Card_-_FilesList.txt`, `.../128-Admin_Integrations.txt`, `objects/karbon_kontacts.fmp12/custom-menus/30-Requests_Party_Find.txt` | 15 | 06 party/customer list (advanced find has no source behavior, so OP-PARTY-05 covers it by inference), UI-19 files, 06 menus (D-28) |
| G-03 | **karbon_reports / SumaryByMonth.** The data source has no file in the set. Posting writes revenue and new-customer counts to it. | `objects/karbon_kontroller.fmp12/data-sources/8-karbon_reports.txt`; `576-...` steps 100-124; `578-...`; `objects/karbon_charts.fmp12/layouts/20-SumaryByMonth.txt` | 17 | BR-SALES-06, BR-SALES-17 (replaced by a view; the source's "new customer count" per period has no rule yet), D-13 |
| G-04 | Time-tracking tables (Project, Milestone, WorkEffort, TimeEntry, TimeLog, Timesheet) that are deleted or external | kontroller relationships 17-28, 41; charts table occurrences and relationships 74-77; Picker Project Picker | 85 | D-03 retirement, BR-SALES-11 unlinking |
| G-05 | Deleted fields on deprecated or helper objects (ActivityGroup, ActivityMergeField, DiscussionUser, Chat_deprecated, Globals, the UtilityEmailMessage table occurrences, Template Detail) | `objects/karbon_data.fmp12/layouts/63-ActivityAttachment_Deprecated.txt` and others | 74 | D-12. The UtilityEmailMessage rows touch the email screens (BR-INTEG-01, I4); the others have no target |
| G-06 | Transaction plumbing: fields and layouts missing in DBTransactions and the karbon_log transaction module | `objects/karbon_kontroller.fmp12/layouts/3-DBTransactions.txt`; `objects/karbon_log.fmp12/scripts/28-Start_Transaction_TransactionPayload.txt` | 38 | none: replaced by one SQLite transaction per command (D-08) |
| G-07 | Empty slots (`ref=none`): inactive lookups, unused dialog inputs, the disabled tax steps in 270, set/get helpers | `objects/karbon_data.fmp12/tables/144-ActivityParty.txt`; `objects/karbon_kontacts.fmp12/scripts/775-Button_Send_Email_Message.txt` | 74 | none (270's empty tax slots are the evidence for OD-07-06) |
| G-08 | Dynamic calls that cannot be traced: kontroller 511 Perform Script By Name (owner `unresolved`, retired), the merge resolvers | `objects/karbon_kontroller.fmp12/scripts/511-Perform_Script_By_Name.txt`; `490-...` | 0 | BR-TEMPLATE-01, OD-07-03, D-21 |
| G-09 | Extended privileges fmurlscript, fmxml and fmxdbc in 6 core files (target `unresolved`) | `objects/karbon_data.fmp12/extended-privileges/5-fmxml.txt` and others | 0 | 08 (no external access in the target), D-10 |
| G-10 | 11 core layouts with target `unresolved` (SumaryByMonth, Preview, DBTransactions, VIRTUALLISTS, Address Configuration, Project Picker, Test, SalesUtility, ExternalLinkType, MergeField, UtilityDiscussion) | `explorers/layouts/csv` | 0 | 06 inventory. The MergeField and ExternalLinkType layouts may be admin screens for BR-TEMPLATE-02 and the external_link stub |
| G-11 | **Party_Merge_Utility.fmp12** has no DDR. kontacts declares a data source for it, and no object in the set uses it. Whether a party merge feature exists is **unavailable**. | `objects/karbon_kontacts.fmp12/data-sources/14-Party_Merge_Utility.txt` | 0 | PARTY domain (no OP-PARTY merge command; BR-PARTY-02 finds duplicates but nothing merges them), 06, D-31 |
| G-12 | Out-of-scope files (LedgerLink 84, Generator 63, tester 23, Build 8, ZeitGeist 2, dev 2, konnector 2) | `objects/unresolved.tsv` | 184 | none (D-01, D-02) |
| G-13 | **PostSalesTransactions** is a function missing from the DDR (plugin or deleted custom function) that gates every post. The plan assumes it is on. | `objects/karbon_kontroller.fmp12/scripts/576-Post_Unpost_Internal_Id_Post.txt` step 39 | 0 | BR-SALES-06, BR-SALES-15, D-24 |

Gap count: 30 + 13 + 13 = **56** (59 at the audit, before the 2026-10-28 owner decisions). The 488 rows sum as 1 + 15 + 17 + 85 + 74 + 38 + 74 + 184.

## What should block

- **I5 (opportunities and sales) waits on OD-07-05 and OD-07-06**, and on OD-07-07 for its tests. Each default changes what the source did with money:
  - OD-07-05: no automatic repost on save.
  - OD-07-06: no computed tax.
  - OD-07-07: one of two disagreeing balance sums.

  These are tier1 rules, and the Accounting owner has not answered D-18.
- **The I3 recompute** (BR-IMPORT-01 recomputes retainer balances) gives different numbers under the two OD-07-07 options. Decide it before the first real import is accepted.
- **OD-07-04 / G-01 (Apply Retainer)** cannot be rebuilt from the DDR. Either the owner describes the behavior, or UI-12 ships without the button. It does not block I1-I4.
- Nothing found here blocks I1, I2, I4 or I6. Those gaps all have recorded defaults the operator can accept.
- Not a blocker, but a risk: 28% of tier1 rules were wrong, and the tier2 and tier3 rules have not had a second reader. The sales and party tier2 rules (BR-SALES-10, -15, -16, BR-PARTY-02, -06, -10) rest on the same API-script patterns.

## Changes made by this audit

| Artifact | Change |
|---|---|
| `07_business_logic.md` | Rewrote 19 tier1 rows under their ids, each marked *Audit fix A-nn (09)*: A-01 to A-12 and A-14 to A-20. Added a cross-reference to BR-SALES-05 and evidence to BR-SALES-11. Reworded OD-07-01. Added OD-07-05, -06 and -07. Added BR-SALES-04 to OP-SALES-04/05. |
| `07_business_logic_appendix.md` | Picker 26 Configure and kontacts 188 Test: disposition reason corrected (both stay retired) |
| `05_api_design.md` | BR-SALES-04 added to `invoice_save` and `sales_receipt_save`. The Apply Retainer row points at OD-07-04 and G-01. |
| `03_migration_plan.md` | OD-07-01 reworded. OD-07-05, -06 and -07 added to the open-decision table and to the open-decisions line of the summary table. |
| `explorers/graph/csv` | 4 rows: kontacts 66 and 67, kontroller 54 and 134 |
| `explorers/scripts/csv` | 3 `callers` cells: Picker 26, kontacts 188, kontacts 26. Picker 26 owner set to `client-ui`. |

`catalog_contract.py verify` passes for the scripts (1369), graph (1467) and layouts (382) catalogs after the changes. `reconcile_notes.md` was not rerun. The 07 changes add no new rule, command or table, only the three OD ids, and those are now in 03.
