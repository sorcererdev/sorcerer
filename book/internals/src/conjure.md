# Conjure

`crates/conjure`: writes one small change where the person asks. Package `sorcerer-conjure`,
library `conjure`, binary `conjure`. This chapter is the engine, the command line and the
language server (`adapters/lsp/`), which lands a change through `workspace/applyEdit` and marks
it ([The server](#the-server)).

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
  `ConjuredChange` (with `unit_lines`, where a change of no files is marked), `ChangedFile`,
  `ChangeRefused` (with the pinned sentences in its `text`; `NotAnswered` carries the unit and
  its facts so the server can record a call that failed).
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
`live`, `writer_for`), `editor_view.rs` (`EditorView`: the editor's open text over `FilesView`, a picture of one
moment: a file not open is read from the disk once and answered as first read, so a change is
made against what the writer was shown and a disk that moved on meanwhile is seen),
`why_text.rs` (`why_text`, Show why's file), and `lsp/` (the server, below).

**Command line** (`src/cli/`): `mod.rs` (the clap types, `main`, `main_with`), `run.rs`
(`conjure FILE:LINE`), `doctor.rs`, `cost.rs`, `log.rs`, `trust.rs`, `lsp.rs` (`conjure lsp`:
`serve` over stdio with `ServerConfig::live`), `format.rs`.

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

The command line prints the change and stops. The editor's server lands `ChangedFile::edits`
through `workspace/applyEdit` and records `ChangeRecord`s; neither the use case, the command line
nor the server writes to the person's files (the editor applies the edit; the server writes only
under `.sorcerer/state/`).

## The server

`adapters/lsp/mod.rs`: `serve(connection, ServerConfig)` plugs `ConjureServer` into the host
(`sorcerer_lsp::serve`) as a product that tracks text (`tracks_text() -> true`, so the host keeps
each open document's version and text). `ServerConfig { open, cache, trust, clock, wire }`;
`wire: WireWriter` builds an attempt's writer over the editor's view (`writer_for` live, a
scripted writer in the tests). The shared `Server` holds the `Client`, the root, `published` (the
`Mark`s published per editor path, for the actions on a line), `shown` (the one change marked
now, a `Shown { change: LandedChange, state: ShownState, settled }`) and `running` (the `Cancel`
of the change being conjured or undone; one at a time). `ShownState` is `Open` (Undo and Try
harder offered), `Undoing(choice)` (an undo waits for the editor; its own edits are not the
person's) or `Recorded` (the record is written: no change, landed in part, or kept by a save;
`LandedChange::note` says why on the marks, and only the Show actions are offered). `settled` lists
the files whose marks are gone for good. `project_relative` and `project_file` map an editor path to the
project's `/`-separated path on the canonical path, as scry's server does.

`commands.rs`: `capabilities` (code actions; the commands `conjure.conjure`, `conjure.undo`,
`conjure.tryHarder`, `conjure.why`, `conjure.show`). `code_actions` offers nothing when
`Client::can_apply_edits` is false (`opened` says so once, `say_once`); otherwise the marked
change's actions (`on_the_line`: by the change id in the editor's `conjure` diagnostics, else by
line; `offers_for` from the shown state; `actions_for` in the pinned order), *Conjure here* when
the line is in a unit (`in_a_unit` parses the editor's text through an `EditorView`; in an
untrusted project nothing is parsed, so a grammar of the project's own is never built or loaded
before trust, and *Conjure here* is offered on any line of a file with a language), and *Conjure for this finding* per open or
waiting scry finding there (`findings_on`, scry's findings file read afresh). `lines_of` turns
the request's range into the line and last line, from 1. `execute` runs a command; what the
person must hear goes through `Client::say`, and an error answers only a malformed command.

`changes.rs`, a change's life:

1. **Asked.** `conjure` calls `start`, which takes `running` (`begin_running`; busy is said) and
   reads `open_documents` (an `OpenDocuments`) on the main loop, so the edit is made at the
   versions the person saw when they asked. A thread runs `conjure_and_land` under `work_done("conjure", "conjuring a
   change")`.
   `conjured` also calls `show_writing` once the view is built: the unit holding the cursor
   (`unit_at`) gets `⟳ conjure: writing…` through `Client::show_working` (`sorcerer/working`).
   `ended`, and the error branch of `conjure_and_land`, call `clear_working` with the progress
   line, so it stays until the edit is answered.
2. **Conjured.** `conjured` opens the project, checks trust (`needs_trust`, the user's
   `TrustList`), maps the open documents into a `landing::Snapshot` (`snapshot_of`), builds an
   `EditorView` over them, the writer (`cfg.wire`), the ledger, the fences and a `FilesContext`
   over the same view, and runs `conjure_change`. A refusal is its sentence; `NotAnswered` is
   also recorded `failed` (`failed_record`).
3. **Landed.** `land`: no files is shown at once as no change and recorded `no-change`. Otherwise
   `moved_on` refuses to send when a file the change edits moved on since it was read (an open
   file at another version; a file not open then and open now with other text, or on disk with
   other text than the change read; a new file that is there now), since an editor may not
   check (Neovim checks only the first file's version) and a file with no version is not checked
   at all; then `landing::workspace_edit` builds one
   `WorkspaceEdit` (each existing file a `TextDocumentEdit` at the snapshot's version, `None` for
   a file that was not open; each new file a `CreateFile` with `ignoreIfExists` and an insert;
   new files left out under `NewFiles::LeftOut`), and `Client::apply_edit` sends it. On
   `applied`, `confirm_landed` (on a thread) waits up to 3 s for each file to read as its
   after-text (`files_not_as_sent`; an editor may answer before it reports, and may skip a file
   and still say applied): all as sent, `show` makes it the shown change (`Open`; the one before
   is recorded `kept` by `end_with`) and `publish_marks` replaces every mark; some not, it is
   said, recorded `landed-in-part` and shown `Recorded` with the note; none, it is said and
   recorded `dropped`. On a refusal the person is told and it is recorded `dropped`. `ended` ends
   the progress line and lets `running` go.
4. **Marked.** `marks.rs`: `marks_of` (the main mark on the asked file, else the first changed
   file; an `Other` mark per other file; a `NoChange` mark on the unit's first line) and
   `mark_diagnostic` (source `conjure`, Information, code `change` or `no-change`, `data` the
   change id, the whole line; the words in `main_text`, `other_text`, `no_change_text`).
5. **Ended**, once, by `record` through `end_with` (an `Undoing` change records its undo's
   choice; a `Recorded` one nothing more): the first `saved` of a changed file is `kept`, turns
   the change `Recorded` with `KEPT_NOTE` and settles that file, and the others keep their marks
   until each is saved or `closed` (`settle`; the change goes when `all_settled`); `changed` sees
   the editor's own undo of an `Open` change (`all_back`: every file at its text before, a new file
   empty or gone) and records `undone`, and clears a no-change mark at any edit; `shutdown`
   records `unanswered` and cancels a running call. Before the change is recorded, `closed` only
   hides the file's marks; `republish` shows them again when it opens.

`undo.rs`: Undo (`undo` → `take_back`) takes `running` for the life of the undo (busy while a
change is conjured, and nothing lands meanwhile) and plans per file with `undo_of` from its text
now (`versioned_text_now`: the editor's, else the disk's): a file at its after-text goes back with
`line_edits(after, before)` at its current version; a new file is deleted (`DeleteFile`, under
`FileDeletion::Allowed`) or emptied; a new file closed unsaved (empty on disk) is deleted; a file
already at its text before needs nothing; any other text is the person's, so the whole undo is
refused with the file named. While the undo is in flight the state is `Undoing(choice)`;
`taken_back` records it when the editor applied it and runs its `Then` with the held `Cancel`. Try
harder (`try_harder`) is the same undo recorded `replaced`, then `start_again` with an `Again`
(the situation the first writer read, the id it replaces, and each file's text before), still
holding `running`: its thread waits up to 3 s (`documents_after_undo`) for the editor to report
the undo, because some editors (Neovim) answer `applyEdit` before they send the `didChange`, and
refuses when it never comes. `show_why` writes `why_text` to
`.sorcerer/state/conjure/why-<id>.md` and shows it; `show_file` shows a file with its changed
lines selected (`changed_lines`, in the new text).

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

## The editors

Both clients are thin: the server does everything, and each client starts `conjure lsp` per
project and makes the two things the server cannot make for itself true.

**VS Code** (`editors/vscode/conjure/`). `src/extension.ts` starts one `conjure lsp` per workspace
folder with VS Code's own language client, whose `documentSelector` is every file under the folder,
so a file a change opens is seen by the server. Finding the binary and downloading it are
`src/binary.ts`, `src/download.ts` and `src/offer.ts`, byte-identical copies of scry's
extension's, which take the product name as their first parameter (`candidates('conjure', …)`,
`assetFor('conjure', target)`, `install(base, 'conjure', …)`); CI's `vscode` job `diff`s the
copies, so a change goes into both. `conjure.path` is `"scope": "machine"` and restricted in an
untrusted workspace, as `scry.path` is. `test/e2e/run.ts` builds a project with the built `fake-claude`
copied as `claude` first on PATH (conjure picks its `claude` provider when one is on PATH) and
named as the provider's `runner` in the project's config (VS Code on macOS may put the login
shell's PATH, with the real `claude`, before the test's), trusts the project with the built `conjure trust` (config and cache
directories of the run's own: `XDG_*`, or `APPDATA` and `LOCALAPPDATA`), and launches VS Code;
`test/e2e/conjure.test.ts` runs *Conjure here*, sees the unsaved change in the buffer with the disk
unchanged, the new test file open and the mark, then *conjure: Undo*. Ubuntu and Windows run it.

**Neovim** (`sorcerer.nvim`, `lua/sorcerer/conjure.lua`; off by default, `conjure = { enabled =
true }`). `M.attach(buf)` starts `vim.lsp.start({ name = "conjure", cmd, root_dir, flags = {
debounce_text_changes = 0 } }, { bufnr = buf })` for every file buffer under a project root, on
`BufReadPost`, `BufNewFile`, `BufWritePost` and `BufEnter` and for the buffers already loaded.
The flag makes Neovim send each `didChange` at once: Neovim otherwise sends it up to 150 ms late
and `moved_on` could not see typing in that window. The attach to every buffer matters because
Neovim checks only the first file's version in a `WorkspaceEdit`, loads an unopened file for an
edit without conjure attached to it, and the server would then read that file as "did not land as
sent" (`confirm_landed`). `status()` of the plugin joins scry's and conjure's progress lines. `lua/sorcerer/working.lua`
draws `sorcerer/working` for both servers: `client_config()` adds `init_options = { drawsWorking =
true }`, the `sorcerer/working` handler and an `on_exit` that clears, to each `vim.lsp.start`;
`show` puts one `eol` extmark per unit in the `sorcerer_working` namespace (highlight
`SorcererWorking`, linked to `Comment`), so it follows edits, and skips a file with no loaded
buffer. VS Code: `editors/vscode/{scry,conjure}/src/working.ts` (`parseWorking`) and `inline.ts`
(`Working`, a `TextEditorDecorationType` with `after` text) are copies, as `binary.ts` is; the
extension sets `initializationOptions: { drawsWorking: true }`, clears on the client's `Stopped`
state, and `activate` returns `drawn()` and `everDrawn()` for the end-to-end tests.

## Tests

`tests/change.rs` (the prompt against `tests/golden/change-prompt.txt`, the situation, the
use case over a `StubWriter` and a `Rig` of doubles, the writers over `ScriptedProvider`),
`tests/editor_view.rs`, `tests/lsp.rs` (the server over `Connection::memory()`, driven by a
small editor that keeps buffers and applies the edits it is sent, checking versions, and can
answer before it reports its changes as Neovim does; a `Script` of replies, a gate to hold a
call, a clock that ticks a second per read), `tests/lsp_process.rs` (`conjure lsp` as a process),
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

