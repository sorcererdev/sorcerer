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

- `conjure_change/request_and_result.rs`: `ChangeRequest`, `ChangeDeps`,
  `ConjuredChange` (with `unit_lines`, where a change of no files is marked), `ChangedFile`,
  `ChangeRefused` (with the pinned sentences in its `text`; `NotAnswered` carries the unit and
  its facts so the server can record a call that failed).
- `conjure_change/run_conjure.rs`: `conjure_change(request, deps)` and `refuse_a_spent_day`. Its
  story: `find_site` (`find_site.rs`, with `Site`: the unit at the cursor, through
  `CodebaseView::units` and `facts`), `refuse_a_spent_day`, the situation, `written_and_checked`
  (one write, the check, the rules it breaks, as an `Answer`), then `within_the_rules`: an answer
  that breaks none is kept; else, when the day allows, one more write on the situation plus
  `broken_rules_text` (each break with its line, from `line_as_written`), the two errands added by
  `both_calls`; a second break is `held_back` (no files, `held_back_text` as the goal). A retry
  that keeps the rules carries `drops_of_retry` on `ConjuredChange::retry_drops`.
- `conjure_change/retry_drops.rs`: `drops_of_retry(TwoAnswers, deps)`: `names_of_the_change`
  reads each touched file before, after the first answer and after the retry
  (`ProjectContext::names_in`), `dropped_by_retry` finds what the first added and the retry lost,
  and `verdicts_of` asks `ChangeDeps::decider` (Jev; `None` gives `DropVerdict::NotChecked`) with
  `drop_questions` over `retry_asked` (the broken rules, the retry's goal, both answers as unified
  diffs). Never holds a change back.
- `conjure_change/rules_check.rs`: `rules_broken_by(files, deps)`: the new texts through
  `ChangeDeps::checker`; only when they have an error, the old texts too; `rules_broken` between
  them. Texts that cannot be checked break nothing.
- `conjure_change/situation.rs`: `situation_for` gathers the test file (`tests_path`), the
  related files (`related_with_room`: `closest_first` over the unit's text, then `chosen_within`
  the budget), the session (`session_of`: `changed_since("HEAD")`, each diff, new files), scry's
  notes and the finding asked for, and the rules, then `situation_text`.
- `conjure_change/write.rs`: `write_once`, the one call to the writer.
- `conjure_change/check.rs`: `checked_files`: each written path through `landing_of`
  (`lexical_relative`, `Fences::allows`, the `exclude` globs), the base text from the view, the
  new text in the base's line endings, `line_edits`, and the parse check.
- `report_cost.rs` (`report_cost`, `DayCost`), `show_log.rs` (`last_changes`).

**Domain** (`src/domain/`), no I/O:

- `config.rs`: `ConjureConfig`, `conjure_config`, `PRODUCT`.
- `language_tables.rs` (its path templates filled with core's `templates::fill`, `{= name =}`): `TestTable`, `ImportsTable`,
  `test_table`, `imports_table`, `tests_path`: the language file's `[test]` and `[imports]`,
  read strictly from `Language.rest`.
- `change_prompt.rs`: `CHANGE_SYSTEM`. `situation.rs`: `Situation`, `RelatedFile` (path, whole
  text, `closeness`, the `names` it defines), `situation_text` (`left_for_room_lines` names the
  related files left out for room; `generated_regions_lines` adds core's `GENERATED_REGIONS_LINE`
  when the file, its test file or a related file holds a `spellbook:region`), `session_text`, and the caps (`RELATED_FILES` 8,
  `RELATED_FILES_BESIDE` 4 and `RELATED_FILES_BESIDE_IN_ALL` 6, `SESSION_LINES` 300,
  `NEW_FILE_NAMES` 20).
- `context_choice.rs`: choosing related files whole under a step's budget:
  `CONTEXT_BUDGET_TOKENS` (6,000), `tokens_of` (a token for every 4 bytes), `Closeness`
  (`UsedByTheUnit`, `Imported`, `BesideAnImport`, `DeclaringAnImport`), `closest_first` (an import
  whose names the unit uses moves up), `chosen_within` (each file kept whole while it fits, else
  left out whole: `Chosen { kept, left_for_room }`), `mentions_word`. Why a fixed order and not
  a model's pick: measured in `notes/durandal/conjure-context-choice-report-2026-11-27.md` (Jev
  picking the files dropped `ports/mod.rs` on the add-port step and said "no change" 6 of 8 times).
- `scry_notes.rs`: conjure's own reading of scry's findings file: `StoredFinding`,
  `StoredFindings` (`Versioned`, version 2), `FindingsRead`, `notes_near`, `ScryNote`.
  `rules.rs`: `rules_text` over `context`, `include` and `[[rule]] text` of `.sorcerer/scry.toml`
  (`RULES_CAP` 3000). Products never import each other: these are conjure's readers.
- `reply.rs`: `read_reply`, `ChangeReply` (the same path twice keeps the last block; `is_nothing`).
- `edits.rs`: `lexical_relative`, `Landing`, `DropReason`, `NameCase`, and a re-export of the
  host's `sorcerer_lsp::line_edits` (`in_base_line_endings`, `line_edits`, `LineEdit`,
  `first_changed_line`, `lines_changed`), which moved there for apprentice
  ([The language-server host](lsp.md#what-the-host-owns)).
- `spellbook_findings.rs`: conjure's reading of `spellbook check --json`: `RuleFinding`,
  `RulesRead` (`Checked`, `NotChecked(why)`), `RulesOutcome` (`Kept`, `AskedAgain`, `HeldBack`, on
  `ConjuredChange::rules`), `findings_of_check_json`, `rules_broken` (errors in the new texts that
  no old finding of the same rule and fingerprint excuses, one for one), `any_error`,
  `broken_rules_text`, `held_back_text`.
- `retry_drops.rs`: `NamedInText` (`NameKind::Call` or `Definition`, line, column), `FileNames`,
  `NamesOfTheChange` (before, first, retry: the same files three ways), `dropped_by_retry` (names
  the first answer has more of than the files before and than the retry, over the whole change,
  so a move between files is no drop; `one_per_line` keeps the earliest, then shortest, name per
  line), `drop_questions` (one `Choice` per drop, `rule_asked_for_removal`, `moved_elsewhere`,
  `behaviour_lost`, at most `MAX_DROPS_ASKED` 5, shared context `goal`, `rules`, `first_answer`,
  `retry` and its own `dropped-N`, so all go as one Jev call), `verdict_of` (the mass on the two
  legitimate options summed, as the guard pattern does: at least 0.7 `Legitimate(WhyRemoved)`, at
  most 0.3 `LooksUnintended`, else `Unsure`), `CheckedDrop`, `retry_step_text`,
  `said_drops_text`, `quiet_drops_text`.
- `drop_notes.rs`: `DropNotes` (`Versioned`, version 1) of `DropNote`s (change id, time,
  `RecordedDrop`): `with_change` adds a change's said drops and keeps the newest `MAX_NOTES` 20,
  `without_change` takes them back. scry reads the file with its own reader.
- `record.rs`: `ChangeRecord` (`retry_drops: Vec<RecordedDrop>`, left out of the line when empty, so
  the record stays version 1), `Choice`, `Attempt`, `Trigger`, `change_id`. `budget.rs`:
  `day_allows`. `log_text.rs`: `record_line`, `usd`, `count`.

**Ports** (`src/ports/`): `writer.rs` (`Writer`, `Written`), `rule_checker.rs` (`RuleChecker`:
`findings_in(texts)`, `describe` for the doctor), `change_store.rs` (`ChangeStore`),
`clock.rs` (`Clock`), `project_context.rs` (`ProjectContext`: related files (`RelatedFiles`: the files and those left
out by a fence), parse errors, the calls and definitions in a text (`names_in`), scry's findings, the rules; the things that need grammars or scry's files, so a use case
asks a port and never touches an adapter). Jev is sorcerer-models' `Decider` port, on
`ChangeDeps::decider`.

**Adapters** (`src/adapters/`): `provider_writer.rs` (`ProviderWriter`: one `Provider::complete`
and its errand), `research_writer.rs` (`ResearchWriter`: Try harder through a `Researcher`),
`imports.rs` (`related_files`: `imported_project_files` from `imports.scm` and `[imports]`, then `with_nearby_files`: after each import, through `NearbyFiles`, the files in its folder of the same language whose text names one of its units (`beside`: `names_defined_in`, `mentions_word`; how a port brings its adapters), then the first file one folder up whose text names the import's folder (`declaring`: where ports are declared), all language-neutral; fenced ones are not read; each file whole, tagged with its `Closeness` and its `names_defined_in`), `scry_files.rs`
(`read_findings`), `steering_files.rs` (`project_rules`), `spellbook_checker.rs`
(`SpellbookChecker::for_project`: the `spellbook` beside the running program, else on PATH, none
without `.sorcerer/spellbook/rules.toml`; `findings_in` runs `spellbook check --texts - --json` in
the root with the texts as JSON on its input, exit 0 or 1 read, anything else `NotChecked` with
its first error line; both calls go through `run_within`, which waits `CHECK_TIMEOUT` (10 s,
`domain/spellbook_findings.rs`) and kills a program still running: `RulesRead::TimedOut`. `rules_broken_by`
then returns `Checked { timed_out }`, `written_and_checked` ends the goal with `TIMED_OUT_NOTE` and
sets `RulesOutcome::NotChecked`, and the answer is used unchecked (fail open; `tests/spellbook_timeout.rs`,
the `Hanging` double in `tests/rules_check.rs`); `rules_for` runs `spellbook rules --json FILE` and reads `RuleNote`s,
none on any failure), `project_context.rs` (`FilesContext`,
the live `ProjectContext`), `names_in_text.rs` (`names_in`: each `@reference.call` named by
`callee_of`, its text before its own `@arguments` (to its `@name` without one) with inner argument
lists cut to their brackets, and each unit from `units_of`; no language named),
`drop_notes_file.rs` (`note_drops`, `forget_drops` on `.sorcerer/state/conjure/drops.json`
through core's `Store`), `jsonl_change_store.rs`, `system_clock.rs`, `wiring.rs` (`Wired`,
`live`, `writer_for`, `decider_for`: Jev from `JEV_API_KEY`, recorded under `conjure`, verb
`decide`), `editor_view.rs` (`EditorView`: the editor's open text over `FilesView`, a picture of one
moment: a file not open is read from the disk once and answered as first read, so a change is
made against what the writer was shown and a disk that moved on meanwhile is seen),
`why_text.rs` (`why_text`, Show why's file), and `lsp/` (the server, below).

**Command line** (`src/cli/`): `arguments.rs` (the clap types), `run_cli.rs` (`main`, `main_with`,
`run_command`), `console.rs`, `run.rs`
(`conjure FILE:LINE`), `doctor.rs`, `cost.rs`, `log.rs`, `trust.rs`, `lsp.rs` (`conjure lsp`:
`serve` over stdio with `ServerConfig::live`), `format.rs`.

## One change, end to end

`conjure src/cart.py:12` becomes `Place::parse` and `project_relative` in `cli/run.rs`, a
`ChangeRequest`, and `conjure_change`:

1. **The site.** The language by `Languages::for_path`; `view.unit_or_broken_region_at` on the
   line (a selection's first line): the unit from `unit_at`, else, when the parser lost the unit,
   the innermost tree-sitter ERROR region holding the line (`broken_region_at`, in
   `sorcerer-core/src/domain/broken_region.rs`), a `Unit` of kind `UnitKind::BrokenRegion`. Its
   situation (`where_the_cursor_is` in `domain/situation.rs`, from `Situation.broken_region`) says
   the code does not parse and is to be repaired. No language, or no unit and no region, is
   `ChangeRefused::OutsideAUnit`. The code action (`in_a_unit`) and the working text
   (`show_writing`) ask the same view method, and so does `fingerprint_now` for scry's notes.
2. **The day.** `spent_since` over the ledger's `conjure` errands, the user's local day;
   `day_allows` refuses a first call at or over `budget_per_day`, and Try harder also when
   its $0.25 would pass it.
3. **The situation** (unless the request carries one, as Try harder does): `situation_for` ends
   with `rules_for_the_file_text(deps.checker.rules_for(file))` (`domain/spellbook_findings.rs`):
   the rules of the file up front, from `spellbook rules FILE --json` (`RuleChecker::rules_for`,
   empty by default and when spellbook cannot say). Then `Writer::write` once. `ProviderWriter` makes one plain call (`max_tokens` 8192, 120 s); `ResearchWriter` is a
   researcher with Read, Grep and Glob, $0.25 and 300 s. Both record their errand in the ledger,
   a failed call too.
4. **The check.** `read_reply`, then for each file `landing_of`: a path leaving the project
   (`OutsideProject`), under a fence or through a link out (`Fenced`, by `Fences::allows`, which
   resolves symlinks), or matching `exclude` (`Excluded`) is dropped and named; a path whose
   current text cannot be read (`NotText`) is dropped too. The rest become `ChangedFile`s: the
   writer's text in the base's line endings, `line_edits` against the base, `parse_errors`
   from the `ProjectContext`. A file equal to its base is left out; no files is "no change".

5. **The rules.** The call after a break is `Writer::write_retry` (default: `write`), which
   `ProviderWriter` and `ResearchWriter` record under the verb `conjure-retry`
   (`write_as`); `report_cost` counts those in `DayCost::retries`, apart from `changes`.
   `rules_broken_by` asks the `RuleChecker` (spellbook, live) about the files'
   new texts and, when they have an error, their old ones; an error the old texts did not have is
   a break. `within_the_rules` keeps an answer with none, asks once more with the breaks named
   (`RulesOutcome::AskedAgain`), and holds back a second break (`RulesOutcome::HeldBack`: no
   files, so the server shows and records it as no change; its goal names the rules). Products do
   not link each other, so the seam is spellbook's program and its JSON, read by conjure's own
   `spellbook_findings.rs`.
6. **The drops.** After a retry that keeps the rules, `drops_of_retry` compares the two answers
   and asks Jev about each drop. The command line prints `retry_drops_text`; the server's
   `step_text` adds `LandedChange::retry_step` (`retry_step_text`: ` · asked again for <rules> ·
   dropped: …`, legitimate drops left out), `main_text` on the mark adds every drop
   (`drops_text`), `record_of` writes them to the record, and `confirm_landed` calls `note_drops`
   for scry; `Server::record` calls `forget_drops` at `Undone` and `Replaced`. `ServerConfig::decider`
   (a `DeciderFor`) builds Jev per project, as `checker` builds the rule checker.

The command line prints the change and stops. The editor's server lands `ChangedFile::edits`
through `workspace/applyEdit` and records `ChangeRecord`s; neither the use case, the command line
nor the server writes to the person's files (the editor applies the edit; the server writes only
under `.sorcerer/state/`).

## The server

`adapters/lsp/conjure_server.rs`: `serve(connection, ServerConfig)` plugs `ConjureServer` into the host
(`sorcerer_lsp::serve`) as a product that tracks text (`tracks_text() -> true`, so the host keeps
each open document's version and text). `ServerConfig { open, cache, trust, clock, wire, checker }`;
`wire: WireWriter` builds an attempt's writer over the editor's view (`writer_for` live, a
scripted writer in the tests); `checker: CheckerFor` builds the project's `RuleChecker`
(`SpellbookChecker` live, `NotChecking` in the tests). `ServerConfig` is in `server_config.rs`. The shared `Server` (`server_state.rs`) holds the `Client`, the root, `published` (the
`Mark`s published per editor path, for the actions on a line), `shown` (the one change marked
now, a `Shown { change: LandedChange, state: ShownState, settled }`) and `running` (the `Cancel`
of the change being conjured or undone; one at a time). `ShownState` is `Open` (Undo and Try
harder offered), `Undoing(choice)` (an undo waits for the editor; its own edits are not the
person's) or `Recorded` (the record is written: no change, landed in part, or kept by a save;
`LandedChange::note` says why on the marks, and only the Show actions are offered). `settled` lists
the files whose marks are gone for good. `project_relative` and `project_file` (`project_paths.rs`) map an editor path to the
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
   (`unit_or_broken_region_at`) gets `⟳ conjure: writing…` through `Client::show_working` (`sorcerer/working`).
   `ended`, and the error branch of `conjure_and_land`, call `clear_working` with the progress
   line, so it stays until the edit is answered.
2. **Conjured.** `conjured` opens the project, checks trust (`needs_trust`, the user's
   `TrustList`), maps the open documents into a `landing::Snapshot` (`snapshot_of`), builds an
   `EditorView` over them, the writer (`cfg.wire`), the ledger, the fences and a `FilesContext`
   over the same view, and runs `conjure_change`. A refusal is its sentence; `NotAnswered` is
   also recorded `failed` (`failed_record`).
3. **Landed.** `land`: no files is shown at once as no change and recorded `no-change`. The step
   ends with one `window/showMessage` (Info) from `step_text` in `changes.rs` (`changed_text`, naming each file by `names_told_apart`, which
the mark's *Show `<file>`* titles use too (`other_files` in `commands.rs`),
   `no_change_text`), sent for no change and, in `confirm_landed`, for a change that landed whole;
   refusals and partial landings send their own Warning instead. Otherwise
   `moved_on` refuses to send when a file the change edits moved on since it was read (an open
   file at another version; a file not open then and open now with other text, or on disk with
   other text than the change read; a new file that is there now), since an editor may not
   check (Neovim checks only the first file's version) and a file with no version is not checked
   at all (`sorcerer_lsp::landing::moved_on`, over the change's files as `FileChange`s from
   `landing::file_changes`); then the host's `landing::workspace_edit` builds one
   `WorkspaceEdit` (each existing file a `TextDocumentEdit` at the snapshot's version, `None` for
   a file that was not open; each new file a `CreateFile` with `ignoreIfExists` and its text over the whole new buffer;
   new files left out under `NewFiles::LeftOut`), and `Client::apply_tool_edit` sends it (noting its lines as a tool's for apprentice; see the host chapter). On
   `applied`, `confirm_landed` (on a thread) waits up to 3 s for each file to read as its
   after-text (`files_not_as_sent`; an editor may answer before it reports, and may skip a file
   and still say applied): all as sent, `show` makes it the shown change (`Open`; the one before
   is recorded `kept` by `end_with`) and `publish_marks` replaces every mark; some not, it is
   said, recorded `landed-in-part` and shown `Recorded` with the note; none, it is said and
   recorded `dropped`. Both messages name the files by `names_told_apart` over all the change's
   files. On a refusal the person is told and it is recorded `dropped`. `ended` ends
   the progress line and lets `running` go.
4. **Marked.** `marks.rs`: `marks_of` (the main mark on the asked file, else the first changed
   file; an `Other` mark per other file; a `NoChange` mark on the unit's first line) and
   `mark_diagnostic` (source `conjure`, Information, code `change` or `no-change`, `data` the
   change id, the whole line; the words in `main_text`, which opens with `done_text` (`conjured: ` and the goal with its first letter lowered by `lowered_first_letter`), `other_text`, `no_change_text`).
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
refused with the file named (the host's `sorcerer_lsp::undo_plan::undo_of`, each `LandedFile` as a
`TakenBack`). While the undo is in flight the state is `Undoing(choice)`;
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
copies, so a change goes into both. Conjure-only files: the `ctrl+m c/u/t/w` chords (`KEYS` and
the pure `actionTitled` in `src/keys.ts`; `registerKeys` in `src/keyActions.ts` asks
`vscode.executeCodeActionProvider` and runs the action whose title the server gave, the way the
lightbulb does; the chords are in `package.json` `contributes.keybindings`, and a unit test pins
them to `KEYS`), and the gutter icon (`conjuredLines` in `src/marks.ts` picks the diagnostics of
source `conjure` and code `change`; `registerConjuredMarks` in `src/conjuredMarks.ts` draws
`media/conjured.svg`). The action titles are the server's (`actions_for` in
`adapters/lsp/commands.rs`): rename one there and in `KEYS`. `conjure.path` is `"scope": "machine"` and restricted in an
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
sent" (`confirm_landed`). A file the change creates is attached too though it is not on disk:
Neovim's `create_file` (`runtime/lua/vim/lsp/util.lua`) writes nothing for a `CreateFile` with
`ignoreIfExists`, only `bufadd`s the name, so `M.attach` skips a name with another scheme
(`fugitive://…`) rather than a name not on disk. The same goes for apprentice's `M.attach`. `status()` of the plugin joins scry's and conjure's progress lines. `lua/sorcerer/working.lua`
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
doubles), `tests/rules_check.rs` (asked again, held back, an old break, a warning, over
`ForbiddenWord`, a `RuleChecker` double in `tests/support/rule_checkers.rs` that finds each line
holding a word), `tests/retry_drops.rs` (a dropped call, a call moved to another file, a
legitimate drop said quietly, unsure and no-Jev verdicts, callee names over the vendored
grammar; `SameAnswer`, a `Decider` double in `tests/support/deciders.rs`), `tests/eval.rs` (the corpora, below). `UPDATE_GOLDEN=1` rewrites the golden prompt. No test calls a model or the network (the eval replays recordings).

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

