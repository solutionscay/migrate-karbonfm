# KarbonFM desktop app

Tauri 2 window on a local axum server (127.0.0.1), server-rendered askama pages with htmx, Alpine.js, Chart.js and
Tailwind/DaisyUI, SQLite through rusqlite. See `migration/02_recommendations.md` and spikes S-01, S-05.

```bash
cargo run -p karbon-desktop                          # the app (window)
cargo run -p karbon-server --bin karbon-dev-server   # the same pages in a browser, http://127.0.0.1:3000/
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
