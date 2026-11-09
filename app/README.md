# KarbonFM desktop app

Tauri 2 window on a local axum server (127.0.0.1), server-rendered askama pages with htmx, Alpine.js, Chart.js and
Tailwind/DaisyUI, SQLite through rusqlite. See `migration/02_recommendations.md` and spikes S-01, S-05.

```bash
cargo run -p karbon-desktop                          # the app (window); a new data folder gets the big demo
cargo run -p karbon-server --bin karbon-dev-server   # the same pages in a browser: open the one-time URL it prints
cargo run -p karbon-server --bin karbon-dev-server -- 3000 --fresh   # a new install (no demo seed): starts on /setup
KARBONFM_DATA_DIR=/tmp/kfm cargo run -p karbon-desktop --no-default-features   # fresh non-demo app, data in /tmp/kfm
KARBONFM_DATA_DIR=/tmp/kfm-demo cargo run -p karbon-desktop   # big demo app with its data in /tmp/kfm-demo (try backup/restore)
KARBONFM_DATA_DIR=/tmp/kfm-small cargo run -p karbon-desktop --no-default-features --features demo   # the small 04_demo_seed
cargo run -p karbon-server --bin karbon-dev-server -- 3000 --data-dir /tmp/kfm-dev --backup-folder /tmp/kfm-backups   # backup/restore in a browser
cargo run --release -p karbon-server --features big-demo --bin karbon-dev-server -- 3917 --big-demo   # ~470 parties, ~3,700 invoices (I3)
cargo run --release -p karbon-synth -- /tmp/kfm-export                                  # the synthetic FileMaker export (never commit it)
cargo run --release -p karbon-server --bin karbon-import -- /tmp/kfm-export --data-dir /tmp/kfm-big   # import it into a new database
cargo run -p karbon-server --bin karbon-dev-server -- 3917 --data-dir /tmp/kfm-big                    # serve that database
./tools/build-css.sh                                 # rebuild server/assets/app.css after changing templates or styles
cargo test && cargo clippy --all-targets -- -D warnings
cd desktop && cargo tauri build                      # bundles (needs: cargo install tauri-cli --version 2.12.4 --locked)
```

The WebDriver smoke test (03 Test strategy, R-03) runs on Windows and Linux in CI (`app.yml`, job `smoke`):
`karbon-smoke` starts the release binary under tauri-driver on a new temp data folder (big demo), opens the
dashboard and INV-1048, checks the title and a few demo figures on each, and writes `dashboard.png`,
`invoice-INV-1048.png`, `summary.txt` and the logs (artifact `smoke-Windows` / `smoke-Linux`). tauri-driver has no
macOS support: the macOS window stays on the manual checklist. On Linux, with `cargo install tauri-driver --version
2.1.0 --locked`, `webkit2gtk-driver` and `xvfb`:

```bash
cargo build --release -p karbon-desktop --features karbon-desktop/custom-protocol -p karbon-smoke
xvfb-run -a -s "-screen 0 1600x1000x24" ./target/release/karbon-smoke --app target/release/karbonfm --out /tmp/kfm-smoke
```

| Path | What |
|---|---|
| `server/` | routes, templates, embedded assets, database open; no Tauri dependency |
| `desktop/` | Tauri shell: starts the server, opens the window, gives the server the print hook |
| `styles/` | Tailwind source, `karbon.css` (from design/), vendored DaisyUI plugin |
| `tools/build-css.sh` | Tailwind standalone CLI, pinned and checksummed; no Node |

The database is `karbonfm.db` in the app data folder (macOS `~/Library/Application Support/com.solutionscay.karbonfm`),
migrated by refinery at every start. When the folder has no database, the app creates the big demo (karbon-synth
imported by karbon-import, about 0.7 s in release); it never replaces an existing one, so move `karbonfm.db` and
`files/` away to get a new demo. The log is `karbonfm.log` in the app log folder (macOS `~/Library/Logs/com.solutionscay.karbonfm`). Rule and
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
