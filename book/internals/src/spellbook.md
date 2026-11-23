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

`cli/`: `arguments.rs`, `run_cli.rs` (`main`, `main_with(args, cwd, env, wire, out, err)`),
one file a command (`list.rs`, `show.rs`, `new.rs`, `holes.rs`, `doctor.rs`, `trust.rs`),
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
`generators.rs` covers `[holes]` and the loader's sentences; `plan.rs` inputs, rendering and
planning; `apply.rs` writing, the records and the open holes (a link out of the project made
with core's `link_to`); `cli.rs` each command through `main_with` with a wiring that uses the
vendored grammars and a fixed clock.
