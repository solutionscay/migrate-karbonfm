# migrate-karbonfm

A test case for the [migrate-filemaker](https://github.com/solutionscay/migrate-filemaker) skill, run against KarbonFM, an open-source FileMaker framework.

The goal is to port KarbonFM's core application to a downloadable desktop app for macOS and Linux. Tauri is the leading candidate, but the stack is not decided yet. The skill's `02_recommendations.md` will confirm it.

## What KarbonFM is

Karbon for FileMaker (karbonfm) is a developer framework for building FileMaker applications. The application is a contact management system (CRM). The framework's value is in its approach to data modeling, transactional scripting, and its library of custom functions for error handling and validation. See the [Karbon Manifesto](https://www.geistinteractive.com/2018/07/30/the-karbon-manifesto/).

KarbonFM was made by Geist Interactive (EOW, Inc.) and is released under the MIT License. This repo is a fork of their work. The original README and license are in [KarbonFm-Source/README.md](KarbonFm-Source/README.md) and [KarbonFm-Source/LICENSE.txt](KarbonFm-Source/LICENSE.txt).

## Layout

| Path | Contents |
|---|---|
| `KarbonFm-Source/` | The original KarbonFM release: the `.fmp12` files in `KarbonFM/`, the original README, logo and license. |
| `DDR/` | The DDR XML exports (13 files plus `Summary.xml`). Git-ignored. |
| `migration/` | The skill's output: `00_app_summary.md`, `01_discovery_*`, and later `02` to `09`. |
| `migration/objects/`, `migration/explorers/` | The extracted object files and the explorer catalogs and CSVs. Git-ignored. |

## Known gaps

- `Karbon.fmp12` is a launcher file. It has no DDR, so the plan has no objects for it.
- `Party_Merge_Utility.fmp12` has no DDR. `karbon_kontacts` references it (data source id 14), so its objects are unavailable.

## Rerun the skill

Set the paths:

```bash
SKILL_DIR="/path/to/migrate-filemaker"
RAW_DIR="$PWD/DDR"
OUT_DIR="$PWD/migration"
```

Step 1, extract every object from the DDR:

```bash
python3 "$SKILL_DIR/scripts/explode_ddr.py" "$RAW_DIR" "$OUT_DIR/objects"
```

Step 3, snapshot each explorer's kinds (repeat `--kind` for an explorer that covers several kinds):

```bash
python3 "$SKILL_DIR/scripts/catalog_contract.py" snapshot \
  --objects "$OUT_DIR/objects" --kind scripts --output "$OUT_DIR/explorers/scripts/catalog.json"
```

Split the batches across agents:

```bash
python3 "$SKILL_DIR/scripts/catalog_contract.py" assign --agents 20 --output "$OUT_DIR/explorers/assignments" \
  --catalog "$OUT_DIR/explorers/layouts/catalog.json" --catalog ...
```

Verify that every object has exactly one row:

```bash
python3 "$SKILL_DIR/scripts/catalog_contract.py" verify \
  --catalog "$OUT_DIR/explorers/scripts/catalog.json" --csv-root "$OUT_DIR/explorers/scripts/csv"
```

See the skill's `SKILL.md` for the full steps.

## Status

The migrate-filemaker run is done: steps 1-9, with 4,768 objects in 13 files and all explorers verified. The plan files:

- [00_app_summary.md](migration/00_app_summary.md): inventory of files, objects and references
- [01_discovery_answers.md](migration/01_discovery_answers.md): discovery questions and default answers
- [02_recommendations.md](migration/02_recommendations.md): target stack
- [03_migration_plan.md](migration/03_migration_plan.md): increments, data migration and cutover
- [04_database_schema.sql](migration/04_database_schema.sql): target schema
- [05_api_design.md](migration/05_api_design.md): commands and integrations
- [06_ui_spec.md](migration/06_ui_spec.md): screens
- [07_business_logic.md](migration/07_business_logic.md): business rules
- [08_auth_roles.md](migration/08_auth_roles.md): roles and access policies
- [09_audit.md](migration/09_audit.md): audit of the plan and open gaps
