# Technology and Architecture Recommendation

The owner fixed the platform: a downloadable desktop app, Tauri with SQLite, one user per install, local data, no remote server (D-06, D-36; answered 2026-10-27), on all three desktop platforms, macOS, Windows and Linux, from one Tauri codebase, with unsigned builds for the demo (owner, 2026-10-28; D-37, OD-02-02). The visual design is design/ ("Carbon & Ember", OD-06-01). On 2026-10-28 the owner also fixed the UI stack: server-rendered HTML, for simplicity, with no React or TypeScript. This file checks those choices against the decision matrix and confirms the choices made in 04 (SQLite dialect) and 05 (axum routes that return HTML).

## Decision basis

No stakeholder weights were given, so the candidates are ranked by hard gates, then by fit (decision matrix rule).

| Constraint/requirement | Weight / hard gate | Evidence | Primary fit (Tauri 2 + axum/htmx + SQLite) | Alternative fit (Electron + SQLite) |
|---|---|---|---|---|
| Existing repository compatibility | n/a | no target repository exists (D-36) | n/a | n/a |
| Owner-fixed stack | hard gate | D-36; owner, 2026-10-28 (UI stack) | pass | fail (not the chosen stack) |
| macOS, Windows and Linux downloadable app, offline, local data | hard gate | D-06, D-37 (owner, 2026-10-28) | pass: one codebase, native bundles (.app/.dmg, .msi/NSIS .exe, AppImage/.deb/.rpm), system webview, all assets embedded in the binary | pass |
| Single user, no remote server | hard gate | D-35, D-36 | pass: embedded SQLite; the HTTP server runs inside the app process on 127.0.0.1 | pass |
| Team/delivery fit | unweighted | team unknown (D-36) | one language (Rust) for logic, routes and templates; HTML with htmx attributes; no JavaScript build | JavaScript/TypeScript for the whole Electron app |
| Authorization coverage | unweighted | 08 AP-01..AP-13 | strong: the page gets HTML only; every write goes through a Rust handler; the window has no Tauri IPC permission | weaker: Node integration must be locked down by hand |
| Client/offline/accessibility needs | unweighted | D-37, D-38 | system webview (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux); semantic HTML with DaisyUI components | bundled Chromium: one engine on all three platforms |
| Data/reporting/integration needs | unweighted | 04 (57 tables), 07 (money rules), EXT-01..03 | rusqlite with plain SQL (money as i64 cents, OD-04-02) + lettre (SMTP) + keyring; Chart.js for charts | better-sqlite3 + decimal.js + nodemailer + keytar |
| Deployment/operations/compliance | unweighted | OD-02-02 | small installers (system webview); unsigned builds on all three platforms for the demo (owner, 2026-10-28); signing only for a real release | larger installers (~100 MB+) |
| Reversal/migration cost | unweighted | owner decision 2026-10-28 | templates and routes are Rust; the command layer does not depend on them | n/a |

## Recommendation

Confirms 04 (SQLite dialect, D-36) and 05 (axum routes returning HTML, owner decision 2026-10-28).

| Layer | Primary | Alternative | Rationale / tradeoff |
|---|---|---|---|
| Architecture/deployment unit | one Tauri 2 codebase built for each platform: macOS universal binary (.app/.dmg), Windows x64 (.msi and NSIS .exe; WebView2 bootstrapper), Linux AppImage and .deb (owner, 2026-10-28; D-37). At start it runs an axum server on 127.0.0.1 (random free port) inside the same process and opens one window on it | Electron app | owner choice; one process, loopback only, no network surface |
| Runtime/framework | Rust (stable): axum on tokio; one module per domain (PARTY, CONTACT, ACTIVITY, SALES, FILE, TEMPLATE, ADMIN, INTEG, IMPORT) with `commands/<domain>.rs` (the rules, 07) and `routes/<domain>.rs` (the handlers, 05) | Node.js main process | type-safe handlers and commands, single writer to SQLite |
| UI/rendering/client | **Server-rendered HTML** (owner, 2026-10-28): askama templates compiled into the binary; htmx for interaction (requests return HTML fragments that htmx swaps in); Alpine.js only for small client-side bits (open/close, tabs, a confirm toggle); Tailwind CSS + DaisyUI for styling; Chart.js for charts. No React, no TypeScript, no JavaScript build step | none | see "UI stack" below |
| Database/data access | SQLite through rusqlite (bundled) with plain SQL, no ORM; WAL, foreign_keys on; refinery migrations | tauri-plugin-sql | the plugin would give the page SQL access, which AP-01 forbids |
| API/server-operation style | axum routes (05): a full page for a normal request, an HTML fragment for an htmx request (`HX-Request` header); typed errors rendered as error fragments (D-25) | JSON API | the only client is the app's own HTML |
| Background jobs/events | none (D-10); the long import reports progress through a status fragment that htmx polls | server-sent events | no server schedules; polling is the simplest |
| File/container storage | files copied into the app data folder; the table stores the path (owner, 2026-10-28; D-17); uploads arrive as multipart form posts | BLOBs in SQLite | keeps the database small; easy backup |
| Static assets | htmx, Alpine.js, Chart.js and the built Tailwind/DaisyUI CSS are vendored at pinned versions and embedded in the binary; no CDN | CDN links | the app works offline |
| Authentication/identity provider | none; single local user on one computer (D-04); the local server listens on 127.0.0.1 only and is protected by a per-launch session cookie (AP-13); optional passcode (OD-08-01) | OS login only | owner answer |
| Hosting/compliance | none; data stays on the machine (D-36); keys in the OS keychain (D-09) | n/a | no PCI data stored (D-09) |
| Document/PDF generation | invoice askama template (the only print output, owner, 2026-10-28; D-38) printed to PDF by the webview, or `printpdf` in Rust (spike S-02) | headless Chromium | print support differs between the three webviews (WebKitGTK on Linux most of all) |
| Integrations and secrets | lettre (SMTP), reqwest with rustls (HTTPS), keyring crate (Keychain / Secret Service) | nodemailer, keytar | all outbound from Rust (AP-02) |
| Authorization enforcement | route guards (session, Host and Origin checks, AP-13) + command preconditions (08) | n/a | default deny |
| Testing/observability/deployment | cargo test with an in-memory SQLite per rule fixture; route tests that call the axum router in process and check the HTML with the `scraper` crate; WebDriver (tauri-driver) smoke tests on Windows and Linux; tracing to a rotating log file (D-30); GitHub Actions matrix build | Playwright on Electron | tauri-driver has no macOS WebDriver; macOS UI checks are manual (risk R-03) |

### UI stack: server-rendered HTML (owner decision, 2026-10-28)

- The UI is about 20 data-dense screens of lists, detail forms, dialogs and portals (06). Each FileMaker layout becomes an askama page; each portal, card and dialog becomes a fragment the page loads or replaces with htmx. FileMaker's model (a layout shows records, buttons run scripts) maps directly onto "a page shows rows, a button posts to a route".
- All logic stays in Rust: the route handler parses the form, calls the command (07) and renders the result. There is no second type system to keep in step with Rust, so the typed-bindings question (OD-05-01) is closed.
- Dense lists are server-paged and server-sorted tables (DaisyUI `table`); htmx swaps the table body. Dialogs are DaisyUI modals on the HTML `<dialog>` element. Alpine.js is used only where a few lines of state are simpler on the client (a dropdown, a tab strip).
- Tailwind is built with the standalone CLI and the DaisyUI plugin file, so the build needs no Node toolchain (spike S-05).
- The app is single-user on one computer: the server listens on 127.0.0.1 only and serves nothing to other computers (owner, 2026-10-28).

## Existing-stack decision

No target repository exists yet (the repository holds only the FileMaker files and docs); this section is skipped.

## Authorization architecture

| Source dimension | Target owner/mechanism | Default deny | Negative-test strategy |
|---|---|---|---|
| Roles/capabilities | ROLE-OWNER only (08) | yes: unknown routes 404; no session 401 (AP-13) | T-08-01, T-08-14 |
| Record predicates | command preconditions AP-05, AP-08 | yes | T-08-05, T-08-08 |
| Field read/write | templates never render secrets (AP-03); excluded columns (BR-IMPORT-02) | yes | T-08-03, T-08-13 |
| Operations/scripts/value lists | registered routes only (05) | yes | T-08-01 |
| Routes/layouts | page routes; no guard beyond the session for a single user | n/a | commands still enforce |
| Extended privileges/access channels | loopback listener only (AP-10, AP-13) | yes | T-08-10, T-08-14 |
| Service principals | none | n/a | n/a |
| UI visibility | template conditionals on the command result | secondary only | n/a |

## Interface style

HTTP routes on a local axum server that return HTML (owner decision, 2026-10-28). The only client is the app's own window; the server listens on 127.0.0.1 only. Routes are use-case shaped (05), not per-table CRUD, and every route calls one command of 07.

## Migration strategy

Per-install import, not a cutover (D-34). Increments in 03 set build order; the app ships to users only when I1-I5 are done, because parties, activities and sales share core tables and FileMaker gets no write-back. Each user then exports their FileMaker data and imports it; they can repeat the import into a fresh database until the result is right, and keep FileMaker until they stop using it. Authorization and core invariants are in the command layer from I1.

## Current external evidence

| Claim affecting choice | Primary source/version | Access date | Fact or inference |
|---|---|---|---|
| Tauri 2 is stable and builds for macOS, Windows and Linux with the system webview (WKWebView, WebView2, WebKitGTK) | Tauri 2.x documentation (tauri.app) | not re-checked online on 2026-10-28; from model knowledge | inference until re-verified (spike S-01) |
| A Tauri window can load a localhost URL served by the same process, with no IPC permission granted to it | Tauri 2 security/capabilities docs | same | inference until re-verified (spike S-01) |
| axum, askama and rusqlite build on stable Rust and embed into a Tauri app | docs.rs | same | inference until re-verified |
| The Tailwind standalone CLI can build DaisyUI styles without Node | Tailwind and DaisyUI docs | same | inference until re-verified (spike S-05) |
| tauri-driver WebDriver testing works on Linux and Windows, not macOS | Tauri testing docs | same | inference until re-verified |
| keyring supports macOS Keychain, Windows Credential Manager and Linux Secret Service | crates.io / docs.rs | same | inference until re-verified |

Re-check these against the current releases before I1 starts (spikes S-01, S-05).

## Risks, spikes, and decisions

The React and TypeScript items are gone with the 2026-10-28 decision: the TanStack Table part of S-01 and the typed-bindings decision OD-05-01.

| Risk/unknown | Impact | Proof/spike | Owner | Decision deadline |
|---|---|---|---|---|
| R-01 Webview differences between the three platforms (WKWebView, WebView2, WebKitGTK: rendering, print, fonts) | UI defects on one platform, most likely Linux | S-01: a Tauri window on the local axum server with an htmx-paged DaisyUI table, a `<dialog>` modal, a Chart.js chart and print-to-PDF, on macOS, Windows 11, Ubuntu LTS and Fedora (owner, 2026-10-28; D-37) | Tech lead | before I1 |
| R-02 Money conversion at the edges | wrong cents from dollar input or from the FileMaker decimal export | Closed as a storage risk: money is INTEGER cents (owner, 2026-10-28; OD-04-02), so sums are exact. S-03 tests only the edges: dollar parsing in the form handlers, formatting in the templates, and import rounding half away from zero, including 7-decimal unit prices (BR-SALES-01, BR-IMPORT-01) | Tech lead | before I5 for the form and template edges; the import edge waits for I3 (I3 deferred: no source export; the demo runs on 04_demo_seed.sql) |
| R-03 No macOS WebDriver | manual UI testing on macOS | route tests cover the HTML on all three platforms in process; Windows and Linux WebDriver smoke tests + macOS checklist | Tech lead | I1 |
| R-04 FileMaker export shape per install | import fails on real data | S-04: export one real file set (OD-03-01) and dry-run the importer | Operator | deferred: no source export; the demo runs on 04_demo_seed.sql (was: before I3) |
| R-05 Code signing / notarization | macOS Gatekeeper and Windows SmartScreen warn on an unsigned app | Closed for the demo: unsigned builds on all three platforms (owner, 2026-10-28; OD-02-02); on macOS the user opens it with right-click > Open, on Windows with "More info" > "Run anyway". Apple Developer ID with notarization and a Windows code-signing certificate only for a real release | Product owner | closed |
| R-06 PDF generation quality | invoices print badly | S-02: webview print vs printpdf | Tech lead | before I5 |
| R-07 Local server reachable by other local processes or by web pages (cross-site requests, DNS rebinding) | another program reads or changes data | AP-13: bind 127.0.0.1 only, per-launch session cookie (HttpOnly, SameSite=Strict), Host and Origin checks on every request; T-08-14 | Tech lead | I1 |
| R-08 Tailwind/DaisyUI build without Node | CSS build breaks or needs Node in CI | S-05: build the DaisyUI theme with the Tailwind standalone CLI in CI on macOS, Windows and Linux | Tech lead | before I1 |

## Rejected alternative

Electron + SQLite fails the owner's hard gate (Tauri was chosen) and would ship a full Chromium and Node runtime per install. It wins on one point: the same rendering engine on all three platforms, which removes R-01 and R-03. If the webview spike (S-01) fails badly on one platform, that is the case for reopening D-36. A React or Svelte single-page frontend was the earlier recommendation; the owner replaced it on 2026-10-28 with server-rendered HTML for simplicity.

## Open decisions

| ID | Decision | Rests on | Owner |
|---|---|---|---|
| OD-02-02 | Code signing and notarization. **Decided (owner, 2026-10-28):** unsigned builds for the demo. | D-36 | Owner / closed |
