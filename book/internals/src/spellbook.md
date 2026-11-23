# Spellbook

`crates/spellbook/` is the package `sorcerer-spellbook`: the library `spellbook` and the program
`spellbook`. It comes from olifant, whose `olifant gen` wrote a convention's files from a
generator directory; spellbook keeps olifant's `generator.toml` keys and its error sentences,
and replaces olifant's tab stops with **holes**: placeholders that parse and say what belongs
there, so a person, an agent or conjure can fill them.

Spellbook depends on `sorcerer-core` and `sorcerer-lsp` only. It makes no model call.

## The modules

`domain/` (data and rules, no I/O):

- `config.rs`: `SpellbookConfig { enabled, exclude }`, `spellbook_config`, `PRODUCT`.
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
  inputs; every edit's template exists; paths, derives and derived holes use known names and no
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
  whole word on its line or within `NEARBY_LINES` (20).

`ports/`: `generators.rs` (`Generators`: `all`, `named`), `grammars.rs` (`Grammars::for_path`:
the loaded language of a path, or none), `workspace.rs` (`Workspace::write_new`, `WriteRefused`),
`generation_store.rs`, `holes_store.rs`, `clock.rs`.

`usecases/`: `list_generators.rs` (`GeneratorSummary`, also the editor's picker reply),
`choices_for.rs` (`resolve_inputs`, `choices_for`), `plan_generation.rs` (`PlanDeps`,
`plan_generation`), `apply_generation.rs` (`ApplyDeps`, `apply_generation`,
`AppliedGeneration`), `open_holes.rs` (`HolesDeps`, `open_holes`).

`adapters/`: `generator_dir.rs` (`GENERATORS_DIR`, `generators_in`, `generator_named`,
`load_generator`, `GeneratorDirs`), `cached_grammars.rs` (`CachedGrammars` over
`Languages::for_path` and `Languages::load`), `disk_workspace.rs`, `jsonl_generation_store.rs`,
`json_holes.rs` (core's `Store` at `spellbook/holes.json`), `system_clock.rs`, `wiring.rs`
(`Wired`, `live`, `wired`, and `plan_deps`/`apply_deps`/`holes_deps`).

`adapters/lsp/`: the server, below.

`cli/`: `arguments.rs`, `run_cli.rs` (`main`, `main_with(args, cwd, env, wire, out, err)`),
one file a command (`list.rs`, `show.rs`, `new.rs`, `holes.rs`, `doctor.rs`, `trust.rs`, `lsp.rs`),
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
   `still_open` finds it; `holes.json` is rewritten with what is still open.

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
  | `spellbook.plan` | generator, inputs | the `Plan` as JSON (`id`, `files[{path,status,text,lines}]`, `holes[{name,kind,instruction,source,places[{file,line}]}]`), or `{ stop }` (a missing input adds `input` and `choices`) |
  | `spellbook.apply` | plan id | `{ written, skipped, refused: null or {path, why}, holes, first: null or {file, line} }`, or `{ stop }` for a plan that is gone |

  A hole's location is `file` everywhere (it is `HolePlace::file`, and `holes.json`'s key);
  `path` is a planned or written file.
- `plans.rs`: `PlanCache`, the plans made and not applied, dropped after ten minutes by the
  clock; `take` removes the plan, so a plan is applied once.
- `marks.rs` (`hole_mark`: the whole line, Information, source `spellbook`, code `hole`, message
  `<name> (<kind>): <instruction>`, `data` the generation id), `diagnostics.rs`
  (`file_diagnostics`: `open_holes` for one file, so the same `still_open` search as the command
  line), `project_paths.rs` (`project_relative`: which editor paths are the project's).

After an apply the server publishes each written file's marks and asks the editor to show the
first file at its first hole (`Client::show_document`). Spellbook does not know conjure ran: it
sees the placeholders gone on the next save. The editor side is sorcerer.nvim's
`lua/sorcerer/spellbook.lua` (`M.new`, one chain of callbacks: generators, choices, plan, the
float in `spellbook_plan.lua`, apply, then conjure's `conjure.conjure [uri, line]` on the first
hole when `conjure_holes` is set); it knows only the replies above.

## Why a grammars port

`scripts/layers` refuses `sorcerer_core::adapters` in `usecases/`, and loading a grammar
(`Languages::load`) is core adapter work (it downloads or builds into the cache). So the use
cases ask `Grammars::for_path`, and `CachedGrammars` answers it. `Loaded` is a core domain type,
so the port may return it.

## The tests

The tests live in `crates/spellbook/tests/`, because they name languages
(`scripts/no-language-names`). `support/bistro_fixtures.rs` copies conjure's Python corpus base
(`corpus/conjure/bistro/base`) and writes a `command` generator (olifant's python-htmx
`command`, ported) from string constants; `support/project_fixtures.rs` points the grammars at
core's vendored sources, as conjure's tests do, so no test touches the network.
`lsp.rs` drives the server over an in-memory connection with a small editor (the handshake, the
four commands, the marks on open, save and close, trust); `generators.rs` covers `[holes]` and the loader's sentences; `plan.rs` inputs, rendering and
planning; `apply.rs` writing, the records and the open holes (a link out of the project made
with core's `link_to`); `cli.rs` each command through `main_with` with a wiring that uses the
vendored grammars and a fixed clock.
