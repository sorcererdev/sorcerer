# Conjure

`crates/conjure`: writes one small change where the person asks. Package `sorcerer-conjure`,
library `conjure`, binary `conjure`. This chapter is the engine and the command line; the editor
side (landing a change through `workspace/applyEdit`, the marks and their actions) is the
language server that the next part adds under `adapters/lsp/`.

## Where conjure came from

The engine is olifant's measured step (`src/Olifant/Server/Step.hs` in the olifant repository)
ported onto sorcerer's layers with no language named. The system prompt in
`domain/change_prompt.rs` is olifant's `stepSystem` word for word, on one line; the situation's
layout in `domain/situation.rs` is its `situationText` line by line. The model reads "one small
step" because that is what was measured; nothing the person sees says "step": the verb is
"conjure" and the noun is "change".

## Module map

Conjure is in five layers (see [Architecture](architecture.md)).

**Use cases** (`src/usecases/`):

- `conjure_change/mod.rs`: `conjure_change(request, deps)`, `ChangeRequest`, `ChangeDeps`,
  `ConjuredChange`, `ChangedFile`, `ChangeRefused` (with the pinned sentences in its `text`).
  Its story: `find_site` (the unit at the cursor, through `CodebaseView::units` and `facts`),
  `refuse_a_spent_day`, the situation, one write, the check.
- `conjure_change/situation.rs`: `situation_for` gathers the test file (`tests_path`), the
  related files, the session (`session_of`: `changed_since("HEAD")`, each diff, new files), scry's
  notes and the finding asked for, and the rules, then `situation_text`.
- `conjure_change/write.rs`: `write_once`, the one call to the writer.
- `conjure_change/check.rs`: `checked_files`: each written path through `landing_of`
  (`lexical_relative`, `Fences::allows`, the `exclude` globs), the base text from the view, the
  new text in the base's line endings, `line_edits`, and the parse check.
- `report_cost.rs` (`report_cost`, `DayCost`), `show_log.rs` (`last_changes`).

**Domain** (`src/domain/`), no I/O:

- `config.rs`: `ConjureConfig`, `conjure_config`, `PRODUCT`.
- `templates.rs`: `fill` (`{= name =}`). `language_tables.rs`: `TestTable`, `ImportsTable`,
  `test_table`, `imports_table`, `tests_path`: the language file's `[test]` and `[imports]`,
  read strictly from `Language.rest`.
- `change_prompt.rs`: `CHANGE_SYSTEM`. `situation.rs`: `Situation`, `situation_text`,
  `session_text`, and the caps (`RELATED_FILES` 8, `RELATED_LINES` 120, `SESSION_LINES` 300,
  `NEW_FILE_NAMES` 20).
- `scry_notes.rs`: conjure's own reading of scry's findings file: `StoredFinding`,
  `StoredFindings` (`Versioned`, version 2), `FindingsRead`, `notes_near`, `ScryNote`.
  `rules.rs`: `rules_text` over `context`, `include` and `[[rule]] text` of `.sorcerer/scry.toml`
  (`RULES_CAP` 3000). Products never import each other: these are conjure's readers.
- `reply.rs`: `read_reply`, `ChangeReply` (the same path twice keeps the last block; `is_nothing`).
- `edits.rs`: `lexical_relative`, `Landing`, `DropReason`, `in_base_line_endings`, `line_edits`
  (`LineEdit`, over the `similar` crate), `first_changed_line`, `lines_changed`.
- `record.rs`: `ChangeRecord`, `Choice`, `Attempt`, `Trigger`, `change_id`. `budget.rs`:
  `day_allows`. `log_text.rs`: `record_line`, `usd`, `count`.

**Ports** (`src/ports/`): `writer.rs` (`Writer`, `Written`), `change_store.rs` (`ChangeStore`),
`clock.rs` (`Clock`), `project_context.rs` (`ProjectContext`: related files (`RelatedFiles`: the files and those left
out by a fence), parse errors, scry's findings, the rules; the four things that need grammars or scry's files, so a use case
asks a port and never touches an adapter).

**Adapters** (`src/adapters/`): `provider_writer.rs` (`ProviderWriter`: one `Provider::complete`
and its errand), `research_writer.rs` (`ResearchWriter`: Try harder through a `Researcher`),
`imports.rs` (`related_files`, from `imports.scm` and `[imports]`), `scry_files.rs`
(`read_findings`), `steering_files.rs` (`project_rules`), `project_context.rs` (`FilesContext`,
the live `ProjectContext`), `jsonl_change_store.rs`, `system_clock.rs`, `wiring.rs` (`Wired`,
`live`, `writer_for`).

**Command line** (`src/cli/`): `mod.rs` (the clap types, `main`, `main_with`), `run.rs`
(`conjure FILE:LINE`), `doctor.rs`, `cost.rs`, `log.rs`, `trust.rs`, `format.rs`.

## One change, end to end

`conjure src/cart.py:12` becomes `Place::parse` and `project_relative` in `cli/run.rs`, a
`ChangeRequest`, and `conjure_change`:

1. **The site.** The language by `Languages::for_path`; `view.units` and `unit_at` on the line
   (a selection's first line); no language or no unit is `ChangeRefused::OutsideAUnit`.
2. **The day.** `spent_since` over the ledger's `conjure` errands, the user's local day;
   `day_allows` refuses a first call at or over `budget_per_day`, and Try harder also when
   its $0.25 would pass it.
3. **The situation** (unless the request carries one, as Try harder does), then `Writer::write`
   once. `ProviderWriter` makes one plain call (`max_tokens` 8192, 120 s); `ResearchWriter` is a
   researcher with Read, Grep and Glob, $0.25 and 300 s. Both record their errand in the ledger,
   a failed call too.
4. **The check.** `read_reply`, then for each file `landing_of`: a path leaving the project
   (`OutsideProject`), under a fence or through a link out (`Fenced`, by `Fences::allows`, which
   resolves symlinks), or matching `exclude` (`Excluded`) is dropped and named; a path whose
   current text cannot be read (`NotText`) is dropped too. The rest become `ChangedFile`s: the
   writer's text in the base's line endings, `line_edits` against the base, `parse_errors`
   from the `ProjectContext`. A file equal to its base is left out; no files is "no change".

The command line prints the change and stops. The editor's server will land `ChangedFile::edits`
through `workspace/applyEdit` and record `ChangeRecord`s; neither the use case nor the command
line writes to the project.

## What a model may be shown

Every file that enters the situation passes `Fences::allows`: the target (`find_site` refuses a
fenced one), the test file, each related file (`related_files` takes an `allowed` predicate, and
drops a template candidate that is absolute or climbs out with `..`), and each session path
(`session_of`). A fenced one is left out and named in `Situation::left_out`, which
`situation_text` prints as one line. Written paths are fenced again on the way out (`landing_of`),
`exclude` is matched on both the written and the resolved path, and `one_block_per_file` makes two
spellings of a path one change; both fold case only when `ChangeDeps::name_case` is
`NameCase::Ignored`, which the wiring asks of `platform::paths::names_ignore_case` (a read-only probe
of the project root).

## Related files, the test file and the session

None of them names a language. `tests_path` fills `[test] tests_for` from the file's folder and
name; `related_files` runs the language's `imports.scm` (captures `@import.path` only), splits
each captured path on the language's `unit_separator` (else `.`), drops the leading words in
`drop_first`, and tries each `[imports] resolve` template, then once more without the last
word. When a language's test file is the file itself (Rust), the situation says the tests live in
it instead of showing it twice. The session block reads the disk; the editor's unsaved text
reaches the writer through the file, its test file and the related files, which the view
supplies.

## Tests

`tests/change.rs` (the prompt against `tests/golden/change-prompt.txt`, the situation, the
use case over a `StubWriter` and a `Rig` of doubles, the writers over `ScriptedProvider`),
`tests/related.rs` (the vendored grammars), `tests/cli.rs` (`main_with` with a `Wired` of
doubles), `tests/eval.rs` (the corpora, below). `UPDATE_GOLDEN=1` rewrites the golden prompt. No test calls a model or the network (the eval replays recordings).

## The eval

`crates/conjure/tests/eval.rs` runs each corpus under `corpus/conjure/<name>/` through
`conjure_change` with the writer's reply replayed, and asserts the corpus's passing scores. One
`#[test]` a corpus (`bistro_reaches_its_passing_scores`, `bistro_rs_reaches_its_passing_scores`); it
costs nothing and runs in `cargo test --workspace` on all three systems. `corpus/conjure/README.md` is
the index; each corpus has its own README and a `cases.md`.

**A corpus** is `corpus.toml` (`name`, `language`, `writer`, `[passing_scores]`, `[baseline]`), `base/`
(the project, plain files), `cases/<id>.toml` (`file`, `line`, optional `end_line`, `kind`, `expect`,
optional `finding` and `also_in_scope`), `cases/<id>.patch` (the person's uncommitted edit, applied
with `git apply` over the committed base; absent for a green case), optional
`cases/<id>.findings.json` (a scry version 2 findings file, copied to
`.sorcerer/state/scry/findings.json`, which `.git/info/exclude` hides from the session diff),
`replies/<id>.json` (`{model, text, cost_usd, seconds}`) and `labels.json` (`{"forward": [ids]}`).
`run_case` builds the project in a temporary directory (`project_of_case`), then calls `conjure_change`
over the vendored grammars with a `CorpusWriter` that replays the reply.

**The four scores**, printed as a table with `--nocapture` and asserted by `Scores::assert_reaches`:
`forward` (the id is in `labels.json`, which a person writes after reading the recorded change against
`expect`), `parses` (no changed file has a parse error), `in_scope` (every changed path is the unit's
file, its test file by `tests_path`, or in `also_in_scope`) and `no_change_on_green` (a green case
answered "no change", or changed only its test file). A corpus with no `forward_min` fails after its
table.

**Recording.** `CONJURE_RECORD=all` (refused when `CI` is set: `record_mode_from`) asks the live
`ProviderWriter` for each case that has no `replies/<id>.json`, through the project's default provider
(`claude -p` on the subscription), and writes it. It prints a running total and stops once the total
reaches `CONJURE_RECORD_CEILING` dollars (default 0.50), keeping what it wrote; delete a reply to
record that case again. `CONJURE_EVAL_CASES=a,b` runs only those cases and asserts nothing.

**Adding a case.** Write `cases/<id>.toml` and its patch (a scratch copy of `base/`, the edit,
`git diff --cached`), record it with `CONJURE_RECORD=all cargo test -p sorcerer-conjure --test eval
<corpus> -- --nocapture`, read the reply against `expect`, add the id to `labels.json` when it is
forward and a line to `cases.md`. A real record from `changes.jsonl` becomes a case the same way:
its file, line and trigger become the toml, the person's diff at that time the patch.

**Passing scores** are stated before a corpus is first recorded and move only by a ruling written in
its `corpus.toml` with the date and the numbers; bistro-rs's `forward_min` was set from its first
recording (hits minus one). Replies are never converted (`.gitattributes`: `corpus/conjure/*/replies/**
-text`), so what was recorded is what CI replays.

