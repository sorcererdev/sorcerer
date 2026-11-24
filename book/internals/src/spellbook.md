# Spellbook

`crates/spellbook/` is the package `sorcerer-spellbook`: the library `spellbook` and the program
`spellbook`. It comes from olifant, whose `olifant gen` wrote a convention's files from a
generator directory; spellbook keeps olifant's `generator.toml` keys and its error sentences,
and replaces olifant's tab stops with **holes**: placeholders that parse and say what belongs
there, so a person, an agent or conjure can fill them.

Spellbook depends on `sorcerer-core` and `sorcerer-lsp` only. It makes no model call.

## The modules

`domain/` (data and rules, no I/O):

- `config.rs`: `SpellbookConfig { enabled, exclude }`, `spellbook_config`, `PRODUCT`, `SPELLBOOK_IS_OFF`; `enabled` is read by `cli/run_cli.rs::run_command` and
  the server (`republish`, `wired_for_commands`).
- `generator.rs`: the types (`Generator`, `Input`/`InputKind`, `Query`/`QueryValue`,
  `Edit`/`EditBody`/`OnExists`, `HoleSource`) and `read_generator(id, text, templates)`, which
  reads `generator.toml` key by key with errors naming the place (`[inputs.app] type: …`). The
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
  without resolving earlier inputs, and `one_of` and `choices` together are refused by `read_generator`); every edit's template exists; paths, derives and derived holes use known names and no
  holes; every template parses as a template (core's `pieces`) and uses known names; a hole
  has one kind and one plain instruction; a placeholder hole's kind is in the generator
  language's `[holes]`.
- `holes_table.rs`: `holes_table(language)` reads `[holes]` strictly from `Language.rest`;
  `placeholder(language, kind, name, instruction)` fills a kind's template with core's `fill`.
- `inputs.rs`: `Given` (what was typed), `Values` (inputs then derived names, in order, as
  `fill` takes them; `with_derived`), `InputStop` and `input_stop_text`, `is_a_name`, and
  `query_choices(files, glob, value)`: the glob compiled with `literal_separator` (so `*` is one
  folder), matched against every file **and every folder above it**, read as the parent
  folder's name, the stem or the path.
- `template.rs`: `render(template, values, derived, placeholder_of)` → `Rendered { text, holes }`;
  each hole's line is counted where its text lands, so a block placeholder keeps the
  indentation the template gave the hole.
- `holes.rs`: `Hole { name, kind, instruction, source: HoleOrigin, places }`,
  `HolePlace { file, line, text }`, and `HoleBook`, which gathers one generation's holes file by
  file (one hole per name, every place).
- `plan.rs`: `Plan { id, generator, inputs, files, holes }`, `PlannedFile`, `FileStatus`,
  `PlanStop` and `plan_stop_text`, `relative_path` (lexical: no absolute path, no `..` out),
  `plan_id` (eight hex digits of `fnv64_hex` over the generator, the inputs and the time).
- `record.rs`: `GenerationRecord` (a line of `generations.jsonl`), `OpenHole` and `HolesFile`
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
  (strict keys; `missing`/`fix`/`fixes` say "arrives in milestone 2"; both `match` and `query` is
  refused; every regex is compiled once to catch a bad one early). `rule_query.rs`: `query_text`, the
  short form written as tree-sitter query text (`@finding`, `#match?`, `#not-match?`, backslash and
  quote escaped).
- `finding.rs` (`Finding`, `RawFinding`, `fingerprint`: seven hex digits of `fnv64_hex` over the
  rule and the node text with whitespace collapsed), `allow.rs` (`allow_comments`, `silencing`),
  `report.rs` (`ReportFile`, `with_files_replaced`, `sort_findings`; `report.json` is a core
  `Versioned` at version 1).

`ports/`: `generators.rs` (`Generators`: `all`, `named`), `grammars.rs` (`Grammars::for_path`:
the loaded language of a path, or none), `workspace.rs` (`Workspace::write_new`, `WriteRefused`),
`generation_store.rs`, `holes_store.rs`, `clock.rs`, `rule_runner.rs` (`RuleRunner::findings_in`), `report_store.rs`.

`usecases/`: `list_generators.rs` (`GeneratorSummary`, also the editor's picker reply),
`choices_for.rs` (`resolve_inputs`, `choices_for`), `plan_generation.rs` (`PlanDeps`,
`plan_generation`), `apply_generation.rs` (`ApplyDeps`, `apply_generation`,
`AppliedGeneration`), `open_holes.rs` (`HolesDeps`, `open_holes`), `check_files.rs` (`CheckDeps`, `check_files`), `check_project.rs`
(`check_project`).

`adapters/`: `generator_dir.rs` (`GENERATORS_DIR`, `generators_in`, `generator_named`,
`load_generator`, `GeneratorDirs`), `cached_grammars.rs` (`CachedGrammars` over
`Languages::for_path` and `Languages::load`), `disk_workspace.rs`, `jsonl_generation_store.rs`,
`json_holes.rs` (core's `Store` at `spellbook/holes.json`), `json_report.rs` (the same at
`spellbook/report.json`), `rules_toml.rs` (`load_rules`, `RULES_FILE`), `query_rules.rs` (`QueryRules`,
the `RuleRunner` over tree-sitter), `checking.rs` (`Wired::checking()`: the rules read fresh, the settings
and the runner, then `Checking::deps()`), `system_clock.rs`, `wiring.rs` (`Wired`, `live`, `wired`, and
`plan_deps`/`apply_deps`/`holes_deps`).

`adapters/lsp/`: the server, below.

`cli/`: `arguments.rs`, `file_arguments.rs` (`relative_to_root`: `check`/`holes` file arguments made project-relative), `run_cli.rs` (`main`, `main_with(args, cwd, env, wire, out, err)`),
one file a cli subcommand (`list.rs`, `show.rs`, `new.rs`, `holes.rs`, `check.rs`, `doctor.rs`, `trust.rs`, `lsp.rs`),
`format.rs` (`columns`), `console.rs`.

## One generation, end to end

1. `generator_named` loads the directory: `read_generator`, then `check_generator`.
2. `resolve_inputs` takes the inputs in the generator's order: a given value (an enum's
   checked against `choices_for`, an ident's against `is_a_name`), else an enum's only choice,
   else the default, else empty when optional, else `InputStop::Missing` with the choices. Then
   `Values::with_derived` adds `[derive]` in order.
3. `plan_generation`, for each edit: fill the path and keep it (`relative_path`, then
   `Fences::allows_relative`); an existing file is skipped or stops the plan; the body is a
   text, or a template rendered with `render` and the placeholders of **the language of the
   file's own path** (`Grammars::for_path`): a generator's `.html` or empty `__init__.py` is not
   the generator's language. A placeholder hole in a file no language covers stops the plan.
   Every new file a language covers is parsed (`parse_errors_of`); an error line is
   `PlanStop::DoesNotParse` naming the template. Then the holes are gathered (`HoleBook`) and
   the id made. Nothing is written.
4. `apply_generation` writes each new file through `Workspace::write_new` (`DiskWorkspace`
   refuses with `Fences::allows`, which follows links, and opens with `create_new`, so a file
   that appeared since the plan is never overwritten), stops at the first refusal, appends the
   record for what was written, and adds the placeholder places to `holes.json`.
5. `open_holes` re-reads each saved place and keeps it at its current line while
   `still_open` finds it; `holes.json` is rewritten with what is still open. A hole whose file is
   missing is kept (a stash or branch switch may bring it back) but not listed.

## The server

`spellbook lsp` (`cli/lsp.rs`, which runs before the project is opened: the editor sends its
root at initialize) serves `adapters/lsp/` through the host's `Product` trait
(`sorcerer-lsp/src/serve.rs`), the way conjure's does and without conjure's writer.

- `spellbook_server.rs`: `serve` and the product (`SpellbookServer`); `tracks_text` is false:
  the marks come from the disk, so a placeholder replaced and **saved** loses its mark, not on
  each keystroke. `opened` and `saved` call `Server::republish`; `closed` publishes an empty
  list for a file it published for.
- `server_config.rs`: `ServerConfig { open, cache, trust, clock }` (`live(env)`; tests give the
  vendored grammars and a clock they move; there is no `wire`, spellbook has no writer).
  `server_state.rs`: `Server` (`publish`, `republish`, `closed`, `trusted`) and `NOT_TRUSTED`.
- `commands.rs`: the four commands, all through `workspace/executeCommand`. Each opens the project
  afresh (config edits need no restart), refuses an untrusted one with a `{ "stop": … }` reply,
  and builds `Wired` (`wiring::wired`):

  | command | arguments | reply |
  |---|---|---|
  | `spellbook.generators` | none | `[GeneratorSummary]` (a broken generator is left out and logged) |
  | `spellbook.choices` | generator, inputs so far, input | `{ choices }` or `{ stop }` |
  | `spellbook.plan` | generator, inputs | the `Plan` as JSON (`id`, `files[{path,status,text,lines}]`, `holes[{name,kind,instruction,source,places[{file,line,text,edges}]}]`), or `{ stop }` (a missing input adds `input` and `choices`) |
  | `spellbook.apply` | plan id | `{ written, skipped, refused: null or {path, why}, holes, first: null or {file, line}, stops: [{ file, name, kind, instruction, range }] }`, or `{ stop }` for a plan that is gone |

  A hole's location is `file` everywhere (it is `HolePlace::file`, and `holes.json`'s key);
  `path` is a planned or written file.
- `plans.rs`: `PlanCache`, the plans made and not applied, dropped after ten minutes by the
  clock; `take` removes the plan, so a plan is applied once.
- `marks.rs` (`hole_mark(hole, row)`: the placeholder's span (the whole line when the row no longer holds it), Information, source `spellbook`, code `hole`, message
  `<name> (<kind>): <instruction>`, `data` the generation id), `diagnostics.rs`
  (`file_diagnostics`: `open_holes` for one file, so the same `still_open` search as the command
  line, plus the file's findings; it returns `FileDiagnostics { marks, findings, rules_problem }`
  and `all()` joins them, since the host's `publish` replaces a file's whole list),
  `findings.rs` (`finding_mark`: the whole line, Hint, Warning or Error, source `spellbook`, the rule
  id as its code), `project_paths.rs` (`project_relative`: which editor paths are the project's).

After an apply the server publishes each written file's marks and asks the editor to show the
first file with its first hole's placeholder selected (`Client::show_document`). The span comes
from `placeholder_span` (`domain/open_hole_places.rs`, the same match `still_open` makes, as a
byte range); `file_diagnostics` (`adapters/lsp/diagnostics.rs`) reads the file once and builds
both the marks and `stops`, and `mark_written_files` (`commands.rs`) joins the files' stops in
written order. `stops` are the session's tab stops, each hole by line then column, ranges in
protocol positions (0-based, UTF-16); `first` stays for older plugins. Clients never search text
for a placeholder. Spellbook does not know conjure ran: it
sees the placeholders gone on the next save. The editor side is sorcerer.nvim's
`lua/sorcerer/spellbook.lua` (`M.new`, one chain of callbacks: generators, choices, plan, the
float in `spellbook_plan.lua`, apply, then conjure's `conjure.conjure [uri, line]` on the first
hole when `conjure_holes` is set); it knows only the replies above. After the apply, `finish`
hands the stops of the opened file to `spellbook_holes.lua` (`M.start`): one extmark a stop
(UTF-16 converted to bytes, gravity set so typed text stays inside), buffer-local `<Tab>`/`<S-Tab>`
in insert and select mode, each stop selected in select mode the way Neovim's `vim/snippet.lua`
does it. A reply without `stops` (an older server) gets the old end line, so `stops`' fields
`file`, `name`, `instruction` and `range` are part of the contract too. The plugin draws a hole,
a rule finding (source `spellbook`, code the rule id) and a conjure mark (code `change`) by their
`source` and `code` alone (`signs.lua`, `place_marks`), and `:Spellbook check` reads
`spellbook check --json` (`findings[{rule,file,line,severity,message}]`, exit 0/1, 2 = could not
run) into the quickfix list (`M.check`): those fields are part of the contract.

## A check

`spellbook check` (`cli/check.rs`) and the server share one path:

1. `Wired::checking()` (`adapters/checking.rs`) reads `.sorcerer/spellbook/rules.toml` fresh
   (`load_rules` → `parse_rules`, so a bad file is a sentence, not a panic) and `[spellbook]`, and
   builds a `QueryRules` over the project's `CachedGrammars`.
2. `check_files(deps, files)` (`usecases/check_files.rs`), for each file: skip it when it matches
   `[spellbook] exclude`, has no language (`Grammars::for_path`) or is not text; keep the rules whose
   `files` pattern matches and no `exclude` does (`Rule::applies_to`); ask the `RuleRunner` for raw
   findings; keep one a line for each rule; read the file's allow comments (`allow_comments` over `RuleRunner::comments_in`, the parser's comment nodes, so text in a string is never one; with the
   language's `comment` marker) and drop each finding one names (`silencing`: its line or the one
   above); add an `allow-without-reason` or `unknown-rule` warning on each comment that needs it; sort;
   then `ReportStore::replace_files` keeps just those files' findings in `report.json`.
3. `check_project` (`usecases/check_project.rs`) is the same over `view.glob("**")` less
   `.sorcerer/` and `.git/`, and `replace_all`.
4. `QueryRules::findings_in` (`adapters/query_rules.rs`) parses the file with `Loaded::parse`,
   compiles each rule's query once for each language (`sorcerer_core::domain::query::compile`, which
   refuses predicates tree-sitter would skip; the cache is per `QueryRules`, so per command or per
   save), refuses one with no `@finding` capture, and reports each capture's start line and text.

The server's `file_diagnostics` calls `check_files` for the one file, so a save refreshes the report
and the findings; a rules file that does not load is said once with `Client::say_once`.

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
so the corpus is the one source of truth); `support/project_fixtures.rs` points the grammars at core's
vendored sources, as conjure's tests do, so no test touches the network.
`rules.rs` covers the corpora's rules (path patterns on real paths, queries the grammars accept) and
the loader; `check.rs` the use cases on a copy of each base (a planted break, the allows, the report); `lsp.rs` drives the server over an in-memory connection with a small editor (the handshake, the
four commands, the marks on open, save and close, trust); `generators.rs` covers `[holes]` and the
loader's sentences; `plan.rs` inputs, rendering and planning; `apply.rs` writing, the records and the
open holes (a link out of the project made with core's `link_to`); `cli.rs` each command through
`main_with` with a wiring that uses the vendored grammars and a fixed clock.

`corpus.rs` runs the corpora in `corpus/spellbook/<name>/` (`bistro` Python, `bistro-rs` Rust); each also has `cases/check/<id>.{toml,patch}` (a planted break, every file checked, `expected/check/<id>.json` the findings) and a `rules.toml` among its `spellbook/` files. A
corpus is a base copied from another corpus plus a `spellbook/` folder that becomes
`.sorcerer/spellbook/`; its cases are `cases/new/<id>.toml` (a generator and inputs; every written
file is compared with `expected/new/<id>/<path>` after `\r\n` becomes `\n`, and the open holes, one a
place, with the case's `[[holes]]`), `cases/holes/<id>.{toml,patch}` (a `new`, then a `git apply`
of the person's edit) and `cases/load/<id>/` (an overlay with one mistake, and `expected.txt`: the
sentence, from the generator's load error or, for a path the plan refuses, the plan's stop). To add a
case, write its toml, run the corpus test with `SPELLBOOK_BLESS=1 … -- --nocapture` (it writes
`expected/` and prints the holes), read every file written, paste the holes in, and run it again
without the variable. `corpus/spellbook/README.md` has the layout.
