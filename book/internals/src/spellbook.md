# Spellbook

`crates/spellbook/` is the package `sorcerer-spellbook`: the library `spellbook` and the program
`spellbook`. It comes from olifant, whose `olifant gen` wrote a convention's files from a
generator directory; spellbook keeps olifant's `generator.toml` keys and its error sentences,
and replaces olifant's tab stops with **holes**: placeholders that parse and say what belongs
there, so a person, an agent or conjure can fill them.

Spellbook depends on `sorcerer-core` and `sorcerer-lsp` only. It makes no model call.

## The modules

`domain/` (data and rules, no I/O):

- `config.rs`: `SpellbookConfig { enabled, exclude, sync_on_save }` (`sync_on_save` is read by the server's sync on save, `adapters/lsp/sync_on_save.rs`), `spellbook_config`, `PRODUCT`, `SPELLBOOK_IS_OFF`; `enabled` is read by `cli/run_cli.rs::run_command` and
  the server (`republish`, `wired_for_commands`).
- `generator.rs`: the types (`Generator`, `Input`/`InputKind`, `Query`/`QueryValue`,
  `Edit::{Create, Insert}`/`EditBody::{Template, Text, Copy}`/`OnExists`/`OnMissing`, `HoleSource`, `Use`) and `read_generator(id, text, templates)`, which
  reads `generator.toml` key by key with errors naming the place (`[inputs.app] type: …`).
  `edit_body` takes exactly one of `template`, `text` or `copy` (`copied_file`: a path under
  `FILES_DIR`, kept as its path there; `copy` is create-only). `Generator.files` (the bytes of
  `files/`, which `read_generator` leaves empty and `generator_dir.rs` fills) and
  `Generator.notes_after` (`[notes] after`, read by `notes_after`) carry pack-shaped data. The
  workspace's `toml` sorts a table's keys, so the order of `[inputs.X]` headers and `[derive]`
  lines (which is the order inputs are asked and names derived) is read from the text with a
  regular expression (`section_order`, `derive_order`).
- `generator_problems.rs`: `GeneratorProblem { generator, file, why }` and the sentences:
  `unknown_key_text` (`content` → `text`, olifant's later keys → "arrives in milestone 2", else
  the nearest key by edit distance), `unknown_value_text`, `function_style_text`
  (`pascalCase(x)` → `{= x|pascal =}`).
- `generator_checks.rs`: `check_generator(generator, languages)`, the checks across the file:
  the language is the project's; an enum's `one_of` names a query whose glob uses only earlier
  inputs (an enum with `choices` is `InputKind::Fixed`: its list is its choices, `choices_for` returns it
  without resolving earlier inputs, and `one_of` and `choices` together are refused by `read_generator`); every edit's template exists in `templates/` and every `copy` in `files/` (`edit_templates_exist`); paths, derives, derived holes and notes use known names (the inputs, `TIMESTAMP`, the derived names: `known_names`) and no
  holes; every template parses as a template (core's `pieces`) and uses known names, and its text
  holds no `{{ name }}` around one of them (`refuse_braced_name`: other `{{ … }}` is an askama or
  Jinja page's own text); a hole
  has one kind and one plain instruction; a placeholder hole's kind is in the generator
  language's `[holes]`; an insert's phrases with no `{= =}` parse (`Anchor::parse`), its `unless`
  compiles once filled with stand-in values (`inserts_are_well_formed`).
- `fix.rs`: `Fix { generator, label, with, replace }`, `Replace { enclosing, text }`,
  `FixBuiltins` (the slots `files` bound, `dir`, `stem`, `file`, `finding`), `fix_inputs`,
  `fix_label`, `check_fix` (with `FixGenerator::{Absent, Broken, Takes}`: the load-time check of
  a fix against the project's generators), `finding_for_instruction` (one line, no quotes or
  backslashes, 60 characters) and `with_instructions_filled` (the values inside a `replace`
  hole's instruction filled before rendering, since `pieces` cannot nest `{= =}`).
- `anchor.rs`: `Anchor::parse` (olifant's `<position> <target>` phrases: `Position`, `Target`),
  `Resolution::{At(Place), Missing, Several, Refused}`, `Place::{Lines, InLine}`,
  `resolve_in_text` (the targets that need no grammar: `marker`, `line matching`, `file`),
  `resolution_text` and `anchor_text` for the sentences.
- `change.rs`: `TextChange` (whole lines, rows from 0), `apply_changes` (bottom up, refusing
  overlaps), `change_at` (a rendered text placed at a `Place`, indented, in the file's line
  ending), `changes_between` and `hunks` (both over `similar`), `added_and_removed`,
  `with_line_ending`.
- `holes_table.rs`: `holes_table(language)` reads `[holes]` strictly from `Language.rest`;
  `placeholder(language, kind, name, instruction)` fills a kind's template with core's `fill`.
- `inputs.rs`: `Given` (what was typed; `with_timestamp(now)` adds the built-in `TIMESTAMP`
  as `YYYYMMDDHHMMSS` unless given), `Values` (inputs, then the built-in timestamp
  (`with_built_ins`, not counted among the recorded inputs, left out when the generator declares
  its own `timestamp` input), then derived names, in order, as `fill` takes them; `with_derived`),
  `InputStop` and `input_stop_text`, `is_a_name`, and
  `query_choices(files, glob, exclude, value)`: the glob compiled with `literal_separator` (so `*` is one
  folder), matched against every file **and every folder above it**, read as the parent
  folder's name, the stem or the path; a match that any `exclude` glob (a `[query.X]` `exclude`, filled like the glob in `choices_for`) also matches is dropped.
- `template.rs`: `render(template, values, derived, placeholder_of)` → `Rendered { text, holes }`;
  each hole's line is counted where its text lands, so a block placeholder keeps the
  indentation the template gave the hole.
- `holes.rs`: `Hole { name, kind, instruction, source: HoleOrigin, places }`,
  `HolePlace { file, line, text }`, and `HoleBook`, which gathers one generation's holes file by
  file (one hole per name, every place).
- `plan.rs`: `Plan { id, generator, inputs, files, holes, fix, notes }` (`notes`: the generators'
  `[notes] after`, then region hints), `FixOrigin`, `PlannedFile`
  (`new_file`, `copied(path, bytes)` (UTF-8 bytes become text; others `binary: Some(BinaryBody)`,
  which serializes as its length, with empty `text`), `bytes()` (what `apply_plan` writes), `left`, `changed(path, before, after)`, which works out `changes`, `hunks`,
  `added`, `removed`; `before` is what the apply checks the disk against; `region` and `set_up` when an
  owned region rewrote it), `FileStatus::{New,
  Skipped, Changed, Unchanged}`, `PlanStop` (with `FindingGone`, `NoReplaceCase`, `Anchor`,
  `NothingToDo(Applied)`, `RegionBroken`, `RegionWouldLoseLine`, `RegionsDoNotLoad`, `RegionNotThere`, `Region`) and `plan_stop_text`, `relative_path` (lexical: no absolute path, no `..` out),
  `plan_id` (eight hex digits of `fnv64_hex` over the generator, the inputs and the time).
- `region.rs`: owned regions. `Region { name, file: PathPattern, exclude, query: RegionQuery { glob,
  exclude }, item, init_at, comment, severity }`, `REGIONS_FILE`, `RegionsProblem`, `DeclaredRegions`,
  `parse_regions(text)`; `RegionInstance` (`Region::instance_in(file)`, `query_glob`, `query_excludes`,
  `items`, `opening_marker`, `closing_marker`); and the text work, all on plain strings: `find_block`
  (`Block { open_row, close_row, indent }` or `BrokenMarkers { row, why }`), `splice`, `Markers`,
  `with_markers_at`, `outside_every_block`, `adopt`, `first_lost_line` (which lets an opening marker's wording change) (`LostLine`). See
  [A sync, end to end](#a-sync-end-to-end).
- `record.rs`: `GenerationRecord` (a line of `generations.jsonl`; a fix adds `fix: RecordedFix`
  and `changed: Vec<ChangedFile>`, whose `region` names the owned region that rewrote the file), `OpenHole` and `HolesFile`
  (`holes.json`, a core `Versioned` at version 1), `record_of`, `open_holes_of`.
- `open_hole_places.rs`: `still_open(hole, text)`: a placeholder carrying `hole <name>:` is
  found by that text anywhere, nearest the recorded line first; a short one (`todo_field`) as a
  whole word on its line or within `NEARBY_LINES` (20), standing between the same signs it was written
  between (`OpenHole.edges`, set by `render`), so a `()` type hole is not mistaken for the `()` of
  `Result<(), E>`.
- `path_pattern.rs`: `PathPattern::parse` and `matches(path)` (the slots bound): a rule's `files` and
  `exclude`, compiled to an anchored `regex` (`*` one name, `**` any folders, `**.ext` = `**/*.ext`,
  `{slot}` a lazy `[^/]+?` capture).
- `rule.rs`: `Rule`, `RuleBody::{Match(MatchForm), Query(String)}`, `Severity` (a rule that names none is `Error`; `warn` and `hint` are written out), `parse_rules(text)`
  (strict keys; `fix` or `[[fixes]]` read into `Rule.fixes`, both together refused; `missing` says
  "arrives in milestone 2b"; both `match` and `query` is refused; every regex is compiled once to catch a bad one early). `rule_query.rs`: `query_text`, the
  short form written as tree-sitter query text (`@finding`, `#match?`, `#not-match?`, backslash and
  quote escaped).
- `finding.rs` (`Finding`, whose `node_text` is not serialized and whose `region: Option<RegionDrift>`
  is flattened into a region's finding; `REGION_STALE`, `REGION_NOT_SET_UP`, `REGION_BROKEN` beside
  `ALLOW_WITHOUT_REASON` and `UNKNOWN_RULE`; `RawFinding` with the node's `kind`
  and `Span` (rows and bytes) and the `unit` around it, which `QueryRules` fills from core's
  `units_of`/`unit_at`; `fingerprint`: seven hex digits of `fnv64_hex` over the rule and a text with
  whitespace collapsed; a rule's finding uses `RawFinding::fingerprint(text)` =
  `rule_finding_fingerprint(rule, unit, line)`, the unit's name and the finding's whole first line,
  so identical matches in different definitions or on different lines differ while edits elsewhere
  leave it alone; allow warnings and regions fingerprint their own words), `allow.rs` (`allow_comments`, `silencing`),
  `report.rs` (`ReportFile`, `with_files_replaced`, `sort_findings`; `report.json` is a core
  `Versioned` at version 1). `Finding.existing` is set by the baseline and left out of the JSON when
  false.
- `baseline.rs`: `Baseline` (sorted `BaselineEntry { file, rule, fingerprint }`), `parse_baseline`,
  `baseline_text` (the header and `[[baseline]]` tables), `Baseline::of(findings)` (rule findings
  only, never a region's), `Baseline::marked(findings)` (each entry marks one finding `existing`, by
  count, in the findings' order), `Baseline::marked_unchanged_first(findings, unchanged)` (the same,
  but the findings `unchanged` names take the entries first: `check --since` passes "not on an added
  line") and `Baseline::change_to(newer)` (`BaselineChange { paid_off, new }`).
- `pack.rs`: `PackAddress::parse` (`<repo>[@rev]`: the last `@` with no `/`, `\` or `:` after it),
  `PackRecord` and its `text()` (`.sorcerer/pack.toml`: `name`, `repo`, `rev`), `pack_name`
  (the pack's root `pack.toml` `name`, else the repository's folder), `PACK_RECORD`,
  `SPELLBOOK_DIR`, `PackStop` and `pack_stop_text`.
- `rule_settings.rs`: `with_project_settings(rules, settings) -> ProjectRules { run, off }`: the
  `[spellbook.rules]` severities laid over the rules, `off` ones left out of `run` and listed, a
  setting for no rule refused naming it. `config.rs`'s `SpellbookConfig.rules` holds the settings
  (`RuleSetting::{Hint, Warn, Error, Off}`).

`ports/`: `generators.rs` (`Generators`: `all`, `named`), `grammars.rs` (`Grammars::for_path`:
the loaded language of a path, or none; `covers` and `comment_marks` (core's `CommentMarks`: a
comment's start and, where comments close, its end), which `CachedGrammars` answers from the
language's data without loading a grammar), `workspace.rs` (`Workspace::write_new(path, bytes)`,
`Workspace::write_changed(path, before, after)`, `WriteRefused` with `Changed`),
`generation_store.rs`, `holes_store.rs`, `clock.rs`, `rule_runner.rs` (`RuleRunner::findings_in`,
`RuleRunner::enclosing(file, text, span, kinds)`: the nearest node of those kinds around a
finding), `anchors.rs` (`Anchors::resolve(file, text, anchor)`, and
`resolve_inserting(file, text, anchor, inserting)`, which an insert's `at` uses so a target may
choose by the text; its default is `resolve`), `report_store.rs`, `pack_source.rs` (`PackSource::fetch`
→ `FetchedPack { rev, manifest, files: Vec<PackFile> }`, and `SorcererDir` (`holds`, `write`):
the project's `.sorcerer/`, which the `Workspace` port fences off).

`usecases/`: `list_generators.rs` (`GeneratorSummary`, also the editor's picker reply),
`choices_for.rs` (`resolve_inputs`, `choices_for`), `plan_generation.rs` (`PlanDeps`, which
carries `anchors` and `regions`; `plan_generation` = `plan_edits` then `refresh_regions`; `plan_edits`'s `Insert` places an insert edit and its `with_used` plans each
`[[uses]]` over a `PlannedView`), `sync_regions.rs` (`SyncMode`, `SyncPlan`, `plan_sync`,
`refresh_regions`, `region_findings`, `instances`, `not_set_up_text`; the private `Keeper` does the
work), `planned_view.rs` (`PlannedView`: a `CodebaseView` that answers
`exists`, `text` and `glob` with the plan so far laid over the project and passes every other
question on), `fixes_for.rs`
(`FixOffer`, `fixes_for`, `region_fix_offer`), `plan_fix.rs` (`FixDeps`, `FindingAt`, `plan_fix`), `apply_plan.rs`
(`ApplyDeps`, `apply_plan(deps, plan, ChangesLanded)`, `AppliedGeneration` with `changed`), `open_holes.rs` (`HolesDeps`, `open_holes`), `check_files.rs` (`CheckDeps`, which carries `regions`, `off` and `baseline`, `check_files`), `check_project.rs`
(`check_project`), `check_since.rs` (`check_since(deps, rev)`), `check_texts.rs`
(`check_texts(deps, texts)`: texts not on disk, over a `PlannedView`), `install_pack.rs`
(`InstallDeps`, `install_pack(deps, address)` → `InstalledPack { record, copied, kept }`: refuse a
project with `.sorcerer/spellbook/` before fetching, fetch, copy what the project lacks, keep what
it has, write the record).

`adapters/`: `generator_dir.rs` (`GENERATORS_DIR`, `generators_in`, `generator_named`, `files_in` (a
folder's files as bytes: `files/` as is, `templates/` through `templates_in`, which refuses one that
is not UTF-8),
`load_generator`, which follows `[[uses]]` with `uses_agree`: each used generator there and
loading, its `with` names its inputs, no cycle, at most four in a chain; `GeneratorDirs`), `cached_grammars.rs` (`CachedGrammars` over
`Languages::for_path` and `Languages::load`), `disk_workspace.rs`, `git_pack_source.rs`
(`GitPackSource`: `git clone` (`--depth 1` with no revision) into a `tempfile` folder, `checkout
--detach REV`, `rev-parse HEAD`, then the `.sorcerer/` files read with `state/` left out and a link
refused; `GIT_TERMINAL_PROMPT=0` so it never waits; only `git` is run, the same on every platform),
`disk_sorcerer_dir.rs` (`DiskSorcererDir`), `jsonl_generation_store.rs`,
`json_holes.rs` (core's `Store` at `spellbook/holes.json`), `json_report.rs` (the same at
`spellbook/report.json`), `regions_toml.rs` (`load_regions`), `baseline_toml.rs` (`BASELINE_FILE`, `load_baseline`,
`write_baseline`), `rules_toml.rs` (`load_rules`, `RULES_FILE`; `check_fixes` checks every
rule's fixes against `generator_named`), `query_rules.rs` (`QueryRules`, the `RuleRunner` over
tree-sitter), `query_anchors.rs` (`QueryAnchors<G>`, the `Anchors` port over a language's query captures,
read through core's `Captures`), `checking.rs` (`Wired::checking()`: the rules
read fresh with `[spellbook.rules]` over them, the baseline, the settings and the runner, then `Checking::deps()` and `Checking::fix_deps()`), `system_clock.rs`, `wiring.rs` (`Wired`, `live`, `wired`, and
`plan_deps`/`apply_deps`/`holes_deps`; `Wired.regions` is `regions.toml` read once).

`adapters/lsp/`: the server, below.

`cli/`: `arguments.rs`, `file_arguments.rs` (`relative_to_root`: `check`/`holes` file arguments made project-relative), `run_cli.rs` (`main`, `main_with(args, cwd, env, wire, out, err)`),
one file a cli subcommand (`init.rs`, `list.rs`, `show.rs`, `new.rs`, `fix.rs`, `sync.rs`, `holes.rs`, `check.rs`, `doctor.rs`, `trust.rs`, `lsp.rs`;
`run_before_a_project` serves `lsp` and `init`, which run before a project is opened: `init` takes
`Project::root_above(cwd)`, else the folder itself),
`format.rs` (`columns`, `count`, and the plan rows the commands share: `file_row`, which words a region's
row `(region NAME: +a −r)` or `(region NAME: set up, N items)`, `hunks_text`, `notes_text`), `console.rs`.

## One generation, end to end

1. `generator_named` loads the directory: `read_generator`, the `files/` bytes, then
   `check_generator`.
2. `plan_edits` adds the built-in timestamp from `PlanDeps.clock` (`Given::with_timestamp`;
   `--set timestamp=…` wins). `resolve_inputs` takes the inputs in the generator's order: a given value (an enum's
   checked against `choices_for`, an ident's against `is_a_name`), else an enum's only choice,
   else the default, else empty when optional, else `InputStop::Missing` with the choices. Then
   `Values::with_built_ins` adds the timestamp and `Values::with_derived` adds `[derive]` in order.
3. `plan_edits` (`own_edits`), for each edit: fill the path and keep it (`relative_path`, then
   `Fences::allows_relative`); an existing file is skipped or stops the plan; the body is a
   text, a `copy` (`PlannedFile::copied`: the bytes as they are), or a template rendered with `render` and the placeholders of **the language of the
   file's own path** (`Grammars::for_path`): a generator's `.html` or empty `__init__.py` is not
   the generator's language. A placeholder hole in a file no language covers stops the plan.
   Every new file a language covers is parsed (`parse_errors_of`), but a binary copy never is; an error line is
   `PlanStop::DoesNotParse` naming the template. Then the holes are gathered (`HoleBook`), the
   `[notes] after` filled, and each `[[uses]]` planned by `with_used` (which hands the used
   generator this one's timestamp and appends its notes once each, `PlanSoFar::with_notes`); then
   the id is made. Nothing is written. In an empty project this is the same: every shipped
   language is in `Project.languages`, so `check_generator`'s language check passes with no file
   of the language there.
4. `apply_plan` writes each new file's `PlannedFile::bytes()` through `Workspace::write_new` (`DiskWorkspace`
   refuses with `Fences::allows`, which follows links, and opens with `create_new`, so a file
   that appeared since the plan is never overwritten), stops at the first refusal, appends the
   record for what was written, and adds the placeholder places to `holes.json`. Rows kept for
   a file it has just written whole are dropped first (`remembered_outside`): that file replaces
   whatever was remembered at its path, so a file deleted by hand and generated again lists each
   hole once.
5. `open_holes` re-reads each saved place and keeps it at its current line while
   `still_open` finds it (a short placeholder away from its recorded line only on a line that still holds the pieces of the line it was written in, `HolePlace::frame`, so another method's `()` does not keep a filled hole open); `holes.json` is rewritten with what is still open. A hole whose file is
   missing is kept (a stash or branch switch may bring it back) but not listed.

## A fix, end to end

`cli/fix.rs::fix` parses `FILE:LINE`, reads the rules (`Wired::checking`) and calls
`usecases/plan_fix.rs::plan_fix(Checking::fix_deps(), FindingAt, given)`:

1. The file is read as it is now and the rules that look at it run again
   (`RuleRunner::findings_in`); the finding on the line is taken, or the plan stops with
   `FindingGone`. A gone finding whose fix's generator has nothing left to do
   (`plan_generation` says `NothingToDo`) is `NothingToDo(Applied::Fix)`: the fix was applied.
2. The fix is picked (`--fix`, from 1), its `with` filled from the finding (`FixBuiltins::of`,
   `fix_inputs`) and the person's `--set` laid over it.
3. `plan_generation` plans the generator. Its inserts are placed by `Insert::land_in`: the file as
   planned so far (or read now), `when` and `unless` checked, the body rendered with the file's
   language's placeholders, the first `at` phrase that resolves for that text through
   `Anchors::resolve_inserting`, the body placed by `change_at`, the holes counted on the file's lines, a new parse error refused.
4. `replacement` asks `RuleRunner::enclosing` for the first `replace` case's node kind around the
   finding; that node's whole lines become the rendered `text` (`finding` there is the replaced
   code; `with_instructions_filled` first), indented like the node.
5. `with_replacement` moves the replacement past the generator's changes above it in the same
   file (an overlap stops the plan) and rebuilds the file with `PlannedFile::changed`, so the
   file's `changes` are always against the text on disk.
6. The replaced file must still parse.

`apply_plan` then lands changed files first through `Workspace::write_changed` (refused, with
nothing written, when the disk no longer has `before`), writes the new files, appends the record
and the holes. The server lands changed files itself as one `WorkspaceEdit` and calls
`apply_plan` with `ChangesLanded::ByTheEditor` ([A fix in the editor](#a-fix-in-the-editor)).

### Why an `Anchors` port

The use cases never parse. A phrase like `end of parameters in CloseTable.__init__` is resolved by
`adapters/query_anchors.rs` (`Parsed`, built once per file with `Captures::of`), and only through
captures, so the adapter names no language, node kind or grammar field:

| target | `Parsed` method | captures it reads |
|---|---|---|
| `imports` | `imports`, `top_level_imports`, `row_after_the_preamble`, `row_in_order` and `last_block` (given the text to insert, `end of` goes before the first import of the last block that sorts after it, when that block is in order by its collapsed text) | `@import.path` (`imports.scm`); a top-level statement that is a `@function.outer`/`@class.outer` holds its imports inside; the preamble is the leading `@comment.outer`s, `@docstring` (`docstrings.scm`) and `@preamble` (`preamble.scm`: a `package` or file-scoped `namespace` line) |
| `function NAME`, `class NAME` | `the_one`, `definitions`, `in_definition` | `@definition.<kind>` with `@name` from every query file (Rust's `definition.impl` from `impls.scm` holds `Type::method`); the body is `Captures::body_items` |
| `parameters in F` | `parameters` | the function's own `@parameters` (`lists.scm`; not Rust's generics), items `@parameter.inner` |
| `call NAME [in F]` | `call`, `arguments_of`, `callee_text` | `@reference.call` with `@name` from every file (Rust's `calls.scm` adds `Type::new(…)` and struct literals), the call's child `@arguments` |

A list's separator is read from the list itself (the text between its last two items); a list of
one or none takes the language's `[syntax] list_separator` (`domain/language_syntax.rs`), and a
language without it refuses the place with a sentence. An empty list's place is after its first
child, the opening token. `Class.method` splits on the language's `unit_separator`. `row_after`
counts a node that holds its own line ending (Rust's `//!` comment) as ending on its line.

## A sync, end to end

An **owned region** is a list kept between two marker lines and rewritten whole from the files on
disk (the user chapter's *Owned regions and `spellbook sync`* has the keys). Everything below names no
language: the markers' comment syntax is the language's `comment` and `comment_end` (from
`language.toml`, read through `Grammars::comment_marks`; `opening_marker`/`closing_marker` write
`CommentMarks::around`, `find_block`'s `marker_of` reads `CommentMarks::inside`, so a closed
comment's markers are `<!-- spellbook:region … -->`; a language with no `comment` stops the plan
saying it cannot hold a region; a region's own `comment`, for a file no language covers, is a
line comment), the items are the person's `regions.toml`, the block is found by its text,
and the parse check and the `init_at` phrases go through the ports that exist.

1. **Load.** `Wired` reads `.sorcerer/spellbook/regions.toml` once when it is made
   (`adapters/regions_toml.rs::load_regions` → `domain/region.rs::parse_regions`) and keeps a
   `DeclaredRegions` (`Result<Vec<Region>, RegionsProblem>`): a command, or one server request, sees one
   reading. `PlanDeps.regions` borrows it; `Wired::checking()` turns a problem into the sentence `check`
   exits 2 with. `parse_regions` is strict (`unknown_key_text`), parses each `init_at` with
   `Anchor::parse`, dry-fills the query and item templates with core's `fill` over the names each may
   use, and keeps `file` inside the project and off `.sorcerer/` and `.git/` (`relative_path`).
2. **Instances.** `usecases/sync_regions.rs::instances(view, regions)`: every file of `view.glob("**")`
   (less `.sorcerer/`, `.git/`) that a region's `file` pattern matches and no `exclude` does
   (`Region::instance_in`, which binds the slots): a `RegionInstance { region, file, slots }`.
3. **The block.** `Keeper::state_of` finds the comment marker (`comment_for`: the language's, or the
   region's own `comment` for a file no language covers; both, or neither, is a `PlanStop::Region`),
   then `find_block(text, name, comment)`: the first non-blank text of each line, the comment marker,
   then `spellbook:region NAME` or `spellbook:end NAME`. No pair is `NotSetUp`; a broken pair is
   `Broken { row, why }`. `block_of` fills the query glob (`RegionInstance::query_glob`: the slots, the
   region file's `dir` and `file`), asks `view.glob`, and keeps the matches a literal-separator
   `globset` matcher accepts (core's `FilesView::glob` lets `*` cross folders; a region's `*` stays in
   one), less the query's excludes; `RegionInstance::items` renders each (`stem`, `parent`, `dir`, `path`,
   the slots), trims, drops empty lines, dedups and sorts by bytes. `splice` writes the opening marker
   (with the current glob), the items and the closing marker, each with the opening marker's
   indentation, in the file's line ending (`with_line_ending`); the same text back is `Fresh`, else
   `Stale`.
4. **Init.** Under `SyncMode::Init(name)` a `NotSetUp` instance of that region is set up
   (`Keeper::set_up`): the first `init_at` phrase the `Anchors` port resolves to whole lines
   (`Place::Lines`), moved past any block of the file's other regions it lands in
   (`outside_every_block` over `Keeper::blocks_in_file`), `with_markers_at` there, `adopt` (a line
   outside every block that is exactly an item is removed, so a hand-written list moves in instead of
   being declared twice; another region's items stay), then `splice`.
5. **The plan.** `Keeper::keep_all` goes file by file (two regions in one file are kept one after the
   other on the same text); a file whose text changed must keep every line but a block's items and the
   lines adopted (`refuse_a_lost_line` over `first_lost_line`: else `PlanStop::RegionWouldLoseLine`
   naming the line, which also catches a block nested in another), and is parsed again (`refuse_a_new_parse_error`:
   only a new error stops) and becomes `PlannedFile::changed(path, before, after)` with `region` (the
   names) and, for `--init`, `set_up` (the item count). Regions not set up are collected per region into
   `Plan.notes` (`not_set_up_text`); a broken pair is `PlanStop::RegionBroken`, which stops the whole
   plan. `plan_sync(deps, mode)` wraps this in a `Plan` whose generator is `"sync"` (inputs empty, or
   `init`), with `SyncPlan.blocks`, the number of blocks once applied (`regions up to date (5 regions)`).
6. **Apply.** `cli/sync.rs::sync` prints (`--check`: the stale and not-set-up lines and exit 1;
   `--plan`: `file_row` and `hunks_text` from `cli/format.rs`; nothing to do: `regions up to date`) or
   calls `apply_plan` unchanged: a region row is a changed file, landed through `write_changed` only
   onto the text the plan saw. The record's `changed[]` entries carry `region`.

### Every plan refreshes its regions

`plan_generation` is now two steps: `plan_edits` (the edits and the `[[uses]]`, as before; `with_used`
calls `plan_edits`, so a used generator does not refresh in the middle of the plan) and then
`refresh_regions(deps, plan)`. `refresh_regions` lays the plan's new and changed files over the
project with a `PlannedView` and runs `Keeper::keep_all` over it with no anchors (a plan never sets a
region up), so a file the plan creates is listed at once. `with_region_files` puts each rewritten file
into the plan: a new file keeps `New` with its text rewritten; a changed one is recomposed against
its disk text (`PlannedFile::changed(path, before, after)`, as `with_replacement` does), so its
`changes` stay one change from the disk; a skipped or unchanged file becomes changed (bistro's
`__init__.py`, which `command` skips, is `~ … (region commands-exports: +1 −0)`); any other is appended.
Not-set-up regions become notes; a `regions.toml` that does not load becomes the note `regions are not
being refreshed: …` and the plan goes on. `plan_fix` calls `plan_edits`, the replacement, the parse
check, then `refresh_regions` once at its end.

### Regions in `check`

`CheckDeps.regions` holds the loaded regions. `findings_of_files` adds `regions_out_of_step`, which
calls `region_findings(view, grammars, regions, Some(files))`: a `Stale` block is `region-stale` on its
opening marker's line (the region's `severity`) with `Finding.region = RegionDrift { region, added,
removed }` (flattened into the JSON); `NotSetUp` is `region-not-set-up` (warn, line 1); `Broken` is
`region-broken` (error). A region that cannot be kept (`PlanStop::Region`) is a config error naming
`regions.toml`, as a broken rules file is. `cli/check.rs::offers_for` gives a region's finding the
built-in `region_fix_offer` (`usecases/fixes_for.rs`): generator `"sync"`, inputs `region` and `file`
(and `mode = init`); `cli/fix.rs::region_finding_here` sends `spellbook fix` on such a line to
`cli/sync.rs::sync`. `cli/doctor.rs::regions_row` uses the same `instances` and `region_findings` over
a `FilesView` and `CachedGrammars` whose `covers`/`comment_marks` read the languages' data, so the
doctor loads no grammar.

## The server

`spellbook lsp` (`cli/lsp.rs`, which runs before the project is opened: the editor sends its
root at initialize) serves `adapters/lsp/` through the host's `Product` trait
(`sorcerer-lsp/src/serve.rs`), the way conjure's does and without conjure's writer.

- `spellbook_server.rs`: `serve` and the product (`SpellbookServer`); `tracks_text` is true (full
  sync), so the host keeps each open document's text and version (`Client::text_of`) for the
  regions' sync, but the product does nothing on a change: the marks come from the disk, so a
  placeholder replaced and **saved** loses its mark, not on each keystroke. `opened` calls `Server::republish`, `saved` calls `Server::saved` (which
  forgets the file's landing text first, syncs the regions, then republishes the file and every
  other open region file; see "Regions in the editor" below); `closed` publishes an empty list for a file it
  published for. One exception to "from the disk": a file a fix is landing in, or has landed
  in and the person has not saved, is marked over the plan's text (`Server::landing`, filled by
  `remember_landing` in `landing.rs` before the `workspace/applyEdit`, read by
  `diagnostics_now`). Applying a fix makes the editor open a changed file it had not opened, and
  that `opened` would otherwise publish the old disk text's marks over the planned ones. Saving
  or closing the file forgets it; a refused edit forgets it and republishes the open files from
  the disk (`forget_refused_landing`).
- `server_config.rs`: `ServerConfig { open, cache, trust, clock }` (`live(env)`; tests give the
  vendored grammars and a clock they move; there is no `wire`, spellbook has no writer).
  `server_state.rs`: `Server` (`publish`, `republish`, `closed`, `trusted`) and `NOT_TRUSTED`.
- `commands.rs`: the four commands, all through `workspace/executeCommand`, which the product
  takes through `Product::request_later` so an apply can answer once the editor has landed its
  changes (`Server::execute(params, reply)`). Each opens the project
  afresh (config edits need no restart), refuses an untrusted one with a `{ "stop": … }` reply,
  and builds `Wired` (`wiring::wired`):

  | command | arguments | reply |
  |---|---|---|
  | `spellbook.generators` | none | `[GeneratorSummary]` (a broken generator is left out and logged) |
  | `spellbook.choices` | generator, inputs so far, input | `{ choices }` or `{ stop }` |
  | `spellbook.plan` | generator, inputs, and for a fix `{ file, line, rule, index, version }`; the generator `sync` is the regions' refresh (inputs `mode = init` and `region` set one up; a fix argument is ignored) | the `Plan` as JSON (`id`, `files[{path,status,text,lines,region?,set_up?}]`, `notes`, `holes[{name,kind,instruction,source,places[{file,line,text,edges}]}]`; a changed file has `status: "changed"`, `hunks[{header,from,to,lines}]`, `added`, `removed`, `changes`; a fix's plan has `fix{rule,file,line,fingerprint,index,label}`), or `{ stop }` (a missing input adds `input` and `choices`; a finding gone and a fix already applied are stops) |
  | `spellbook.apply` | plan id | `{ changed, written, skipped, refused: null or {path, why}, holes, first: null or {file, line}, stops: [{ file, generation, name, kind, instruction, range }] }` (the holes this plan wrote only), or `{ stop }` for a plan that is gone or a changed file that moved on (`close_table.py changed since the plan; nothing was applied: fix again`) |

  A hole's location is `file` everywhere (it is `HolePlace::file`, and `holes.json`'s key);
  `path` is a planned or written file.
- `plans.rs`: `PlanCache`, the plans made and not applied (`KeptPlan { plan, version }`, the
  version of a fix's document the editor gave), dropped after ten minutes by the clock; `take`
  removes the plan, so a plan is applied once.
- `code_actions.rs`: `Server::code_actions`, a finding's fixes as quick fixes (below).
- `landing.rs`: `Server::land_in_the_editor`, a plan's changed files as one `workspace/applyEdit`
  over `Server::send_edit` (`workspace_edit`, `text_edit`, `edit_label`), and `changed_since_text`
  (below).
- `sync_on_save.rs`: `Server::sync_on_save`, `sync_planned` (a sync over the editor's unsaved
  landings) and `republish_region_files`; see "Regions in the editor".
- `marks.rs` (`hole_mark(hole, row)`: the placeholder's span (the whole line when the row no longer holds it), Information, source `spellbook`, code `hole`, message
  `<name> (<kind>): <instruction>`, `data` the generation id), `diagnostics.rs`
  (`file_diagnostics`: `open_holes` for one file, so the same `still_open` search as the command
  line, plus the file's findings; it returns `FileDiagnostics { marks, stops, findings,
  rules_problem }` and `all()` joins the marks and the findings' marks, since the host's
  `publish` replaces a file's whole list; `file_diagnostics_over` reads the text through another
  `CodebaseView`, which an apply uses for the files the editor has not saved yet). `Server::publish`
  keeps each published file's `Finding`s in `published`, which the code actions read,
  `findings.rs` (`finding_mark`: the whole line, Hint, Warning or Error, source `spellbook`, the rule
  id as its code), `project_paths.rs` (`project_relative`: which editor paths are the project's).

After an apply the server publishes each written file's marks and asks the editor to show the
first file with its first hole's placeholder selected (`Client::show_document`). The span comes
from `placeholder_span` (`domain/open_hole_places.rs`, the same match `still_open` makes, as a
byte range); `file_diagnostics` (`adapters/lsp/diagnostics.rs`) reads the file once and builds
both the marks and `stops`, and `mark_files` (`commands.rs`) joins the files' stops, changed files
first, then written ones in written order. `holes_this_plan_wrote` then keeps only the stops whose
`generation` is the plan's id: a fix that changes a file an earlier generation left holes in (the
`command` generator's `todo_field` in a test file) walks none of them, and with no hole of its own
`first` is null and no document is shown. The older holes stay marked. `stops` are the session's tab stops, each hole by line then column, ranges in
protocol positions (0-based, UTF-16); `first` stays for older plugins. Clients never search text
for a placeholder. Spellbook does not know conjure ran: it
sees the placeholders gone on the next save. The editor side is sorcerer.nvim's
`lua/sorcerer/spellbook.lua` (`M.new`, one chain of callbacks: generators, choices, plan, the
float in `spellbook_plan.lua`, apply, then conjure's `conjure.conjure [uri, line]` on the first
hole when `conjure_holes` is set); it knows only the replies above. After the apply, `finish`
says the end line (`M.end_text`: `spellbook: wrote …`, or after a fix `spellbook: fixed … ·
changed N files`, naming the changed files no window shows, from `not_shown`, as `· x.py not
shown, unsaved`) whether or not a session starts, then hands the stops of the opened file to
`spellbook_holes.lua` (`M.start`), unless the reply's `holes` has no placeholder to fill: one extmark a stop
(UTF-16 converted to bytes, gravity set so typed text stays inside), buffer-local `<Tab>`/`<S-Tab>`
in insert and select mode, each stop selected in select mode the way Neovim's `vim/snippet.lua`
does it. A reply without `stops` (an older server) gets the old end line, so `stops`' fields
`file`, `name`, `instruction` and `range` are part of the contract too. The plugin draws a hole,
a rule finding (source `spellbook`, code the rule id) and a conjure mark (code `change`) by their
`source` and `code` alone (`signs.lua`, `place_marks`), and `:Spellbook check` reads
`spellbook check --json` (`findings[{rule,file,line,severity,message}]`, exit 0/1, 2 = could not
run) into the quickfix list (`M.check`): those fields are part of the contract.

### A fix in the editor

The server offers a fix; the editor runs it through the same `plan` and `apply` as `new`.

1. **The offer.** `textDocument/codeAction` (`code_actions.rs`) reads the findings published for
   the file (`Server::published_findings`, no new check) on the range's lines, and for each one
   `fixes_for` gives the rule's fixes. Each becomes a `CodeAction { title: "spellbook: <label>",
   kind: quickfix, isPreferred: true on the very first, diagnostics: [the finding's mark], command:
   { command: "spellbook.fix", arguments: [{ generator, inputs, missing, fix: { file, line, rule,
   index, label, version: null } }] } }`. An untrusted or switched-off project, rules that do not
   load, or `context.only` without a quick-fix kind give `[]`. `spellbook.fix` is not in
   `executeCommandProvider`: it is the editor's own command, which asks for `missing`, sets
   `version` to the document's version, and enters the `new` chain at the plan.
2. **The plan.** `spellbook.plan [generator, inputs, fix]` calls `plan_fix` (the fix's own
   generator plans; `generator` only names it) and keeps the plan with `fix.version`.
3. **The landing.** `spellbook.apply` on a plan with changed files, for an editor that can apply
   edits (`Client::can_apply_edits`), sends one `workspace/applyEdit` labelled `spellbook:
   <label>` (through `Client::apply_tool_edit`, so lackey does not triage its lines): a `TextDocumentEdit` per changed file, whose edits are the file's `changes` (whole
   lines, so column 0, except past an open last line, `text_edit`), the fix's file carrying
   the kept version and the others none. The apply is answered only in the edit's reply
   callback: applied, `Server::applied` runs `apply_plan(…, ChangesLanded::ByTheEditor)` (new
   files, record, holes), publishes the changed files' marks read through a `PlannedView` of
   their planned text (the editor has not saved them) and the written files' from the disk,
   opens the first hole, and replies as for `new` plus `changed`; refused, it replies
   `{ stop: changed_since_text }` and nothing is written. An editor that cannot apply edits gets
   the command line's path (`ChangesLanded::NotYet`, `write_changed`), said once with
   `spellbook: your editor cannot apply edits; changed files were written to disk`.

The VS Code side is `editors/vscode/spellbook/`, the same contract in TypeScript.
`src/newFlow.ts` (`newFlow`) is the story over two seams, a `Server` and an `Asker`: the four
commands, sent by `newCommand.ts` (`serverOf`) as `ExecuteCommandRequest`s to the folder's own
client, never through VS Code's command registry; `asker.ts` is the Asker of VS Code's pickers, and
the e2e swaps parts of it (`useAsker`). The plan preview is `planPreview.ts` (`PlanPreviews`, a
content provider for the scheme `spellbook-plan`, one read-only document per new file, coloured by
VS Code from the path, under a QuickPick of Apply, Cancel and the files). After the apply,
`newCommand.ts` (`finish`) gives the opened file's `stops` to `holeSession.ts`
(`startHoleSession`): `holes.ts` (`snippetFor`) turns the lines from the first stop to the last
into one snippet with each stop as `${n:…}`, put back over the same lines with
`editor.insertSnippet` (`keepWhitespace`), so the session is VS Code's own snippet mode; the
status line follows the stops with `shifted` and `stopAt` (also `holes.ts`). `checkCommand.ts`
runs `spellbook check --json` into the `spellbook check` diagnostic collection, leaving out open
files (the server shows those). `holeMarks.ts` draws the hole look from the marks' `source` and
`code` (`marks.ts`, `holeRanges`). Both clients read `stops`, `first`, the hole mark's
`source`/`code` and the check JSON: the contract.

A fix runs through the same chain. `fixCommand.ts` registers `spellbook.fix` (the code action's
command: the editor, not the server, runs it), `spellbook.fixHere` (`ctrl+m b f`, which asks
`vscode.executeCodeActionProvider` for the cursor's line and runs the first `spellbook.fix`
action, `fix.ts` `preferredFix`) and `spellbook.fixThis` (`ctrl+m f`, fix this: `fix.ts`
`fixThis` picks the spellbook fix, else the one preferred quick fix of another server, else the
quick-fix menu, leaving out scry's, conjure's and lackey's actions, which fix nothing).
`holesCommand.ts` registers `spellbook.holes` (`ctrl+m b h`): `spellbook holes --json` in the
folder's root, read by the pure `openHoles.ts`, in a QuickPick that opens the hole picked. `fixHere` refuses a dirty buffer (`dirtyText`), then calls
`flowIn` (`newCommand.ts`) with a `Given` that carries the action's `generator`, `inputs`,
`missing` and `fix` with the document's `version` filled in (the action has `version: null`):
`newFlow` asks only the `missing` inputs and sends `spellbook.plan [generator, inputs, fix]`
(`fix.ts` `fixRequest`). `planPreview.ts` shows a `changed` file as `vscode.diff` of a
`spellbook-plan-before` document (its text on disk, read under the folder) against the
`spellbook-plan` one; `plan.ts` (`planTitle`, `fileItems`) words the `~ path (+a −r)` rows and the
`fix ... · 2 files changed` title. The apply is `client.sendRequest(ExecuteCommandRequest)`, which
does not stop the language client answering the server's `workspace/applyEdit` meanwhile: that
request is what the apply waits for. `finish` then says `fixedText` (`fix.ts`) and starts the hole
session even in a changed file, which is unsaved by then. The end line is said whether or not a
session starts; with one it counts no holes (`holesOnEndLine`, `holes.ts`), since the session's
line says them. `fixedText` names the changed files no tab holds (`notShown` in `fix.ts`, over
`inATab` in `fixCommand.ts`, which reads `vscode.window.tabGroups`).

The fix actions are `kind = "quickfix"` with `isPreferred` on the first (`fix_action` in
`adapters/lsp/code_actions.rs`); conjure's are `refactor.rewrite`. VS Code orders the menu by that.
Neovim does not: `vim.lsp.buf.code_action` lists each client's answers in client order, so
sorcerer.nvim's `lua/sorcerer/code_actions.lua` wraps `vim.ui.select` (`M.wrap`, re-applied on every
`LspAttach` in `M.setup`) and, for `opts.kind == "codeaction"` only, reorders the items with
`M.ordered`: each client's quick fixes as one block (`ctx.client_id`; its preferred ones first,
`preferred_first`), the blocks that start with a preferred fix before the others, each in the
order the clients came; then the rest in its order. A group by kind alone let a second server's
preferred fix (Ruff's `Organize imports`) sit between spellbook's two. No server is named;
`fixes_first = false` skips it.

In sorcerer.nvim the fix is `lua/sorcerer/spellbook.lua` `M.fix(arg, ctx)`, registered as the
client's `commands["spellbook.fix"]` in `M.attach` (`vim.lsp.start`'s `commands`), so a code action
with that command runs in the editor and never goes to the server. It refuses a modified buffer
(`dirty_text`), sets `fix.version` from `vim.lsp.util.buf_versions`, and runs the New chain
(`ask_inputs` with `only = missing`, `make_plan` with the `fix` argument, `spellbook_plan.show`,
`apply`). The apply is a plain `executeCommand`: the plugin registers no `workspace/applyEdit`
handler and never blocks on a request, so Neovim's default handler lands the server's edit while the
apply is pending and answers it (`util.apply_workspace_edit`: one undo step a buffer, buffers left
modified; a `version: null` is `vim.NIL`, which that function accepts). `spellbook_plan.lua`
(`M.lines`, `M.title`) draws `~ path (+a −r)` rows and each hunk with `DiffAdd`/`DiffDelete` line
highlights. `keys.lua` binds `spellbook_fix` (`<leader>mbf`) to `M.fix_here`, which asks
`vim.lsp.buf.code_action` for the first `spellbook.fix` quick fix on the line (`:Spellbook fix
k=v` is the same with answers); the common `fix` (`<leader>mf`, `fix_this` in `keys.lua`) asks
every client on the buffer for the line's quick fixes (`M.actions_on_row`) and runs `M.fix_here`
when one is a spellbook fix, else the line's preferred quick fix. `M.check(files)` runs
`spellbook check --json [files]` (`<leader>mbk` is `M.check_file`, the current file) and
`M.holes` runs `spellbook holes --json` (`:Spellbook holes`, `<leader>mbh`), both into the
quickfix list through `spellbook_json`.

### Regions in the editor

A save keeps the owned regions (`Server::saved` in `server_state.rs`: `forget_landing`, then
`sync_on_save`, then `republish` of the saved file and `republish_region_files`, every other open
file a region's `instances` name, since a file written or deleted makes another file's block stale or
fresh). `sync_on_save` (`adapters/lsp/sync_on_save.rs`) reads as its steps:

1. `wired_for_commands`: nothing in an untrusted or switched-off project.
2. `sync_planned(wired, Refresh)`: `plan_sync` over a `PlannedView` of `Server::landing_texts`
   overlaid with `open_documents` (each open file the disk still holds, with the text and version
   the editor last reported), so an open region file is planned from what its buffer holds,
   unsaved changes included, and a landed edit is not planned twice. It returns an `EditorSync {
   plan, versions }`: the version of each open file the plan read.
3. A `PlanStop` (broken markers, a `regions.toml` that does not load) is `say_once` with
   `plan_stop_text`; an empty plan says nothing.
4. `[spellbook] sync_on_save = false` (`spellbook_config`): `say_once` `spellbook: N region(s) out of
   date; run spellbook sync`, and the blocks are left for their findings, which the republishing of
   step 6 shows on open region files.
5. `split_sync(plan, versions)` gives a `SyncSplit`: a file the editor has open with a version
   (and only for an editor that `can_apply_edits`) goes to `land_in_open_files`, one `send_edit`
   labelled `spellbook: sync region NAME` with each document tied to that version, so the editor
   refuses a buffer that moved on; the answer `applied` records the sync (`record_landed_sync`,
   `apply_plan(…, ByTheEditor)`, a `generations.jsonl` line whose generator is `sync`), says
   `spellbook: synced region NAME in FILE (+a −r)` per file and republishes from the landed text;
   a refusal (`send_edit` already forgot the landing and republished from the disk) says `… was
   not synced (the editor refused the edit); run spellbook sync`. A file not open goes to
   `write_region_files`: `apply_plan(…, NotYet)`, `write_changed` onto the text the plan saw, the
   same line per file. An open file with no version known is never edited through the editor: it
   is said not synced and keeps its `region-stale` finding (`left_stale`).
6. `saved` republishes, as above.

`Server::send_edit(plan, versions, answered)` (`landing.rs`) is the one sender of a plan as an edit:
it remembers the landing texts, sends `workspace/applyEdit` labelled by `edit_label` (`spellbook:
<fix label>`, `spellbook: <generator>`, or for a sync `spellbook: sync region NAME`, the names from
`region_names`), forgets the landing on a refusal, and runs `answered(server, plan, applied)` on the
main loop; `land_in_the_editor` (an apply) and `land_in_open_files` (a save) both use it.

The quick fix: `code_actions.rs::offers_for` gives a finding with a `region` the built-in
`region_fix_offer` (the same one `check --json` prints), so `region-stale` offers `spellbook: Sync
region NAME` and `region-not-set-up` `spellbook: Set up region NAME`, each a `spellbook.fix` with
`generator: "sync"`, `inputs { region, file[, mode: init] }` and `missing: []`. `commands.rs::plan`
sends the generator `sync` (`SYNC_GENERATOR`, `usecases/sync_regions.rs`) to `Server::planned_sync`,
which picks `SyncMode` from the inputs and calls `sync_planned`, keeping the plan with its versions
(`KeptPlan { plan, versions }`; a fix's plan keeps the fix's document version the same way). Its
apply is `apply_sync`, the same `split_sync` as a save: the files not open are written to disk at
once, the open ones land as one edit tied to the versions the plan read, and the reply (`changed`,
no holes) waits for the editor's answer; a refused edit is a stop naming the file.

The clients learn one word. VS Code: `plan.ts` words a file with `region` as `~ path · region NAME:
+a −r` (`set up, N items` with `set_up`) and a sync's title (`spellbook: sync · 1 file changed`);
`newFlow.ts` takes the generator `sync` without asking the server for generators (`SYNC_GENERATOR`)
and ends a sync with no files with `syncedText` (`spellbook: regions up to date`, then the notes);
`syncCommand.ts` registers **Sync regions** (`spellbook.sync`, `ctrl+m b s`) as `flowIn` with
`{ generator: 'sync' }`; a region's quick fix goes through `fixCommand.ts` unchanged. The server's
`window/showMessage` is taken over after the client starts (`extension.ts`,
`showMessagesInPlace`): `messages.ts` (`messagePlace`) sends the save's `spellbook: synced region …`
lines to the status bar and every other message to a notification of its level. sorcerer.nvim:
`spellbook_plan.lua` (`changed_text`, `M.title`) words the same rows; `spellbook.lua` `M.sync`
(`:Spellbook sync`, `keys.lua`'s `spellbook_sync`, `<leader>mbs`) and `M.fix` for a `sync` argument call
`planned_sync`, which plans `["sync", inputs]`, says `regions up to date` for no files, refuses a
region file whose buffer is modified (`dirty_text`), and shows the float; the save's edit and
message need nothing (Neovim's own `workspace/applyEdit` and `window/showMessage` handlers). The
contract grows by `files[].region`, `files[].set_up`, `notes`, the two finding codes with
`region`/`added`/`removed`, and the generator name `sync`.

## A check

`spellbook check` (`cli/check.rs`) and the server share one path:

1. `Wired::checking()` (`adapters/checking.rs`) reads `.sorcerer/spellbook/rules.toml` fresh
   (`load_rules` → `parse_rules`, so a bad file is a sentence, not a panic) and `[spellbook]`, lays
   `[spellbook.rules]` over the rules (`with_project_settings`: `Checking.rules` is what runs,
   `CheckDeps.off` the ids turned off, which `allow_warnings` still counts as known), reads
   `baseline.toml` (`load_baseline`), and builds a `QueryRules` over the project's `CachedGrammars`.
2. `check_files(deps, files)` (`usecases/check_files.rs`), for each file: skip it when it matches
   `[spellbook] exclude`, has no language (`Grammars::for_path`) or is not text; keep the rules whose
   `files` pattern matches and no `exclude` does (`Rule::applies_to`); ask the `RuleRunner` for raw
   findings; keep one a line for each rule; read the file's allow comments (`allow_comments` over `RuleRunner::comments_in`, the parser's comment nodes, so text in a string is never one; with the
   language's `CommentMarks`: the start right before `spellbook:allow`, and a closing `-->` taken off the reason) and drop each finding one names (`silencing`: its line or the one
   above); add an `allow-without-reason` or `unknown-rule` warning on each comment that needs it; add
   the regions' findings; sort; mark the baselined ones (`Baseline::marked`); then
   `ReportStore::replace_files` keeps just those files' findings in `report.json`. The rules' half is
   `rule_findings_of_files`, the regions' `regions_out_of_step(deps, files)`.
3. `check_project` (`usecases/check_project.rs`) is the same over `view.glob("**")` less
   `.sorcerer/` and `.git/`, and `replace_all`. `check_since(deps, rev)` (`usecases/check_since.rs`)
   takes `view.changed_since(rev)` (still on disk), runs `rule_findings_of_files` on them, marks the
   baselined ones over those whole findings (`marked_unchanged_first`, the untouched lines first, so a
   pasted copy is the new one), then keeps a finding when its line is in
   `added_lines(text_at(rev), text)` (`domain/change.rs`, a line diff with `\r\n` folded; a file
   absent at `rev` is all new), adds `regions_out_of_step(deps, None)` (every region) and sorts; it
   writes no report. `check_texts(deps, texts)`
   (`usecases/check_texts.rs`) lays the texts over the view as a `PlannedView`, runs
   `rule_findings_of_files` on their paths with `CheckDeps { view, ..deps }`, sorts and marks; no
   regions, no report. It is what `check --texts` runs for conjure.
4. `QueryRules::findings_in` (`adapters/query_rules.rs`) parses the file with `Loaded::parse`,
   compiles each rule's query once for each language (`sorcerer_core::domain::query::compile`, which
   refuses predicates tree-sitter would skip; the cache is per `QueryRules`, so per command or per
   save), refuses one with no `@finding` capture, and reports each capture's start line and text.

The server's `file_diagnostics` calls `check_files` for the one file, so a save refreshes the report
and the findings; a rules file that does not load is said once with `Client::say_once`.
`lsp/findings.rs::finding_mark` makes an existing finding a Hint ending ` (existing)`.

`cli/rules.rs` (`spellbook rules FILE [--json]`) lists `Rule::applies_to` of the rules `checking()`
loaded (turned-off ones are not in them): id, severity, message, why. Conjure's `SpellbookChecker::rules_for`
reads it to tell its writer the rules up front.

`cli/check.rs` turns the flags into a `CheckScope` (`Project`, `Files`, `Since`, `Texts`, the
last read by `texts_to_check` from the `--texts` file or standard input, refusing a path that is
absolute or climbs out; its findings offer no fixes) for `check`, and
`--baseline` into `record_baseline`: `check_project`, `Baseline::of`, `change_to` the old one,
`write_baseline`, and `baselined_text`. `is_blocking` (an error not existing) sets the exit code;
`verdict_text` counts the existing; a whole-project check adds the paid-off line from `change_to`.

## Why a grammars port

`scripts/layers` refuses `sorcerer_core::adapters` in `usecases/`, and loading a grammar
(`Languages::load`) is core adapter work (it downloads or builds into the cache). So the use
cases ask `Grammars::for_path`, and `CachedGrammars` answers it. `Loaded` is a core domain type,
so the port may return it.

## The tests

The tests live in `crates/spellbook/tests/`, because they name languages
(`scripts/no-language-names`). Unit tests sit in each `domain` file. `support/bistro_fixtures.rs`
copies conjure's Python corpus base (`corpus/conjure/bistro/base`) and installs the corpus's own
`command` generator (`corpus/spellbook/bistro/spellbook/generators/command`, read with `include_str!`,
so the corpus is the one source of truth); `support/project_fixtures.rs` points the grammars at their
test sources (core's `tests/support/test_grammars.rs`, included by path: vendored or fetched by
`scripts/test-grammars`), as conjure's tests do, so no test touches the network.
`support/corpus_cases.rs` holds the corpus case runners (`run_new_cases`, `run_check_cases`,
`run_fix_cases`, `run_sync_cases`, `run_adopt_cases`, `run_load_cases`, each given a corpus folder):
`corpus.rs` runs them over `corpus/spellbook/<name>/`, and `languages.rs` over every
`corpus/languages/<name>/` (check cases; new cases where the language has `[holes]`, and a hole of
every kind left by its new and fix cases; sync cases where it has `comment`; fix cases).
`rules.rs` covers the corpora's rules (path patterns on real paths, queries the grammars accept) and
the loader; `check.rs` the use cases on a copy of each base (a planted break, the allows, the report); `lsp.rs` drives the server over an in-memory connection with a small editor (the handshake, the
four commands, the marks on open, save and close, trust); `generators.rs` covers `[holes]` and the
loader's sentences; `plan.rs` inputs, rendering and planning; `apply.rs` writing, the records and the
open holes (a link out of the project made with core's `link_to`); `cli.rs` each command through
`main_with` with a wiring that uses the vendored grammars and a fixed clock (the fix session of the
design end to end, `the_fix_session_of_the_design_runs_end_to_end`); `anchors.rs` each phrase over
command-shaped files (one-line and one-per-line lists, a call narrowed with `in`); `fix.rs`
`fixes_for` and `plan_fix` + `apply_plan` with `use-port` (on the import, on a `sqlite3.connect`
statement with its hole, and a second run that has nothing to do). `use_corpus_rules` copies the
corpus's generators with its rules, since the rules' fixes name them. `cli.rs` also runs the
`starter` corpus's `app` (the font's bytes, the binary row, the chain's notes once each, a
`--set timestamp` naming the used migration, `plan.notes` in `--json`) and `init --pack` on a local
repository made with `git` in a temporary folder (a tag, the project's own file kept, `state/` left
out, the default branch named by its folder, a project with a spellbook refused, a revision git
lacks); `generators.rs` the `copy` and `[notes]` sentences and `{{ name }}`.

`adoption.rs` covers the baseline, `check --since` (in a fresh git repository) and `[spellbook.rules]`
through `main_with`; the corpus's `adopt` cases (`corpus/spellbook/README.md`) run the same on bistro
and bistro-rs. `regions.rs` covers `plan_sync`, `region_findings` and a `new` refreshing a region on bistro-rs;
`sync.rs` the `sync` command, `check`'s region findings and fix, `new`'s region row and notes, the exit
codes and `doctor`'s row, through `main_with`. `lsp.rs`'s region tests run on bistro-rs with the
corpus's `base/` laid over it (`support::bistro_rs_with_regions`): a save that writes a closed region
file and says so, one that lands in an open file as an edit (and no second edit on the next save), a
refused edit (said, the finding back), `sync_on_save = false` (the finding, said once), the two quick
fixes, and `spellbook.plan ["sync", …]` (refresh with region rows and hunks, `mode = init`, nothing to
do, a generator's plan naming its region). The VS Code e2e's last case saves hand-written commands in
bistro with the corpus's `__init__.py` markers in place: closed, the file is synced on disk; open, the
buffer gets the edit and stays unsaved.

`corpus.rs` runs the corpora in `corpus/spellbook/<name>/` (`bistro` Python, `bistro-rs` Rust, and
`starter`, which has no `base` in its `corpus.toml`: an empty project, where `app` writes a manifest,
a main, a copied script and a binary font and plans the first migration by `[[uses]]`; a `new` case
compares a written file byte for byte when it is not UTF-8, `assert_same_file`);
`corpus.toml`'s `overlay` (the corpus's `base/`, the files that carry region markers) is laid over the
shared base for every case unless the case says `base = "shared"` (`project_of(dir, corpus, Base)`);
`cases/sync/<id>.{toml,patch}` runs the command line through `main_with` (`mode` for `sync`, or a whole
`command`) and compares what it printed with `expected/sync/<id>.txt`, its exit code with `exit`, and
every file it changed with `expected/sync/<id>/` (`texts_of` before and after); a load case with
`sync = true` is a mistake in `regions.toml`, its sentence the sync's stop; a `new` case compares the
files it changed (a region's) as well as those it wrote; each also has `cases/check/<id>.{toml,patch}` (a planted break, every file checked, `expected/check/<id>.json` the findings) and a `rules.toml` among its `spellbook/` files. A
corpus is a base copied from another corpus plus a `spellbook/` folder that becomes
`.sorcerer/spellbook/`; its cases are `cases/new/<id>.toml` (a generator and inputs; every written
file is compared with `expected/new/<id>/<path>` after `\r\n` becomes `\n`, and the open holes, one a
place, with the case's `[[holes]]`), `cases/holes/<id>.{toml,patch}` (a `new`, then a `git apply`
of the person's edit), `cases/fix/<id>.{toml,patch}` (a planted finding, then `plan_fix` and `apply_plan` with the case's fix and inputs; every changed or written file is compared with `expected/fix/<id>/<path>`, and the open holes with the case's `[[holes]]`) and `cases/load/<id>/` (an overlay with one mistake, and `expected.txt`: the
sentence, from the generator's load error or, for a path the plan refuses, the plan's stop). To add a
case, write its toml, run the corpus test with `SPELLBOOK_BLESS=1 … -- --nocapture` (it writes
`expected/` and prints the holes), read every file written, paste the holes in, and run it again
without the variable. `corpus/spellbook/README.md` has the layout.
