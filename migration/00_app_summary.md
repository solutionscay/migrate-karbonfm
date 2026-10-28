# Application Summary

Cite evidence as object file paths: `objects/<file>/<kind>/<id>-<name>.txt`.

Labels: **fact** (from an object file or the DDR), **inference**, **product decision**, **unavailable**. Step 3 (the explorers) has not run yet, so every column that needs a reading of the objects is `unresolved` and points to the explorer that will fill it.

## Source

| Item | Value |
|---|---|
| DDR export (folder, date, FileMaker version) | fact: `DDR/` (13 DDR XML files and `Summary.xml`). Made 10/27/2026 at 8:03:52 PM with FileMaker 26.0.3 (`Summary.xml` root; `# ddr:` header of each `objects/<file>/file/0-options.txt`). Host path of the source files: `.../migrate-karbonfm/KarbonFM/`. |
| Object files (`objects/index.tsv` rows) | fact: 4,768 object files, 399,692 object lines. |
| Extraction errors | fact: none. `explode_ddr.py` passed every calculation check (each file: calculations in raw XML = calculations shown, 38,711 in all) and wrote 4,768 objects. It skipped 296 separators (27 layout and 269 script separators; see the note under Application profile). `objects/unresolved.tsv` has 488 rows: external 89, missing 260, none 139. |

## Application profile

| Attribute | Value | Evidence |
|---|---|---|
| Name and purpose | inference: "Karbon for FileMaker" (karbonfm), a developer framework built as a contact-management / CRM application. This comes from the repository README (`git show HEAD:README.md`), not from the DDR. fact: every file carries a version calculation; `karbon_log` returns "0.9.5". The latest repository commit is "Release 0.9.6". | `objects/karbon_log.fmp12/tables/129-meta.txt` (field 6 `FileVersion`) |
| FileMaker files | fact: 13 files with a DDR (see the table below). Two files have **no DDR**: `Karbon.fmp12` (a launcher file, operator-supplied fact) and `Party_Merge_Utility.fmp12` (referenced by `karbon_kontacts`, FileReference id 14). | `objects/*/file/0-options.txt`; `objects/karbon_kontacts.fmp12/data-sources/14-Party_Merge_Utility.txt` |
| Tables / table occurrences / relationships | fact: 195 base tables (3,071 fields) / 784 table occurrences / 683 relationships | `objects/*/tables/`, `objects/*/table-occurrences/`, `objects/*/relationships/` |
| Layouts | fact: 382 layout objects (`Summary.xml` counts 409; the other 27 are separators) | `objects/*/layouts/` |
| Scripts | fact: 1,369 script objects (`Summary.xml` counts 1,638; the other 269 are separators) | `objects/*/scripts/` |
| Custom functions | fact: 594 (the same library is repeated in 10 files: 45 to 102 per file) | `objects/*/custom-functions/` |
| Value lists | fact: 116 | `objects/*/value-lists/` |
| Privilege sets / accounts / extended privileges | fact: 40 / 27 / 119 | `objects/*/privilege-sets/`, `objects/*/accounts/`, `objects/*/extended-privileges/` |
| Custom menus / menu sets | fact: 315 / 18 | `objects/*/custom-menus/`, `objects/*/custom-menu-sets/` |

Fact: the extractor's counts match `Summary.xml` for every file and kind, except for the layout and script separators. (`Summary.xml` "Tables" counts table occurrences, and "FileReferences" counts data sources.)

### Objects per file and kind

Fact, from `cut -f1,2 objects/index.tsv | sort | uniq -c`.

| File | accounts | auth-files | base-directories | custom-functions | custom-menu-sets | custom-menus | data-sources | extended-privileges | file | layouts | privilege-sets | relationships | scripts | table-occurrences | tables | value-lists | total | object lines |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Picker Module.fmp12 | 2 | 1 | 2 | 58 | 2 | 27 | 4 | 10 | 1 | 21 | 3 | 21 | 80 | 22 | 6 | 2 | 262 | 16,983 |
| ZeitGeist.fmp12 | 2 | 1 | 1 | 0 | 1 | 24 | 0 | 9 | 1 | 2 | 3 | 0 | 8 | 1 | 1 | 0 | 54 | 769 |
| karbon_Build.fmp12 | 2 | 2 | 2 | 53 | 1 | 24 | 21 | 9 | 1 | 14 | 3 | 34 | 31 | 35 | 12 | 3 | 247 | 11,262 |
| karbon_Generator.fmp12 | 2 | 2 | 2 | 102 | 2 | 1 | 2 | 10 | 1 | 59 | 3 | 61 | 240 | 72 | 32 | 31 | 622 | 55,099 |
| karbon_LedgerLink.fmp12 | 2 | 1 | 2 | 57 | 2 | 31 | 2 | 9 | 1 | 68 | 3 | 89 | 300 | 113 | 38 | 27 | 745 | 74,651 |
| karbon_charts.fmp12 | 2 | 1 | 2 | 54 | 2 | 33 | 13 | 9 | 1 | 22 | 3 | 18 | 37 | 29 | 4 | 2 | 232 | 9,504 |
| karbon_data.fmp12 | 2 | 1 | 2 | 45 | 1 | 24 | 1 | 9 | 1 | 68 | 3 | 63 | 8 | 77 | 65 | 2 | 372 | 22,417 |
| karbon_dev.fmp12 | 2 | 1 | 1 | 0 | 1 | 24 | 0 | 9 | 1 | 2 | 3 | 0 | 8 | 1 | 1 | 0 | 54 | 769 |
| karbon_konnector.fmp12 | 2 | 1 | 1 | 0 | 1 | 24 | 0 | 9 | 1 | 2 | 3 | 0 | 8 | 1 | 1 | 0 | 54 | 769 |
| karbon_kontacts.fmp12 | 2 | 1 | 2 | 55 | 2 | 32 | 13 | 9 | 1 | 77 | 3 | 222 | 347 | 238 | 23 | 48 | 1,075 | 76,174 |
| karbon_kontroller.fmp12 | 2 | 1 | 2 | 61 | 1 | 24 | 8 | 9 | 1 | 21 | 3 | 136 | 196 | 150 | 2 | 0 | 617 | 102,715 |
| karbon_log.fmp12 | 2 | 1 | 2 | 51 | 1 | 24 | 4 | 9 | 1 | 3 | 3 | 2 | 17 | 3 | 3 | 0 | 126 | 4,834 |
| karbon_tester.fmp12 | 3 | 1 | 2 | 58 | 1 | 23 | 7 | 9 | 1 | 23 | 4 | 37 | 89 | 42 | 7 | 1 | 308 | 23,746 |
| **Total** | 27 | 15 | 23 | 594 | 18 | 315 | 75 | 119 | 13 | 382 | 40 | 683 | 1,369 | 784 | 195 | 116 | **4,768** | **399,692** |

Inference: `ZeitGeist`, `karbon_dev` and `karbon_konnector` have the same shape (1 table, 2 layouts, 8 scripts, no custom functions or data sources). They look like shell files of a single template. The file explorer confirms this.

### Files without a DDR

| File | Status | Evidence |
|---|---|---|
| Karbon.fmp12 | fact (operator): a launcher file. It has no DDR, so no objects. unresolved: which file it opens and which scripts it calls. | Operator statement; no data source in the 13 DDR files points to it. |
| Party_Merge_Utility.fmp12 | fact: `karbon_kontacts` names it as FileReference id 14 (`pathList=file:Party_Merge_Utility`), with no DDR link. It has no DDR, so its objects are unavailable. The graph and script explorers trace what `karbon_kontacts` uses from it. | `objects/karbon_kontacts.fmp12/data-sources/14-Party_Merge_Utility.txt` |

## Files and references

Fact, from each file's `data-sources` objects. "References" lists data sources that resolve to a file in this DDR set (the alias, when the data source has a different name). "Unresolved" lists data sources with no DDR in the set, with their path.

| File | References these files (data sources) | Unresolved data sources | Evidence |
|---|---|---|---|
| Picker Module.fmp12 | karbon_data (twice: "Data" and "karbon_data", both with a `$DataFileRef` path first), karbon_kontroller, karbon_kontacts | none | `objects/Picker_Module.fmp12/data-sources/` |
| ZeitGeist.fmp12 | none | none | `objects/ZeitGeist.fmp12/data-sources/` (empty) |
| karbon_Build.fmp12 | none | 21, each with a variable path set at run time: "Otto Migrator" (`$$OttoMigratorDataSource`) and File_01 to File_20 (`$$File_01` to `$$File_20`) | `objects/karbon_Build.fmp12/data-sources/` |
| karbon_Generator.fmp12 | karbon_kontroller (as "Karbon_Kontroller") | karbon_Generator_Kontroller (`file:karbon_Generator_Kontroller`) | `objects/karbon_Generator.fmp12/data-sources/` |
| karbon_LedgerLink.fmp12 | karbon_kontroller (as "DataSourceControllerFile"), karbon_data (as "DataSourceDataFile"); both paths add a `$$` variable | none | `objects/karbon_LedgerLink.fmp12/data-sources/` |
| karbon_charts.fmp12 | karbon_data, karbon_konnector (twice: "karbon_konnector" and "Karbon_Konnecter"), karbon_log, karbon_tester, karbon_Build, ZeitGeist, karbon_kontroller, karbon_charts (itself), karbon_LedgerLink, Picker Module, karbon_dev, karbon_Generator | none | `objects/karbon_charts.fmp12/data-sources/` |
| karbon_data.fmp12 | karbon_LedgerLink | none | `objects/karbon_data.fmp12/data-sources/` |
| karbon_dev.fmp12 | none | none | `objects/karbon_dev.fmp12/data-sources/` (empty) |
| karbon_konnector.fmp12 | none | none | `objects/karbon_konnector.fmp12/data-sources/` (empty) |
| karbon_kontacts.fmp12 | karbon_kontroller, karbon_log, karbon_Build, karbon_data, karbon_charts, Picker Module, karbon_dev, karbon_Generator, karbon_LedgerLink, ZeitGeist, karbon_konnector, karbon_tester | Party_Merge_Utility (`file:Party_Merge_Utility`, id 14) | `objects/karbon_kontacts.fmp12/data-sources/` |
| karbon_kontroller.fmp12 | karbon_data, ZeitGeist, karbon_konnector (as "Karbon_Konnecter"), karbon_kontroller (itself), karbon_LedgerLink, karbon_log, karbon_charts | karbon_reports (`file:karbon_reports`) | `objects/karbon_kontroller.fmp12/data-sources/` |
| karbon_log.fmp12 | karbon_data, ZeitGeist, karbon_konnector (as "Karbon_Konnecter"), karbon_kontroller | none | `objects/karbon_log.fmp12/data-sources/` |
| karbon_tester.fmp12 | karbon_konnector (as "Karbon_Konnecter"), ZeitGeist, karbon_kontroller, karbon_data, karbon_kontacts, Picker Module | karbon_data_loader (`file:karbon_data_loader`) | `objects/karbon_tester.fmp12/data-sources/` |

Fact: 75 data sources. 48 resolve to a file in the set, 27 do not (21 variable paths in `karbon_Build`, plus `Party_Merge_Utility`, `karbon_Generator_Kontroller`, `karbon_reports`, `karbon_data_loader`). No file names `Karbon.fmp12`. Inference: `karbon_kontacts` and `karbon_charts` reference almost every other file, so they are likely the interface hubs, and `karbon_data` is the main data file (65 of the 195 base tables). The graph explorer confirms or corrects this.

Fact: 89 rows of `objects/unresolved.tsv` are `ref=external`: karbon_Generator 40, karbon_LedgerLink 33, karbon_kontroller 12, karbon_Build 2, karbon_tester 2.

## Explorer coverage

Step 3 is done. `catalog_contract.py verify` passes for all 9 explorers: every one of the 4,768 objects has exactly one CSV row (updated 2026-10-27). CSVs: `explorers/<name>/csv/`.

| Explorer | Kinds | Objects | Object lines | Batches | `catalog_contract.py verify` |
|---|---|---:|---:|---:|---|
| Scripts | scripts | 1,369 | 249,741 | 294 | pass |
| Custom functions | custom-functions | 594 | 26,133 | 20 | pass |
| Tables | tables | 195 | 27,297 | 15 | pass |
| Layouts | layouts | 382 | 51,419 | 54 | pass |
| Graph | table-occurrences, relationships | 1,467 | 34,526 | 42 | pass |
| Security | privilege-sets, accounts, extended-privileges | 186 | 1,244 | 39 | pass |
| Value lists | value-lists | 116 | 1,978 | 8 | pass |
| Menus | custom-menus, custom-menu-sets | 333 | 6,479 | 26 | pass |
| File | file, data-sources, auth-files, base-directories (no other kinds exist) | 126 | 875 | 61 | pass |
| **Total** | 16 kinds | **4,768** | **399,692** | **559** | **pass (9 of 9)** |

Every kind in `objects/index.tsv` appears in exactly one row. Catalogs: `explorers/<name>/catalog.json`. Reading caveats from Step 3 (graph and two layout shares read through parsed or condensed views; a possible `callers.json` overwrite between script panes) are listed in `01_discovery_answers.md` for the Step 6 audit.

## Data model

Fact (mechanical count): 195 base tables, 3,071 fields: 2,087 regular, 386 calculated, 50 summary, 567 globals, 18 containers. 328 fields have an active validation rule and 895 have auto-enter. The full per-table list is in [00_app_summary_appendix.md](00_app_summary_appendix.md). Role and key status come from the table explorer, so they are `unresolved` here.

| File | Base tables | Fields | Regular | Calculated | Summary | Globals | Containers | Validation | Auto-enter | Key status | Evidence |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| Picker Module.fmp12 | 6 | 81 | 6 | 44 | 0 | 37 | 0 | 0 | 3 | unresolved | `objects/Picker_Module.fmp12/tables/` |
| ZeitGeist.fmp12 | 1 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | unresolved | `objects/ZeitGeist.fmp12/tables/` |
| karbon_Build.fmp12 | 12 | 130 | 84 | 15 | 2 | 29 | 0 | 25 | 37 | unresolved | `objects/karbon_Build.fmp12/tables/` |
| karbon_Generator.fmp12 | 32 | 367 | 236 | 65 | 0 | 67 | 0 | 5 | 76 | unresolved | `objects/karbon_Generator.fmp12/tables/` |
| karbon_LedgerLink.fmp12 | 38 | 788 | 670 | 59 | 17 | 44 | 2 | 205 | 248 | unresolved | `objects/karbon_LedgerLink.fmp12/tables/` |
| karbon_charts.fmp12 | 4 | 23 | 20 | 0 | 0 | 3 | 0 | 10 | 12 | unresolved | `objects/karbon_charts.fmp12/tables/` |
| karbon_data.fmp12 | 65 | 1,167 | 925 | 145 | 31 | 66 | 5 | 73 | 456 | unresolved | `objects/karbon_data.fmp12/tables/` |
| karbon_dev.fmp12 | 1 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | unresolved | `objects/karbon_dev.fmp12/tables/` |
| karbon_konnector.fmp12 | 1 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | unresolved | `objects/karbon_konnector.fmp12/tables/` |
| karbon_kontacts.fmp12 | 23 | 355 | 28 | 37 | 0 | 300 | 9 | 10 | 44 | unresolved | `objects/karbon_kontacts.fmp12/tables/` |
| karbon_kontroller.fmp12 | 2 | 97 | 86 | 4 | 0 | 7 | 2 | 0 | 7 | unresolved | `objects/karbon_kontroller.fmp12/tables/` |
| karbon_log.fmp12 | 3 | 16 | 12 | 1 | 0 | 3 | 0 | 0 | 10 | unresolved | `objects/karbon_log.fmp12/tables/` |
| karbon_tester.fmp12 | 7 | 44 | 17 | 16 | 0 | 11 | 0 | 0 | 2 | unresolved | `objects/karbon_tester.fmp12/tables/` |

Counting rules: Regular = a Normal field that is not global. Globals counts every field with `global=True`, of any type, so the columns do not add up to Fields. Containers = `dataType=Binary`. Validation and Auto-enter count active options only (see the appendix).

Fact: the `# records:` headers show that the DDR files hold almost no data. The largest counts are utility tables (`VirtualListRows` with 1,000 rows in `karbon_kontacts` and `karbon_Build`, `Rows` with 529 in `karbon_tester`) and reference data (`Country` 179, `QBOEntity` 19, `MergeField` 17, `ActivityPartyRole` 16). Inference: the files are a framework or template build, not a production data set. Data volume is a discovery question.

Inference: `karbon_kontacts` has 300 globals in 355 fields. That points to a UI-state / interface file, not a data file.

### Relationship semantics

unresolved: the graph explorer fills this (683 relationships). Do not call an equality relationship a foreign key until the referenced key is shown to be unique.

### Globals, containers, time, and validation

- Globals by purpose: fact: 567 global fields (300 in `karbon_kontacts`). Purpose: unresolved (table explorer).
- Container fields and where they store data: fact: 18 container fields (karbon_kontacts 9, karbon_data 5, karbon_LedgerLink 2, karbon_kontroller 2). Base directories: 23 objects. Each file has a `Thumbnails` option (12 Temporary, 1 Permanent), and there are 7 named base directories: `Files/Picker Module/`, `HTTPRequest/`, `Karbon/`, `WebIntegrator/`, `fmqboMO/`, `karbon_data/`, `karbon_kontroller/` (4 times). Evidence: `objects/*/base-directories/`.
- Source timezone for Date/Time/TimeStamp values: unavailable from the DDR (159 TimeStamp, 57 Date, 14 Time fields). Discovery question.
- Fields with validation or auto-enter: fact (mechanical): 328 validation and 895 auto-enter. The table explorer gives the exact rules.

## Functional and logic map

unresolved: the script, function, table and layout explorers fill this. Signals in the object names, as inference only:

| Domain | Objects (scripts, fields, functions, layouts) | Reads/writes | Specialized rules | Open questions |
|---|---|---|---|---|
| Contacts / parties (CRM core) | `karbon_kontacts` (347 scripts, 77 layouts), `karbon_data` (65 tables) | unresolved | unresolved | Which files are interface and which are data |
| Accounting integration (QuickBooks Online) | `karbon_LedgerLink` (300 scripts, 38 tables, e.g. `QBOEntity`); scripts `Sync_QBO_With_FM`, `QBO.OpenInvoices` | unresolved | unresolved | Is the QBO integration in scope (`EXT-` id) |
| Transactions / controller | `karbon_kontroller` (196 scripts, 102,715 lines, the largest file; table `DBTransactions`) | unresolved | unresolved | |
| Code generation / build / migration tools | `karbon_Generator` (240 scripts), `karbon_Build` (Otto Migrator data sources) | unresolved | unresolved | Developer tooling, or part of the product? |
| Testing / logging / charts / pickers | `karbon_tester`, `karbon_log`, `karbon_charts`, `Picker Module` | unresolved | unresolved | |

`ExecuteSQL()` in calculations: fact: 48 object files with an `fn:ExecuteSQL` ref (mostly the `SQL.*` custom functions and the `Return_Table_List` / `Return_Field_List` scripts). `Execute SQL` ODBC script steps: fact: 0. No script is excluded by its name before its effects are traced.

## Security model

| Dimension | Count | Evidence | Notes |
|---|---:|---|---|
| Accounts and provisioning | 27 | `objects/<file>/accounts/` | fact: every file has `Admin` ([Full Access], Active) and `[Guest]` ([Read-Only Access], Inactive). `karbon_tester` also has `Bots` (privilege set `Bot`, Active). All are `managedBy=FileMaker`, with no external IdP. The DDR has no passwords or hashes. |
| Privilege sets | 40 | `objects/<file>/privilege-sets/` | fact: the 3 standard sets in each file, plus `Bot` in `karbon_tester`. |
| Record-level rules | unresolved | security explorer | |
| Field restrictions | unresolved | security explorer | |
| Layout / script / value-list access | unresolved | security explorer | |
| Extended privileges (access channels) | 119 | `objects/<file>/extended-privileges/` | fact: 9 per file, 10 in Picker Module and karbon_Generator. Which sets have them: unresolved. |
| Hide and conditional-format rules | unresolved | layout explorer | UI evidence only |
| Outbound file-access authorization | 2 | `objects/karbon_Build.fmp12/auth-files/1-OutboundAuthorization.txt`, `objects/karbon_Generator.fmp12/auth-files/1-OutboundAuthorization.txt` | fact: `karbon_Build` authorizes access to "Otto Migrator", and `karbon_Generator` to "HubSpot". All 13 files have `requireAuthorization=False`. |

Access level is not inferred from privilege-set names alone.

## Open decisions

| Decision | Evidence | Impact | Owner |
|---|---|---|---|
| OD-00-01: What does `Karbon.fmp12` (launcher, no DDR) do at open, and must the rebuild reproduce it? | Operator statement | Startup flow, UI entry point | Operator |
| OD-00-02: Get a DDR for `Party_Merge_Utility.fmp12`, or treat it as out of scope? | `objects/karbon_kontacts.fmp12/data-sources/14-Party_Merge_Utility.txt` | Party merge behavior in `karbon_kontacts` | Operator |
| OD-00-03: Are `karbon_reports`, `karbon_Generator_Kontroller` and `karbon_data_loader` live files, or old references? | `objects/karbon_kontroller.fmp12/data-sources/8-karbon_reports.txt`, `objects/karbon_Generator.fmp12/data-sources/2-karbon_Generator_Kontroller.txt`, `objects/karbon_tester.fmp12/data-sources/6-karbon_data_loader.txt` | 89 `ref=external` rows | Operator |
| OD-00-04: Are the developer-tooling files (`karbon_Generator`, `karbon_Build`, `karbon_tester`, `karbon_dev`) in the migration scope? | Data sources and names above | Scope of Steps 3 and 5 (about 120,000 object lines) | Product decision |
| OD-00-05: Is the QuickBooks Online integration (`karbon_LedgerLink`) in scope? | `objects/karbon_LedgerLink.fmp12/` | Integrations, `EXT-` ids | Product decision |
| OD-00-06: Production data volume and the source timezone | `# records:` headers show near-empty files | Data migration, schema | Operator |
