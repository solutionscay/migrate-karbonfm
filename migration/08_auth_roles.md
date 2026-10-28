# Identity and Authorization Contract

One user per install, data local to the machine, no remote server (D-04, D-35, D-36). The app serves its own HTML from a local axum server on 127.0.0.1 inside the desktop process (owner, 2026-10-28). There is no login, no role hierarchy and no record predicate to port: every FileMaker file has only Admin ([Full Access]) and an inactive [Guest] (karbon_tester adds Bots). Authorization in the target is the protection of the data and of the commands inside one desktop app: who may reach the local server (AP-13), what leaves the machine, and which destructive or locked operations are refused. The app is single-user on one computer; the server listens on 127.0.0.1 only and serves nothing to other computers (owner, 2026-10-28).

## Authority and evidence

| Source | Count | Authority/use | Gaps |
|---|---|---|---|
| `accounts`, `privilege-sets` object files | 27 accounts, 40 privilege sets | identity catalog: Admin and Guest in each file, Bots in karbon_tester | password hashes absent |
| Record/field/layout privileges in privilege-set files | 40 | only the built-in sets: [Full Access], [Data Entry Only], [Read-Only Access], plus Bot | no custom record predicates exist |
| Script/value-list privileges | 40 | built-in | none |
| `extended-privileges` object files | 119 | access channels (fmapp, fmrest, fmxml, ...) | all retired: no server |
| Layout explorer (hide and format rules) | 713 hide, 140 formats | UI evidence only | see 06 |
| Menu explorer | 333 | removed commands (Delete All Records, Duplicate on invoices) | see 06 |
| Discovery | D-04, D-09, D-20, D-35 | single local user, keychain secrets, command-layer rules | OD-08-01 |

## Identity provisioning

| Source account/category | Status/auth type | Target identity | Invite/reset/cutover | Role assignment owner | Audit/exception |
|---|---|---|---|---|---|
| Admin ([Full Access]) in each file | Active, FileMaker | the single local user (ROLE-OWNER), created by the welcome setup (BR-ADMIN-01) | none: no password migration, no login (D-04) | n/a | user id on every audit stamp (BR-ADMIN-04) |
| [Guest] in each file | Inactive | none | retired | n/a | n/a |
| Bots (Bot set) in karbon_tester | Active | none | retired with the tooling files (D-01) | n/a | n/a |
| Karbon `user` table rows (app-level users) | data | imported as data; the row linked to the importing person becomes the local user (OP-IMPORT-01) | the import asks which user is "me" | user | other users stay as historical creators |

## Roles and capabilities

| Source privilege set/rule | Target role/capability | Permission boundary | Explicit denies | Source evidence | Approval owner |
|---|---|---|---|---|---|
| [Full Access] | ROLE-OWNER: every command of 05 | the commands behind the registered routes (AP-01) | AP-04, AP-05, AP-07, AP-08 still refuse | `objects/karbon_data.fmp12/privilege-sets/1-Full_Access.txt` | Product owner |
| [Data Entry Only], [Read-Only Access] | none | n/a | n/a | built-in, held by no active account | n/a |
| Bot | none | n/a | n/a | `objects/karbon_tester.fmp12/privilege-sets/4-Bot.txt` | n/a |

## Entity operation matrix

ROLE-OWNER may run every command; the conditions below are the only denials. Full table-by-command matrix: `08_auth_roles_appendix.md`.

| Entity/domain | Role | Create | View/query | Edit | Delete | Bulk/export/admin | Enforcement owner |
|---|---|---|---|---|---|---|---|
| PARTY | ROLE-OWNER | allow | allow (deleted parties hidden) | allow | conditional: hard delete needs confirmation (AP-07) | none | Rust commands |
| CONTACT | ROLE-OWNER | allow | allow | allow | conditional (AP-07) | none | Rust commands |
| ACTIVITY | ROLE-OWNER | allow | allow | conditional: sent email read-only (AP-08) | conditional (AP-07) | none | Rust commands |
| SALES | ROLE-OWNER | allow | allow | conditional: not locked, not on/before book close (AP-04, AP-05); payments only via payment_save (AP-06) | conditional: AP-04, AP-05, AP-07 | no bulk delete (menus removed Delete All Records) | Rust commands |
| FILE | ROLE-OWNER | conditional: 20 MB, dialog paths only (AP-09) | allow | n/a | detach only | none | Rust commands |
| TEMPLATE | ROLE-OWNER | allow | allow | allow | allow | none | Rust commands |
| ADMIN | ROLE-OWNER | setup once | allow (secrets masked, AP-03) | allow | none | reference seed on fresh DB only | Rust commands |
| INTEG | ROLE-OWNER | n/a | n/a | n/a | n/a | outbound only to enabled hosts (AP-02, AP-12) | Rust commands |
| IMPORT | ROLE-OWNER | into a new database only | report | n/a | n/a | whole export | Rust commands |

## Record-level policies

| Policy id | Entity/operation | Exact source predicate | Target predicate and fields | Null/edge semantics | Enforcement owner | Tests |
|---|---|---|---|---|---|---|
| AP-04 | sales_transaction unlock/edit/delete | `Selected_SalesTransaction::date ≤ System::BookCloseDate` (`objects/karbon_kontacts.fmp12/scripts/335-btn_Unlock_Sales_Transaction_Id.txt` step 9; `objects/karbon_kontroller.fmp12/scripts/461-Delete_a_SalesTransaction_From_QBO_data.txt` step 68) | sales_transaction.date <= system_setting.book_close_date -> refuse | empty book_close_date -> no lock | Rust | T-08-04 |
| AP-05 | sales_transaction edit/delete | `SalesTransaction::LockedInd` (`objects/karbon_kontroller.fmp12/scripts/560-Delete_SalesTransaction_SalesTransactionId.txt` steps 55-56; the qbo_id check of steps 50-51 is removed with QBO, D-02) | locked_ind = 1 -> refuse | locked_ind empty = unlocked | Rust | T-08-05 |
| AP-08 | email message edit/send | already sent (`objects/karbon_kontroller.fmp12/scripts/522-Edit_EmailMessage_data.txt` steps 123-125) | email message sent flag set -> refuse | n/a | Rust | T-08-08 |

No ownership predicate exists in the source; none is invented (single user).

## Field-level policies

| Entity/field | Role | Read | Create/write | Update | Masking/omission | Source restriction | Tests |
|---|---|---|---|---|---|---|---|
| system_setting email account password; user SMTP password; API keys | ROLE-OWNER | never (has_secret flag only) | keychain only | keychain only | omitted from every response and log | none in source (plain fields, D-09) | T-08-03 |
| Employee SSN, card/bank numbers | ROLE-OWNER | no column | no column | no column | not imported (BR-IMPORT-02) | none in source | T-08-13 |
| organization.federal_tax_id_number, vendor.tax_id | ROLE-OWNER | allow | allow | allow | none: plain TEXT, no encryption, masking or extra access rule (owner, 2026-10-28; demo; D-09) | none in source | none (no rule to test) |

## Operation, script, and value-list access

Every 05 command is allowed to ROLE-OWNER through its registered route (AP-01, AP-13). Retired scripts (Commander, Re-login, Perform Script By Name) have no command (AP-10).

## Route/layout and access-channel matrix

| Route/page/channel | Source layout/extended privilege | Allowed | Underlying rules | Direct-access test |
|---|---|---|---|---|
| All page and fragment routes (06 UI-01..UI-21, routes in 05) | layouts of kontacts, Picker | ROLE-OWNER from the app window (AP-13) | command rules | a hand-made request with the session cookie still hits AP-04..AP-08 in the command |
| fmapp, fmwebdirect, fmxml, fmrest, fmphp, fmxdbc, fmurlscript, fmreauthenticate10, fmextscriptaccess | `objects/*/extended-privileges/` | none | no remote server (D-36) | loopback listener only (AP-10, AP-13) |

## UI visibility and exceptions

Hide rules gate no sensitive data in the core layouts (the explorers found none tied to an account or group); they are presentation only (06). No authorization gap rests on a hide rule.

## Session and global state

| FM global/`$$` value | Observed use | Target scope | Trusted for authorization? | Recomputation |
|---|---|---|---|---|
| session table, meta::CurrentUserId, Get(AccountName) | binds the account to a Karbon user (`objects/karbon_kontacts.fmp12/scripts/54-Store_UserID_On_Session.txt`) | app state: the single user id | no (single user) | read from user table at start |
| $$allow_commit | payment commit guard | removed (AP-06) | no | n/a |
| selector, Utility* globals, VirtualList | UI staging | form fields and Alpine.js state on the page (D-23) | no | n/a |

## Privilege bypass and application-level permissions

| Source | Evidence | What it bypasses or adds | Target policy |
|---|---|---|---|
| Commander scripts (evaluate parameter, reset/delete accounts) | `objects/karbon_kontroller.fmp12/scripts/6-Commander.txt` | account admin by fmp:// URL | not migrated (AP-10) |
| Admin menu requires [Full Access] | kontacts menus/scripts (D-20) | admin screens | single user: settings always available |
| Custom menus removing Delete All Records / Duplicate | `objects/karbon_kontacts.fmp12/custom-menus/` | bulk delete, duplicate invoice | no such commands exist |
| auth-files (requireAuthorization=False everywhere) | `objects/*/auth-files/` | file access between FileMaker files | n/a (one binary) |

## Service principals and integrations

| Principal/integration | Direction | Allowed operations/data | Secret owner/rotation | Network/access channel | Audit/tests |
|---|---|---|---|---|---|
| SMTP email | outbound | OP-INTEG-01 | keychain: smtp/<user> | lettre (SMTP, STARTTLS/TLS); user or system account | no retry; failure leaves message unsent and logged |
| Google Places / SmartyStreets address suggestions | outbound | OP-INTEG-02 | keychain: google_places, smartystreets | HTTPS JSON | no retry; UI shows no suggestions |
| FullContact enrichment | outbound | OP-INTEG-03 | keychain: fullcontact | HTTPS JSON, bearer key | no retry |
| QuickBooks Online (accounting) | none | OP-INTEG-04 | none | stubbed, not implemented: no-op command returning NotImplemented | n/a |
| Twilio SMS / chat | none | none | n/a | removed: out of scope (owner, 2026-10-28; OD-05-02) | n/a |

## Policies

| Policy id | Name | Rule | Applies to | Mechanism | Test |
|---|---|---|---|---|---|
| AP-01 | Command-only data access | The page receives HTML only and has no SQL, file-system or shell access. Only the commands of 07, called by the registered axum routes of 05, read or write SQLite; each route parses a typed form and each command runs its own transaction. | all commands | router lists exactly the 05 routes (unknown path 404); no Tauri IPC permission and no sql/fs/shell plugin is granted to the window | T-08-01 |
| AP-02 | Default-deny capabilities and network | The window gets no Tauri IPC permission at all: native dialogs and the keychain are used from Rust only. Outbound network calls happen only in Rust, only to the SMTP host and the integration hosts the user enabled. | OP-INTEG-01..03 | capability file; Rust HTTP client with a host allowlist | T-08-02 |
| AP-03 | Secrets never reach the UI or SQLite | SMTP passwords and API keys are written to the OS keychain by OP-ADMIN-03 and read only inside Rust commands; the settings page renders a has_secret flag, never the value, and a secret field is write-only. | OP-ADMIN-03, OP-INTEG-01..03 | keychain crate; templates have no secret field to render | T-08-03 |
| AP-04 | Book-close lock | Commands that unlock, change or delete a sales transaction refuse when its date <= system_setting.book_close_date (BR-SALES-04). | OP-SALES-04..08 | command precondition | T-08-04 |
| AP-05 | Posted transactions are immutable | A locked transaction (locked_ind = 1) cannot be changed or deleted (BR-SALES-05). | OP-SALES-04, 05, 07, 08 | command precondition | T-08-05 |
| AP-06 | Payments only through payment_save | Payment and payment-line rows are written only by OP-SALES-09, which checks BR-SALES-09; replaces the $$allow_commit commit guard. | OP-SALES-09 | no other command writes payment tables | T-08-06 |
| AP-07 | Destructive actions need confirmation | Hard party delete, transaction delete, activity discard, relation delete and contact-method delete require confirm = true; without it the command returns ConfirmationRequired and the route renders a confirmation dialog with a summary of what would be deleted. | OP-PARTY-04, OP-PARTY-09, OP-CONTACT-04, OP-ACTIVITY-03, OP-SALES-07, OP-SALES-08 | two-step command (05 Confirmations) | T-08-07 |
| AP-08 | Sent email is read-only | An email message marked sent cannot be edited or re-sent (BR-ACTIVITY-08). | OP-ACTIVITY-02, OP-INTEG-01 | command precondition | T-08-08 |
| AP-09 | File access is scoped | Stored files live only in the app data directory. file_add takes an uploaded multipart body, never a path. The import folder is chosen in a native dialog that the server opens in the desktop process; the route gets back a handle, not a path. No route takes a free path or URL from the page except the enrichment photo download (BR-FILE-01 limits apply). | OP-FILE-01, OP-IMPORT-01 | upload body or dialog handle only; canonicalize and check prefix | T-08-09 |
| AP-10 | No remote command entry | The app registers no URL scheme handler and no IPC listener that runs commands (the fmp:// Commander script is not migrated). Its only listener is the axum server on 127.0.0.1 (AP-13); nothing listens on another interface. | none | no deep-link plugin; loopback bind address only | T-08-10 |
| AP-11 | Optional app lock | Open: an optional passcode or encrypted database (OD-08-01). Default: none; the OS account protects the data. | app start | OD-08-01 | T-08-11 |
| AP-13 | Local server accepts only the app window | The server binds 127.0.0.1 on a random free port. At start the window opens `/session/{one-time token}`, which sets an HttpOnly, SameSite=Strict session cookie and redirects. Every route needs that cookie; the `Host` header must be `127.0.0.1:<port>` (DNS rebinding); a non-GET request needs a matching `Origin`. Another local process or a web page in a browser gets 401 or 403. | all routes | axum middleware; the token is random per launch and works once | T-08-14 |
| AP-12 | Integrations off by default | Each optional integration is enabled by the user in settings, with its key stored by AP-03; a disabled integration makes no network call. | OP-INTEG-02, OP-INTEG-03 | settings flag checked in the command | T-08-12 |

## Negative authorization tests

| Test id | Identity/role | Attempt | Expected | Layer | Evidence |
|---|---|---|---|---|---|
| T-08-01 | page script | request an unregistered route, or call Tauri IPC or the SQL plugin | 404; IPC not available to the window | HTTP, IPC | AP-01 |
| T-08-02 | command | HTTP call to a host not enabled | refused, logged | Rust HTTP client | AP-02 |
| T-08-03 | page | open the settings page | no secret value in the HTML | template | AP-03 |
| T-08-04 | ROLE-OWNER | unlock a transaction dated on the close date | BookClosed error | command | AP-04 |
| T-08-05 | ROLE-OWNER | edit or delete a locked transaction | Locked error | command | AP-05 |
| T-08-06 | ROLE-OWNER | write payment rows through invoice_save | not possible: no field accepted | command input schema | AP-06 |
| T-08-07 | ROLE-OWNER | hard delete without confirm | ConfirmationRequired | command | AP-07 |
| T-08-08 | ROLE-OWNER | edit a sent email | AlreadySent error | command | AP-08 |
| T-08-09 | page | file_add or import with a path in the form instead of an upload or dialog handle | refused | route | AP-09 |
| T-08-10 | other process | open an fmp:// or custom URL; connect to the app port from another computer | nothing handles it; connection refused (loopback bind) | OS, socket | AP-10 |
| T-08-11 | n/a | app lock (if OD-08-01 adopts it) | locked until passcode | app start | AP-11 |
| T-08-12 | ROLE-OWNER | address suggestion with the integration disabled | no network call | command | AP-12 |
| T-08-13 | import | export containing SSN/card columns | columns absent after import | import | BR-IMPORT-02 |
| T-08-14 | other local process / web page | request a route without the session cookie, with a foreign `Host`, or a POST with a foreign `Origin` | 401 / 403; nothing changes | HTTP middleware | AP-13 |

## Open decisions and unavailable evidence

| ID | Missing/conflicting evidence | Security impact | Required proof/owner | Status |
|---|---|---|---|---|
| OD-08-01 | Optional app passcode or encrypted database (SQLCipher) for a shared or unencrypted machine. | data at rest on a shared machine | Product owner | open |

## Completion gate

- [x] Every privilege-set dimension maps to target enforcement (built-in sets only; ROLE-OWNER or retired).
- [x] Every extended privilege has a retirement decision (no server).
- [x] Every request defaults deny outside the registered routes and the session check (AP-01, AP-13), with conditional rules AP-04..AP-08.
- [x] Field restrictions (secrets) are enforced before serialization (AP-03).
- [x] Record predicates reference real target columns (sales_transaction.date, sales_transaction.locked_ind, system_setting.book_close_date).
- [x] Hide/CF rules are secondary UI evidence only.
- [x] Identity provisioning covers the single user, retired accounts and the absence of service principals.
- [x] Negative tests go through HTTP to the local server, not only the UI.
