# Technology and Architecture Recommendation

The owner fixed the platform: a downloadable desktop app for macOS and Linux, Tauri with SQLite, one user per install, local data, no server (D-06, D-36; answered 2026-10-27). This file checks that choice against the decision matrix, picks the frontend, and confirms the choices made in 04 (SQLite dialect) and 05 (Tauri commands).

## Decision basis

No stakeholder weights were given, so the candidates are ranked by hard gates, then by fit (decision matrix rule).

| Constraint/requirement | Weight / hard gate | Evidence | Primary fit (Tauri 2 + SQLite) | Alternative fit (Electron + SQLite) |
|---|---|---|---|---|
| Existing repository compatibility | n/a | no target repository exists (D-36) | n/a | n/a |
| Owner-fixed stack | hard gate | D-36 | pass | fail (not the chosen stack) |
| macOS and Linux downloadable app, offline, local data | hard gate | D-06, D-37 | pass: native bundles (.app/.dmg, AppImage/.deb/.rpm), system webview | pass |
| Single user, no server | hard gate | D-35, D-36 | pass: embedded SQLite, commands in-process | pass |
| Team/delivery fit | unweighted | team unknown (D-36) | Rust for the command layer, TypeScript for the UI | TypeScript only |
| Authorization coverage | unweighted | 08 AP-01..AP-12 | strong: capability allowlist per window, Rust-only DB access | weaker: Node integration must be locked down by hand |
| Client/offline/accessibility needs | unweighted | D-37, D-38 | system webview (WebKit on macOS and Linux); WCAG via the component library | bundled Chromium: one engine on both platforms |
| Data/reporting/integration needs | unweighted | 04 (57 tables), 07 (money rules), EXT-01..03 | rusqlite (money as i64 cents, OD-04-02) + lettre (SMTP) + keyring | better-sqlite3 + decimal.js + nodemailer + keytar |
| Deployment/operations/compliance | unweighted | OD-02-02 | small installers (system webview); updater plugin; code signing needed | larger installers (~100 MB+) |
| Reversal/migration cost | unweighted | frontend is web tech in both | UI code moves; Rust commands would need a rewrite | n/a |

## Recommendation

Confirms 04 (SQLite dialect, D-36) and 05 (Tauri commands, D-36).

| Layer | Primary | Alternative | Rationale / tradeoff |
|---|---|---|---|
| Architecture/deployment unit | one Tauri 2 app per platform (macOS universal binary; Linux AppImage and .deb) | Electron app | owner choice; small, sandboxed IPC |
| Runtime/framework | Rust (stable) for the core: `src-tauri` with one module per domain (PARTY, CONTACT, ACTIVITY, SALES, FILE, TEMPLATE, ADMIN, INTEG, IMPORT) | Node.js main process | type-safe commands, single writer to SQLite |
| UI/rendering/client | **React 18 + TypeScript + Vite**, TanStack Query (command calls and cache), TanStack Table (dense lists), React Hook Form + Zod (forms), React Aria Components (accessible controls) | Svelte 5 + SvelteKit (static adapter) | see "Frontend choice" below |
| Database/data access | SQLite through rusqlite (bundled), WAL, foreign_keys on; refinery migrations | tauri-plugin-sql from the frontend | the plugin would give the webview SQL access, which AP-01 forbids |
| API/server-operation style | Tauri commands (05), typed errors (D-25); bindings generated with tauri-specta (OD-05-01) | hand-written invoke wrappers | no network API needed |
| Background jobs/events | none (D-10); long commands (import) report progress through Tauri events | n/a | no server, no schedules |
| File/container storage | content-addressed files in the app data directory (D-17) | BLOBs in SQLite | keeps the database small; easy backup |
| Authentication/identity provider | none; single local user (D-04); optional passcode (OD-08-01) | OS login only | owner answer |
| Hosting/compliance | none; data stays on the machine (D-36); keys in the OS keychain (D-09) | n/a | no PCI data stored (D-09) |
| Document/PDF generation | invoice/receipt HTML template printed to PDF by the webview, or `printpdf` in Rust (spike S-02) | headless Chromium | WebKitGTK print support varies on Linux |
| Integrations and secrets | lettre (SMTP), reqwest with rustls (HTTPS), keyring crate (Keychain / Secret Service) | nodemailer, keytar | all outbound from Rust (AP-02) |
| Authorization enforcement | Tauri capabilities allowlist + command preconditions (08) | n/a | default deny |
| Testing/observability/deployment | cargo test with an in-memory SQLite per rule fixture; Vitest + Testing Library; WebDriver (tauri-driver) smoke tests on Linux; tracing to a rotating log file (D-30); GitHub Actions matrix build | Playwright on Electron | tauri-driver has no macOS WebDriver; macOS UI checks are manual (risk R-03) |

### Frontend choice: React + TypeScript

- The UI is about 20 data-dense screens of lists, detail forms, dialogs and portals (06). React has the most mature libraries for exactly that: TanStack Table (virtualized sortable tables), React Hook Form with Zod (validation that mirrors the typed command errors), and React Aria Components (keyboard and screen-reader behavior for WCAG 2.1 AA, D-38).
- TypeScript types for every command can be generated from the Rust structs (tauri-specta, OD-05-01), so a renamed field breaks the build instead of the app.
- Tauri has first-class React + Vite templates; the frontend is a static bundle, so no SSR framework is needed.
- Svelte 5 would give a smaller, simpler component model and works equally well with Tauri, but its ecosystem for accessible data grids and complex forms is thinner. With no team preference known, React carries less library risk. Reverse this if the team prefers Svelte: the command layer does not change.

## Existing-stack decision

No target repository exists yet (the repository holds only the FileMaker files and docs); this section is skipped.

## Authorization architecture

| Source dimension | Target owner/mechanism | Default deny | Negative-test strategy |
|---|---|---|---|
| Roles/capabilities | ROLE-OWNER only (08) | yes: capability allowlist | T-08-01 |
| Record predicates | command preconditions AP-04, AP-05, AP-08 | yes | T-08-04, T-08-05, T-08-08 |
| Field read/write | serializer omits secrets (AP-03); excluded columns (BR-IMPORT-02) | yes | T-08-03, T-08-13 |
| Operations/scripts/value lists | registered commands only (05) | yes | T-08-01 |
| Routes/layouts | React routes; no guard needed for a single user | n/a | commands still enforce |
| Extended privileges/access channels | none: no listener (AP-10) | yes | T-08-10 |
| Service principals | none | n/a | n/a |
| UI visibility | component state from command results | secondary only | n/a |

## Interface style

Tauri commands, because the only client is the app's own webview in the same process. A REST or GraphQL layer would add a network surface with no consumer. Commands are use-case shaped (05), not per-table CRUD.

## Migration strategy

Per-install import, not a cutover (D-34). Increments in 03 set build order; the app ships to users only when I1-I5 are done, because parties, activities and sales share core tables and FileMaker gets no write-back. Each user then exports their FileMaker data and imports it; they can repeat the import into a fresh database until the result is right, and keep FileMaker until they stop using it. Authorization and core invariants are in the command layer from I1.

## Current external evidence

| Claim affecting choice | Primary source/version | Access date | Fact or inference |
|---|---|---|---|
| Tauri 2 is stable and builds for macOS and Linux with the system webview (WKWebView, WebKitGTK) | Tauri 2.x documentation (tauri.app) | not re-checked online on 2026-10-27; from model knowledge | inference until re-verified (spike S-01) |
| Tauri capabilities restrict which commands and plugins a window may use | Tauri 2 security/capabilities docs | same | inference until re-verified |
| tauri-driver WebDriver testing works on Linux and Windows, not macOS | Tauri testing docs | same | inference until re-verified |
| rusqlite bundles SQLite; keyring supports macOS Keychain and Linux Secret Service | crates.io / docs.rs | same | inference until re-verified |

Re-check these against the current releases before I1 starts (spike S-01).

## Risks, spikes, and decisions

| Risk/unknown | Impact | Proof/spike | Owner | Decision deadline |
|---|---|---|---|---|
| R-01 WebKitGTK differences on Linux (rendering, print, fonts) | UI defects on Linux | S-01: hello-world with TanStack Table, a dialog and print-to-PDF on Ubuntu LTS and Fedora | Tech lead | before I1 |
| R-02 Money conversion at the edges | wrong cents from dollar input or from the FileMaker decimal export | Closed as a storage risk: money is INTEGER cents (owner, 2026-10-28; OD-04-02), so sums are exact. S-03 now tests only the edges: UI dollar parsing and formatting, and import rounding half away from zero, including 7-decimal unit prices (BR-SALES-01, BR-IMPORT-01) | Tech lead | before I3 |
| R-03 No macOS WebDriver | manual UI testing on macOS | test plan with Linux automation + macOS checklist | Tech lead | I1 |
| R-04 FileMaker export shape per install | import fails on real data | S-04: export one real file set (OD-03-01) and dry-run the importer | Operator | before I3 |
| R-05 Code signing / notarization | macOS Gatekeeper blocks the app | Apple Developer ID; CI notarization (OD-02-02) | Product owner | before first release |
| R-06 PDF generation quality | invoices print badly | S-02: webview print vs printpdf | Tech lead | before I5 |

## Rejected alternative

Electron + SQLite fails the owner's hard gate (Tauri was chosen) and would ship a full Chromium and Node runtime per install. It wins on one point: the same rendering engine on macOS and Linux, which removes R-01 and R-03. If the Linux WebKitGTK spike (S-01) fails badly, that is the case for reopening D-36. For the frontend, Svelte 5 lost only on library depth for accessible tables and forms; a team preference for Svelte reverses that choice at no cost to the Rust side.

## Open decisions

| ID | Decision | Rests on | Owner |
|---|---|---|---|
| OD-02-02 | Code signing and notarization: Apple Developer ID for macOS; Linux package formats (AppImage, .deb, .rpm) and the update channel. | D-36 | Product owner |
