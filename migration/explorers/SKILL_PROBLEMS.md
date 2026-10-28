# migrate-filemaker: skill problems from the KarbonFM run (run 2)

A deduplicated list for a later skill fix. Sources: the 22 Step 3 explorer reports and the Steps 1-2 and assign reports (`9983bc15`, `7a43f8c7`) in `/Users/jose/skiff-agent/projects/migrate-karbonfm/results/2026-10-27-a27c55ba-run-the-migrate-filemaker-skill-end-to/`, plus the Step 4 run. "Seen in" gives report ids (the first 8 hex digits of the file name). Nothing here was fixed; the skill was not edited.

## A. Dispatch and the explorer contract

| # | Problem | Seen in | Suggested fix |
|---|---|---|---|
| A1 | The Step 3 prompt had a blank batch list and a blank `--assignment` path in 19 of 22 panes; each pane guessed `assignments/<group>/agent-NN.tsv` from its share name. With a blank `--assignment`, `verify` falls back to the whole catalog. (The orchestrator's send loop caused the blanks, but the contract does not document the mapping either.) | almost all | Document `explorers/assignments/<group>/agent-NN.tsv` in `explorer-contract.md`, and make `verify` refuse an empty `--assignment` value. |
| A2 | A mixed assignment (the "small" group: security, value-lists, menus, file) cannot be checked in one `verify` call (it takes one `--catalog`/`--csv-root` pair), and the agent boundary is not visible from the file name. | ba25b88f | Let `verify` take several catalog/csv-root pairs, or write the explorer names into the TSV name. |
| A3 | Panes shared `/private/tmp/claude-501/s` for helper files; one pane overwrote another's `callers.json`, so some `callers` cells may be wrong. | 0a8d719b, 0df7d174, 513c1d6b, plan | Name a per-agent work folder in the contract (for example `$OUT_DIR/explorers/_work/<agent>/`, gitignored). |
| A4 | `output_file` starts with the kind (`scripts/...`) while `--csv-root` already ends in `<explorer>/csv`, so CSVs land at `csv/<kind>/...`. Easy to get wrong on a first read. | 999e6c8d, a1055be1 | Say it with an example path, or drop the kind prefix. |
| A5 | `verify` prints only "N objects exactly once". It does not report how many CSVs and rows it checked, and it does not check enum values or empty required cells. | 0df7d174, 95ffd6b3, aa19753d | Print CSV and row counts; add `--check-enums` using each workflow's enum list. |
| A6 | No rule for an empty `open_question`: panes wrote `none` (399 rows) or left it empty (2,167 rows). | 48c10913, d3c99e56 | Fix one value (`none`). |
| A7 | Cell path format is not fixed: `objects/` prefix or the `source_id` form, plus step and marker suffixes for callers. | 48c10913, e6761420, aabf7812 | Fix one format: `source_id` form, plus `#step N` or `key N`. |

## B. Object files and explode_ddr.py

| # | Problem | Seen in | Suggested fix |
|---|---|---|---|
| B1 | Boilerplate on almost every script step (`DisableStepCollapsed state=False`, `Restore state=False`, `Repetition / Calculation: \| 1`, empty `#` steps) makes the files 2-3 times longer than their content. Every script pane wrote its own filter. | all script panes | Omit default-valued step options, as is already done for field options. |
| B2 | `Insert Text` steps print whole base64 payloads and sample JSON bodies. | aabf7812 | Truncate long literal payloads, with a length marker. |
| B3 | A `Field table=... name=...` reference does not show whether the field is global, so the write scope needs a lookup in the table file. | 62b8edf6, 6cb54354, aabf7812, cf9868f0 | Add `global` to field references, or to `refs:` entries. |
| B4 | "Picker Module" is spelled three ways: catalog `file` = `Picker Module.fmp12`, folder and `in=` = `Picker_Module.fmp12`, data source `file:Picker Module`. Helpers keyed by one spelling miss callers. | 0df7d174, 16d4d063, 62b8edf6, cf9868f0, b10bb0b0 | Add a `folder` column to index.tsv and the catalog, and use one form in `in=`. |
| B5 | `Script id=N name=...` is quoted only when the name has a space; a quote-based grep drops unquoted names (`name=_Initialize`, `name=onOpen`). | 0df7d174, 6979665e, aabf7812 | Always quote names. |
| B6 | Some custom-function bodies are stored XML-entity-escaped (`&quot;`, `&#13;`); LEGEND does not say so. | 95ffd6b3, a1d576b5 | Unescape on output, or note it in LEGEND. |
| B7 | Undocumented codes: `HideCondition findMode=`, `AuthFileCatalog requireAuthorization`, `OutboundAuthorization`, `Thumbnails type`, `Encryption type=0`, and most layout flag codes (ButtonObj, Object, FieldObj, PortalObj, NumFormat, TextObj flags are all `unknown`). | d3c99e56, e45f07cf, ba25b88f, 9983bc15 | Decode them from FileMaker documentation or test files, or say plainly they are not interpretable. |
| B8 | "base table: missing" (data source present but table id absent) and "external" (variable data-source path) get no rule for how to classify them. | e6761420 | Add a LEGEND line and an explorer rule (`unresolved` plus reason). |
| B9 | A Find step with `Restore state=False` still prints its saved criteria; reading them as criteria gives wrong `reads`. LEGEND notes this, but the workflow does not warn. | cf9868f0 | Add a line to fm-script-explorer, or print those criteria under an `(unused)` header. |

## C. Caller and usage search

| # | Problem | Seen in | Suggested fix |
|---|---|---|---|
| C1 | The documented grep (`Script id=<id> name=` plus `in=<file>`) misses cross-file calls through a data source whose path is a variable: they print `ref=external` with no `in=` (LedgerLink to kontroller through `DataSourceControllerFile`). It also gives false hits for the same id in another file, and for `ref=missing` lines. | 16d4d063, 513c1d6b, 62b8edf6, 6979665e, aa19753d, aabf7812, 999e6c8d | Ship a `callers.py` helper that resolves `in=`, `ref=external` (by data source and name), disabled steps and file options. Every pane wrote its own. |
| C2 | Name-based search gives false hits for generic or duplicated names ("Test", "README", "Record Nav"), and a layout can call a script by name with a different id than the same-named script in its file. | 16d4d063, 6cb54354, cf9868f0 | Same helper: match name and id per file. |
| C3 | Calls made inside `Evaluate` text, `Perform Script by name`, or through `<Function Missing>` plugins cannot be traced. | 95ffd6b3, a1d576b5 | Have the explorer write `unresolved (dynamic call)` in callers instead of `none found`. |
| C4 | Custom functions are file-local, but the documented global grep hits same-named functions in other files. | a1d576b5, 95ffd6b3 | Scope the function caller search to the same file. |
| C5 | No format for callers that are file options, layout triggers or buttons (the trigger object key is lost), and no guidance for 100+ callers (Start Transaction). | 16d4d063, aabf7812, 6979665e | Caller format `<source_id> key N (trigger OnX)`; cap the list at N plus a count. |

## D. Vocabulary and format gaps per explorer

| # | Explorer | Problem | Seen in |
|---|---|---|---|
| D1 | scripts | No `type`/`owner` value for developer tooling, generators or tests (panes used `utility` + `drop-candidate`). `drop-candidate` and "effect owner" overlap for tooling with real effects (Commander). | 513c1d6b, 62b8edf6, 6979665e |
| D2 | scripts | No write-scope term for the `DBTransactions` + `<TO>::ONE = 1` lock-or-create pattern, and no scope for a script whose record context is set by its caller. | 62b8edf6, 48c10913 |
| D3 | scripts | No `reads`/`writes`/`external` format for Truncate Table, Set Next Serial Value, account steps, Commit Records, clipboard, Close File, Save a Copy, Install OnTimer Script, Perform Find omit requests, or `ref=external` field references. | 6cb54354, 999e6c8d, 48c10913, aa19753d |
| D4 | scripts | `calls` format for cross-file targets (data-source name or file name) is not fixed. | 0a8d719b, 6cb54354, aabf7812, aa19753d |
| D5 | scripts | `empty` means "no steps", but a comment-only README has steps. The tier rule is ambiguous for orchestration of an external API. | 62b8edf6, aabf7812 |
| D6 | scripts | No guidance for `Evaluate` on a parameter, loops starting at 0, SQL custom functions that hide field ids from `refs:`, or calls to missing scripts. | 513c1d6b |
| D7 | tables | No tag for global calculated fields (no `storeCalculationResults`). | 7aaa114d |
| D8 | tables | "StrictValidation alone" is tagged `validate-rule`, which flags about 52 audit fields whose validation calculation is switched off. | 7aaa114d, a1055be1 |
| D9 | tables | No rule when both `constant=True` and `calculation=True` are active, for `auto-modified` with `calculation=False`, or for a bare rule under a default `OnlyDuringDataEntry` header. | 7aaa114d |
| D10 | tables / graph | The DDR does not mark the parent side of an Equal relationship; FK direction comes only from key evidence. | a1055be1 |
| D11 | graph | The predicate enum says `GreaterThanOrEqual`/`LessThanOrEqual`; the DDR prints `...OrEqualTo`. | b10bb0b0, e6761420 |
| D12 | graph | No notation for a deleted or external field in a predicate (`TO::<ref=missing>#N` was improvised), and no `data_source` value for `name="" ref=missing`. | b10bb0b0 |
| D13 | graph | `use` has no value for a table occurrence that only serves relationship chains (it shows as `unused`), and script-context vs calculation is not defined. | b10bb0b0, e6761420 |
| D14 | layouts | No label rule when a button has no text, LabelCalc, tooltip or name, or when the tooltip is `Self`; constant `""` hide markers; comment-only hide calculations; empty layouts; target for no-reference layouts; context for a `ref=missing` or `ref=none` table occurrence; and which file to name when the base table is in another file. | b7d5fcf7, d3c99e56, e45f07cf |
| D15 | layouts | A `GroupButtonObj` can wrap another or carry its own `Step`; style names may be quoted; the census counts buttons inside portals and popovers. | d3c99e56, e45f07cf |
| D16 | menus | `removed` cannot be known for trimmed menus (the DDR keeps only retained items); no `items`/`removed` rule for the 286 bare standard-menu references; no columns for mode, platform or title. | ba25b88f |
| D17 | security | No column for `allowCreation` on layouts, value lists and scripts; the `records` enum has no `CreateEditDelete`. | ba25b88f |
| D18 | file | No rule to split a `pathList` whose file name has a space, or that holds `$`/`$$` variables. | ba25b88f |
| D19 | functions | The "twin in the other file" wording assumes one copy; this solution has up to 9. No format for self-recursion or calls through `Evaluate`. | a1d576b5 |

## E. Steps 1-2, assign, and Step 4

| # | Problem | Seen in | Suggested fix |
|---|---|---|---|
| E1 | Batching by file + kind + folder makes tiny batches (the file explorer has 61 batches for 875 lines), so the small explorers write very many CSVs. | 9983bc15, 7a43f8c7 | Merge small folders into one batch up to the batch limit. |
| E2 | The `00_app_summary` count command `cut -f1,2 index.tsv \| sort \| uniq -c` counts the header row. | 9983bc15 | Use `tail -n +2`. |
| E3 | The `00_app_summary` data-model table has no Fields, Validation, Auto-enter or Records columns, and no counting rule (global calcs, inactive options). | 9983bc15 | Add the columns and the rules. |
| E4 | The skill exists in two places (`~/.claude/skills` and `~/.agents/skills`); the Skill tool loaded one and the prompt named the other. | 9983bc15 | Keep one copy, or link one to the other. |
| E5 | Step 4 does not define the signal set exactly: whether value-list `used_by = none` counts as "none found", whether graph `needs-profile` and tables/security `unresolved` targets count, and which columns count as "target". | Step 4 (this run) | List the exact column/value pairs per explorer. |
| E6 | There is no helper to collect signals, and no schema for `01_discovery_signals.tsv`. Grouping 4,452 signals by hand is not possible, so the grouping is keyword-based and approximate. | Step 4 | Ship `collect_signals.py` with a fixed TSV schema (`signal_id, question_id, type, explorer, kind, file, id, name, source, column, value`). |
| E7 | The `01_discovery_answers` "Questions from the objects" table has no Default, Owner or Waits columns, although Step 4 requires them for "I don't know" answers. They had to go into the Answer cell. | Step 4 | Add the three columns. |
| E8 | The template has no place for stakeholder-only questions in the ranked list (goals, users, hosting); they sit in later sections, unranked. | Step 4 | One ranked list for all questions, with the sections referencing ids. |
