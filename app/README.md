# KarbonFM desktop app

Tauri 2 window on a local axum server (127.0.0.1), server-rendered askama pages with htmx, Alpine.js, Chart.js and
Tailwind/DaisyUI, SQLite through rusqlite. See `migration/02_recommendations.md` and spikes S-01, S-05.

```bash
cargo run -p karbon-desktop                          # the app (window)
cargo run -p karbon-server --bin karbon-dev-server   # the same pages in a browser: open the one-time URL it prints
cargo run -p karbon-server --bin karbon-dev-server -- 3000 --fresh   # a new install (no demo seed): starts on /setup
KARBONFM_DATA_DIR=/tmp/kfm cargo run -p karbon-desktop --no-default-features   # fresh non-demo app, data in /tmp/kfm
KARBONFM_DATA_DIR=/tmp/kfm-demo cargo run -p karbon-desktop   # demo app with its data in /tmp/kfm-demo (try backup/restore)
cargo run -p karbon-server --bin karbon-dev-server -- 3000 --data-dir /tmp/kfm-dev --backup-folder /tmp/kfm-backups   # backup/restore in a browser
./tools/build-css.sh                                 # rebuild server/assets/app.css after changing templates or styles
cargo test && cargo clippy --all-targets -- -D warnings
cd desktop && cargo tauri build                      # bundles (needs: cargo install tauri-cli --version 2.12.4 --locked)
```

| Path | What |
|---|---|
| `server/` | routes, templates, embedded assets, database open; no Tauri dependency |
| `desktop/` | Tauri shell: starts the server, opens the window, gives the server the print hook |
| `styles/` | Tailwind source, `karbon.css` (from design/), vendored DaisyUI plugin |
| `tools/build-css.sh` | Tailwind standalone CLI, pinned and checksummed; no Node |

The database is `karbonfm.db` in the app data folder (macOS `~/Library/Application Support/com.solutionscay.karbonfm`),
migrated by refinery at every start; the demo build loads `04_demo_seed.sql` while it has no party, so delete the file to
reseed. The log is `karbonfm.log` in the app log folder (macOS `~/Library/Logs/com.solutionscay.karbonfm`). Rule and
folders per OS: `migration/03_migration_plan.md`, "Demo seed data".

The local server only answers the app window (AP-13): the window opens a one-time `/session/{token}` URL that sets a
session cookie, and every other request without that cookie gets 401. The dev server prints its one-time URL
(`http://127.0.0.1:3000/session/...`); open it once per run, then reload and browse as usual in that browser. Restart
the dev server for a new URL. Tests: `server/tests/auth.rs` (T-08-01, 02, 03, 10, 14) and `desktop/tests/no_ipc.rs`;
`cargo test -p karbon-server keychain_round_trip -- --ignored` writes and deletes one test entry in the OS keychain.

Backup and restore are on Settings (UI-17, OP-ADMIN-06/07). "Back up now…" asks for a folder and writes
`KarbonFM backup <date time>/` there: `karbonfm.db` (a `VACUUM INTO` copy), `files/` (the file store) and
`manifest.json`. Passwords and API keys are not in it; they stay in the keychain. "Restore from a backup…" asks for a
backup folder, checks it, shows what is in it in a dialog and, after Restore, keeps the current data in
`<data folder>/before-restore/` before swapping the backup in. The dev server has no folder dialog: `--backup-folder`
stands in for it (backups go there; a restore offers the newest one), and `--data-dir` keeps the database in a file.
Tests: `server/tests/backup.rs`, `server/tests/backup_routes.rs`.
