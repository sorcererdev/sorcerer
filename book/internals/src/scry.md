# Scry

`crates/scry`: the code reviewer. Library `scry`, binary `scry`.

## Where scry came from

Scry is the successor of a code reviewer the author built and measured in another project.
Its review prompt, its judging rules (the placement, the same-problem and nitpick questions,
the merge table, the verification rounds and caps) and the eval's 58 bistro cases with their
recorded replies were carried over from that measurement, which is why changing a word a model
reads is checked against the eval rather than taken on taste. The rest of this chapter
describes scry as it is.

## Module map

Scry is in five layers (see [Architecture](architecture.md)); a dependency only points inward.

**Use cases** (`src/usecases/`), the API, one module a feature:

- `review_change/mod.rs`: `review_change`, `ReviewRequest`, `ReviewDeps`, `ReviewReport`,
  `ReviewOutcome`, `Progress`; the private `Pass` its steps share.
- `review_change/scope.rs`: `scope_of`; `ask.rs`: the review calls (`Pass::send`, `ReviewResults`);
  `judge.rs`: each finding placed, matched, merged or added (`Pass::judge_all`, `judge`,
  `merge`, `add`); `remember.rs`: resolved, gone, moved, not found again, the reviewed marks
  (`Pass::remember`, `sweep_gone`, `drop_new_findings_in_changed_units`, `mark_reviewed`, `ChangedUnits`, `NotFoundAgain`); `verify.rs`: the
  verification rounds (`Pass::verify_findings_per_file`, `verify_findings_in_file`, `verify_round`, `PrioritizedFindings`, `FindingsToVerify`, `DisplaySlots`) and `citations`, `cites`.
- `list_findings.rs` (`ListRequest`, `list_findings`), `explain_finding.rs` (`Explanation`,
  `CitedText`, `explain_finding`), `dismiss_finding.rs`, `report_cost.rs` (`CostReport`,
  `DayCost`), `show_findings.rs` (`Shown`, `shown_in_file`: what an editor shows in a file).

**Domain** (`src/domain/`), data and rules, no I/O:

- `config.rs`: `ScryConfig`, `MAX_SHOWN`, `scry_config`.
- `issue_type.rs`: `IssueType`, `Verify`, `ISSUE_TYPES`, `issue_type`.
- `finding.rs`: `Finding`, `FindingId`, `Status`, `Place`, `Evidence`, `Scores`,
  `StoredVerdict`, `StoredCitation`, `Dismissal` (`Here`, `Everywhere`), `status_word`.
- `findings.rs`: `Findings` (`new_id`, `get`, `find` (the same, or why not), `dismiss`, `shown_in`, `shown_in_scope`),
  `merge_into`, `renamed`.
- `steering.rs`: `Steering`, `Rule`, `Include`, `RawSteering`, `parse_steering`,
  `STEERING_CAP`.
- `scope.rs`: `ScopeRequest`, `Scope`, `ScopedUnit`, `ScopedFile`, `header_of`.
- `review_prompt.rs`: `ReviewUnit`, `ReviewAsk`, `RawFinding`, `ReviewReply`, `OpenAnswer` (its `holds` a
  `Holds::Still` or `Holds::NoLonger`), `review_prompt`,
  `parse_review`, `batch_units`, `batch_by_lines`, `REVIEW_TIMEOUT`.
- `judging.rs`: `place`, `candidates`, `same_problem`, `finding_text`, `SAME_THRESHOLD`,
  `Placement`, `Match`, `match_stored`, `nitpick`, `confidence_of`, `asked_on`, `cited_code`.
- `budget.rs`: `Caps`, `Budget`, `review_estimate`, `check_estimate`, `cost_of`.
- `verification.rs`: `needs_claim`, `claim_of`, `verdict_word`.
- `placing.rs`: `Placed`, `placed`: where a stored finding sits in its file now.
- `log_text.rs`: the run log's text: `section`, `sub`, `block`, `usd`, `count`, `citation`,
  `scope_text`.

**Ports** (`src/ports/`), the traits a use case talks through: `reviewer.rs` (`Reviewer`,
`Reviewed`), `claim_checker.rs` (`ClaimChecker`; a closure is one), `finding_store.rs`
(`FindingStore`, `Held`), `settings.rs` (`Settings`), `clock.rs` (`Clock`), `run_log.rs`
(`RunLog`, `Quiet`; a closure is one).

**Adapters** (`src/adapters/`): `json_finding_store.rs` (`JsonFindingStore`, the store file at
version 2 and its `pass.lock`), `scry_toml.rs` (`ScryToml`, `STEERING_FILE`, the settings from `.sorcerer/`),
`research_reviewer.rs` (`ResearchReviewer`), `markdown_run_log.rs` (`StreamingLog`,
`runs_dir`, `write_run`, `runs`, `run_key`, `run_time`, `result_of`, `KEEP`, `RESULT`),
`system_clock.rs` (`SystemClock`), `wiring.rs` (`LiveWiring`, `Wiring`, `jev_for`,
`researcher_for`, `claim_checker`, `PRODUCT`), and `lsp/`, the editor's server (below):
`mod.rs` (`ServerConfig`, `serve`, `SETTLE`, the product `ScryServer` and its shared `Server`),
`diagnostics.rs` (`diagnostic`, `STALE_LINE`), `passes.rs` (`Passes`, `RunningPass`, a save
to a pass).

**The command line** (`src/cli/`): `mod.rs` (`main`, `main_with`, the clap types, `run_command`,
`clock_of`, and `Console`: the output, the error stream and the width a command writes with), one file a command (`review.rs`, `list.rs`, `why.rs`, `dismiss.rs`, `cost.rs`, `lsp.rs`,
`doctor.rs` with `doctor_rows` (a `DoctorRow` each), `check_setup`, `languages_row`, `log.rs`), and `format.rs` (`Output`: text or `--json`; `table`, `cut_to`, `finding_text`,
`finding_json`, `cited_range`, `lines_text`). `src/main.rs` is the binary, `cli::main` with
the real environment.

Tests: `tests/support/mod.rs` holds git fixtures, `project_with_vendored_grammars`, the
doubles `StubReviewer`, `StubDecider`, `FixedClock`, `Doubles` (all of them as one `Wiring`),
`open_finding` and `put_findings` (a store seeded by hand), and `Ports` (the store, ledger and
settings adapters over a fixture project). `.sorcerer/scry.toml` at the repository root is
sorcerer's own steering.

## Config

`scry_config(&Project)` reads `[scry]` through core's `Config::table`, which is strict about
unknown keys (`deny_unknown_fields`), then range-checks: `confidence_threshold` in 0..=1,
`budget_per_pass` and `budget_per_day` not negative (nor NaN), and every entry of `issue_types`
a name in `ISSUE_TYPES`. The keys are `enabled`, `issue_types`, `review_model`,
`confidence_threshold`, `budget_per_pass`, `budget_per_day` and `exclude`; at most
`MAX_SHOWN` (3, a constant in `config.rs`) findings are shown in a file. Each failure is an `Error::Config` naming the key and `.sorcerer/config.toml`.
`ScryConfig::default()` holds the shipped values.

## Issue types

`ISSUE_TYPES` is the table of issue types: a `name`, the `sentence` the review prompt prints after it, whether
it is `shown` by default, and its `Verify` (`Citations`, or `CitationsThenClaim` where the
claim checker also runs). The sentences are those of the golden review prompt, word for word.
A new issue type is a new row. `[scry] issue_types` chooses which rows are shown.

## Steering

`domain/steering.rs` holds `Steering { context, includes, rules }` (`Include { path, text }`,
`Rule { id, text }`). `parse_steering(text, file)` reads `.sorcerer/scry.toml` into a
`RawSteering` (the includes still paths) as a `toml::Table` checked by hand, so each refusal
names the key: only `context`, `include` and `[[rule]]` at the top (`unknown key `x``), only
`id` and `text` in a rule (`unknown key `level` in [[rule]] 2`); an id matches
`^[a-z][a-z0-9-]*$`, at most 40 characters, no `--`, no trailing `-`; ids are unique
(`[[rule]] 5: id `ports` is also [[rule]] 1's`); a text and an include are not blank. Every
failure is `Error::Config { file, why }`.

`ScryToml::steering` (`adapters/scry_toml.rs`, the `Settings` port) refuses an old
`.sorcerer/scry.md` whenever it exists, gives `Steering::default()` with no `scry.toml`, and
otherwise reads each include in order: relative, no `..`, allowed by `Fences::allows` (which
resolves symlinks, so a link out of the root is refused, and fences `.env*` and
`.sorcerer/`), and UTF-8; each failure names `include[N]` and the path. Then `over_cap`
(`STEERING_CAP`, 32 KiB of rendered text) is an error naming the size, the cap and the largest
part; nothing is truncated. `steering_source` is the file when there is one (doctor and the
run log say it).

`steering_text` is the prompt's block: the context, each include as
`<include path="…">…</include>`, and `Rules (cite one by its id):` with `rules_text`, the parts
a blank line apart, `(none)` with nothing. `rules_text` is `- id: first line`, further lines
indented two spaces; Jev's nitpick question carries it when a finding cites no rule, so the
prompt and Jev see the rules the same way.

`lookup(cited)` is how a reply's `rule` becomes an id: trimmed, one pair of quotes, backticks
or brackets taken off, lowercased, then an exact id. There is no substring or text matching:
a reply that quotes a rule's words instead of its id loses the rule (the finding stays), and
the run log says so; misses are measured in use.

## The finding and its store

`domain/finding.rs` holds the record. `Finding` has a `FindingId` (six hex digits, stable for its
life), `issue_type`, optional `rule`, `statement`, a `Place` (file, unit, lines, `unit_start`,
`Fingerprint`), `Vec<Evidence>` (file, lines, and the `print` of the cited code when verified),
`Scores` (`confidence`, and `asked_on`, what the nitpick judgement was asked about), an optional
`StoredVerdict` (`right`, `wrong`, `cannot_tell`, or the citation failure, with its reason,
and `cited`, each `StoredCitation` behind it with `confirmed` and `unit_missed` (skipped when
false; `StoredCitation::outcome` words it for the log and `why`): what the claim check cited, or
for a citation failure the finding's own evidence; `serde(default)` and skipped when empty,
so a store from before it reads and one with nothing cited is written as before),
`status`, `seen`, `first`, `last`, `history`, and the `model` and `cost_usd` of the review.
Every status change goes through `Finding::set_status`, which appends to `history`.

`Status`, written kebab-case in the file:

- `open`: shown; it passed the checks and the nitpick bar.
- `unverified`: found, not yet checked (a cost cap held the pass back). Stores written before 2026-11-20 say `waiting`; `Status` reads it through a serde alias, and `list --status waiting` still matches (`list_findings::names`).
- `withheld`: judged a nitpick; kept, not shown.
- `refuted`: the claim checker said the claim is wrong.
- `unsure`: the checker could not tell, or the citations did not hold.
- `elsewhere`: its lines are in no unit in scope.
- `dismissed-here`: the user dismissed it for this unit.
- `dismissed-everywhere`: the user dismissed it everywhere.
- `resolved`: the code changed and it no longer holds.
- `gone`: its unit no longer exists.

`domain/findings.rs` is `Findings` (`findings`, plus `reviewed`, each file to the set of unit
fingerprints a pass has looked at, and `capped`, each file to the units a cost cap held back), stored by `JsonFindingStore` (`adapters/json_finding_store.rs`, the `FindingStore` port),
`Versioned` at version 2 (a version-1 file is unreadable and left alone), at
`.sorcerer/state/scry/findings.json` through core's `Store`; its `hold` takes the pass lock. A file from a newer scry is
`Unreadable` and never rewritten. `new_id` hashes time, file, lines and statement with FNV and
keeps six digits, rehashing with a counter while taken. `get` takes an id or a unique prefix of
at least four characters. `dismiss` sets `dismissed-here` or `dismissed-everywhere`. `shown_in`
is a file's open findings whose issue type is in `[scry] issue_types`, best confidence first, at most
`MAX_SHOWN`.

## Where a finding sits now

The store keeps the line a finding had when it was reviewed; the file moves on. An editor
needs the line as the file is now, so `domain/placing.rs` places a finding among the file's
units now: `placed(finding, units_now, line_count) -> Option<Placed { line, stale }>`.

- Its unit is the one named `place.unit`; of two with that name, the one at the stored
  fingerprint, else the first. With none of that name it is not placed (`None`): the next
  pass marks it gone.
- Its line is the unit's start now plus its distance from the unit's start when it was
  reviewed (`place.lines.start - place.unit_start`, 0 when `unit_start` was not stored), kept
  inside the unit's lines.
- It is stale when the unit's fingerprint differs from the stored one.
- A finding with no unit (about the whole file) keeps its stored line, moved up to the last
  line when the file got shorter, and is never stale.

`usecases/show_findings.rs` is what an editor shows in a file: `shown_in_file(store,
settings, view, file)` reads the store afresh, takes `Findings::shown_in` (exactly what
`scry` shows there: open, of the shown issue types, most confident first, at most three),
and places each with `placed`, dropping the ones whose unit is gone. It is empty when
`[scry] enabled = false`. A file whose units cannot be read counts as having none. It adds
no filter of its own, so the editor and `scry list` agree about what is open.

## The review call

`domain/review_prompt.rs` is the review call as data; its words are the eval's. A `ReviewAsk` carries the batch's
`ReviewUnit`s (file, name, `lines`, enclosing `header`, `text`, `in_progress`), the diff, the
steering text, `reported` (always empty in v1; spellbook's slot) and the `open` findings to
re-answer. `review_prompt` renders it; `tests/golden/review-prompt.txt` is the golden, matched
byte for byte. Unit lines are numbered `{:>5}  {text}` from the unit's first line.

`parse_review` reads the reply: JSON is located from fenced chunks last-first, then the first `{`
to the last `}`, then the first `[` to the last `]` (the array first when it opens first, so a
bare array of findings is not mistaken for its first object); the first that decodes wins. A
finding needs one of the four issue types (the reply's `issue_type`), a file, a statement and `lines` (`[a, z]`, one element, or
`line`); a bad finding is dropped, a bad evidence entry is dropped and the finding kept. `open`
answers need `holds` to be exactly `still holds` or `no longer holds`; a numeric id is accepted.
Only a reply with no JSON at all is an `Err`.

`batch_units` keeps units in order in batches of at most 600 numbered lines; a longer unit goes
alone. `Reviewer` (`ports/reviewer.rs`) is the seam: `review(ask, model, cap_usd, cancel)` returns a `Reviewed` (the reply
text and its `Errand`). `ResearchReviewer` (`adapters/research_reviewer.rs`) is the live one, one `Research` (product `scry`, verb
`review`, tools Read/Grep/Glob, `REVIEW_TIMEOUT` 420 s); the eval swaps in recorded replies
through the same trait.

## The scope

`usecases/review_change/scope.rs`. `scope_of(project, view, request, all, cfg)` turns a
`ScopeRequest` (`Files`, `Working`, `Since(rev)`; `domain/scope.rs`) into a `Scope`: the `base` compared against, the `ScopedUnit`s to
review and the `ScopedFile`s the pass touched. The base is `HEAD`, or for `Since` the view's
`merge_base(rev)` (a bad rev is that `Error::Git`, naming it), each a `Base::Revision`; with no commit yet it is
`Base::NoCommitYet` and every file is all new. The files are the named ones, or the view's `changed_since(base)`
(every file, by `glob("**")`, with no commit). A file is dropped when no language covers it,
`[scry] exclude` matches it, `Fences::allows` refuses it, or its text is `NotText`.

The file reads top to bottom as these steps. `scope_of` builds a `Scoping` (the project, the
view, the `Fences`, the `[scry] exclude` set, the base from `Scoping::base_revision`, and
`all`), takes its `candidate_files` (each once; an absolute path outside the project is
dropped by `relative_to_the_project`), starts from `Scope::empty` and folds in `look_at`'s answer about each
file (`with_file`). `look_at` answers a `FileInScope`: `Skipped` with `reason_to_skip`'s reason (the
first that holds, in the order above) or `not text` (`text_unless_binary`); `Deleted` from
`no_longer_in_the_tree`; or `Present`, with `units_in_scope` turned into `scoped_units` (which calls it) and
the file into a `scoped_file`.

A file that is gone but was at the base is a `ScopedFile` with `deleted` and no units, so the
pass can sweep its findings. A rename is a deletion and an addition (`changed_since` runs
with `--no-renames`). Each file considered and left out goes to `Scope::skipped` with why
(`no language`, `excluded by [scry] exclude`, `fenced`, `not text`, `deleted, and not at the
base`), for the run log. For a file that exists, the units are `compare_units(view.units_at(file,
base), view.units(file))`'s `Added` and `Changed`, or every unit with `all` or no commit; a
base whose units do not read counts as none. `ScopedFile::diff` is `view.diff(file, base)`, or
the file's text when that is empty; `units_now` names every unit now, for the gone sweep.

`header_of` is the innermost unit strictly enclosing this one (any
kind, another name), and its definition line trimmed. `def_line` finds that line as the first
line before the body holding the unit's own name, so a decorator above it is passed over.

## Judging a finding

`domain/judging.rs` (the checks of citations are
`usecases/review_change/verify.rs`) is what happens to a `RawFinding` after the review call.

`place(raw, reviewed, units_now)` gives a `Placement`. Among the `ScopedUnit`s of
its file whose span overlaps its lines, the one holding its first line wins, then the
narrowest; that is `Placement::InReviewedUnit`. With none it is `Placement::Elsewhere`, placed in the narrowest unit of
`units_now` (the file's units now) holding its first line, if any. The place carries that
unit's name, first line and fingerprint.

`candidates(store, raw)` are the stored findings worth the same-problem question, in store
order: not `resolved` or `gone`, sharing the file, the rule (the pass looks the cited id up with
`Steering::lookup` first), or a cited range in the same file. The pass asks them in
order and the first with P(yes) at or past `SAME_THRESHOLD` (0.5) matches. `finding_text` shows each
side as `bug at a.py:41; rule: none; cancel() returns None`. The question, `same_problem`:

- kind `same`, instructions "Do these two code review findings describe the same problem?"
- yes: "one fix would fix both: the same defect or rule break at the same code, however it is
  worded"; no: "they need separate fixes: different defects, or the same kind of defect at
  different code"; unsure not allowed.
- context `finding A`, `finding B`, `code at A`, `code at B` (each `cited_code`).

`match_stored(stored, new_unit)` is the merge table (`Match::MergeInto`, `Drop`, `AddAsNew`): open, unverified, withheld, refuted, unsure
and elsewhere merge; dismissed-here drops in the unit it was dismissed in and is new in any
other (or none); dismissed-everywhere drops; resolved and gone are new.

`nitpick(steering, raw, cited)` is the second question:

- kind `nitpick`, instructions "Is this code review finding a nitpick?"
- yes is `NITPICK_IS`, no is `WORTH_SAYING_IS` (constants in `domain/judging.rs`); unsure
  not allowed.
- context `issue type`, `finding`, `place` (`file:first`), `cited code`, then `project rule`
  (the cited rule's text, by its id) or, citing none, `project rules`
  (`Steering::rules_text`; `(none)` without rules).

`confidence_of` turns the answer into confidence = 1 − P(nitpick): `Yes(Unknown)` is 0, `No(Unknown)` and
`Unsure` are 1. `asked_on(unit_fp, rule_text)` is seven hex digits of FNV-1a over the
unit's fingerprint, the rule text the question carried and the two sides; a withheld finding is
asked again only when it changes. `cited_code(text, line)` is eight lines either side, each
`{:>4} {text}`.

`citations(view, fences, evidence)` checks each cited range by its first line: a relative path,
allowed by `Fences::allows` (so not `.env*`, not `.sorcerer/`, not outside the root), readable
through the view, first line in `1..=lines`. The end line is ignored, as the claim checker
clamps it: 50-60 of a 59-line file holds. Any failure is
`Err("cited code not found")`, no evidence at all `Err("no evidence cited")`; both make the
finding unsure. Each `Evidence` carries a print: the enclosing unit's fingerprint
(`unit_at`), else seven hex digits of the whole file's FNV-1a.

## The review_change use case

`usecases/review_change/`. `review_change(project, &ReviewRequest, &ReviewDeps)` takes a request (the
`ScopeRequest`, `all`, `fresh`, the `--budget`, and the caps when the caller sets them; none
means the command line's, `Caps { pass: None, day: budget_per_day, budget }`) and its ports:
the `CodebaseView`, the `Reviewer`, the `Decider` (Jev), the `ClaimChecker`, the
`FindingStore`, core's `Ledger`, the `Settings`, a `Clock` (`SystemClock` live, `FixedClock`
in tests), the `RunLog` (below), a `progress` callback told `Progress::Sending(units)` right before each
review call goes out (the `SentUnit { file, unit, fingerprint }` of each unit in it; told only for
a call that really goes, after the caps and the cancel let it), `Progress::Reviewed(n)` after each
review call that answered and `Progress::Stored` after the write, and two cancels: `cancel`
stops Jev and the claim checks, `review_cancel` the review calls only (the one in flight, and
`send_batch` checks it before each call). The command line passes one `Cancel` for both; an
editor's server sets `review_cancel` when a save overtakes what a call was sent. It reads
`[scry]` and returns `Skipped` when it is off, works out the scope (`scope_of`, logged with
`scope_text`) and the caps (`caps_for`), then returns `ReviewOutcome::Ran(ReviewReport)` (`looked_at` units sent and answered, the `new` ids,
`spent`, `capped` units, one `failures` line per failed call, `whole`, `checks_capped`
when a claim check did not fit, `day_spent` when what was held back was held by the day's cap
(`Pass::day_held_something_back`: a capped call and `Budget::day_is_spent(review_estimate)`, or
a capped check and `day_is_spent(check_estimate)`), and `touched`, every finding the pass added or changed: those
of `Pass::working_copy` not equal to the store as read (`touched_since`), mapped through the renames), or
`ReviewOutcome::Skipped` when `[scry] enabled` is false or Jev did not answer; the report
carries the `scope` so the caller prints what was in it. The command line
(`cli::go`) checks `enabled` itself before the wiring (`LiveWiring::open`, which needs a Jev key and
a provider) and exits 0, so a pre-commit hook
in a project that turned scry off lets the commit through; any other `Skipped` exits 2.
`review_change` itself reads as the frame of the pass: lock, `findings_on_disk`,
`SpentSoFar::read`, the Pass section (`pass_text`), `Pass::new`, `Pass::run`, its halts (`Halt::JevDidNotAnswer`
is `Skipped`, `Halt::Failed` the error), `Progress::Stored`, unlock. `Pass::run` consumes the pass and ends
by saving it and making the report (`Pass::report`). The steps, in
`Pass::run` (`mod.rs`), each in its own file:

1. **Lock and read.** `FindingStore::hold` takes `state/scry/pass.lock` with `File::lock`
   (blocking) for the whole pass, so two passes never interleave. The store is read once (`findings_on_disk`); `Unreadable` is
   `Error::StoreUnreadable` and nothing is touched. The pass works on a copy, `Pass::working_copy`.
   The ledger is read once too (`SpentSoFar::read`, which also fixes the pass's `now`), through the `Ledger` port: its `review` and `check` rows give the estimates and, through
   core's `cost::spent_since` in the `Clock`'s offset, what scry spent earlier in the local day,
   which starts the pass's `Budget` (Jev's `decide` rows are not counted: a fraction of a cent each).
2. **What to send, the gone sweep and the put-back sweep** (`ask.rs`, `remember.rs`). The scope's units whose fingerprint is not in
   `reviewed[file]` (every one with `fresh`). `Pass::sweep_gone`: an open, unverified or withheld
   finding of a scoped file whose unit is not in `units_now`, or whose file was deleted, is
   `gone`. `Pass::sweep_put_back`: an open or unverified finding of a scoped file whose unit
   is in that file's `ScopedFile::same` (the units equal to the base's, filled by
   `Scoping::compared_with_base` where `compare_units` runs; empty with `--all` or no base) at a
   fingerprint other than the finding's is `resolved` with no call: the user undid the change
   it was about, so no pass would review that unit again. A finding whose fingerprint is the
   base's (the change was committed, bug and all) stays open; one stored with no fingerprint
   is left alone. The store is written even when nothing is sent.
3. **Review** (`ask.rs`). `batch_units` batches them; each batch is a `ReviewAsk` with its files' diffs
   (`diff_of`), `Steering::steering_text`, `reported` empty, and the open and unverified findings on its
   units (`Pass::open_on`). Before each call `Budget::allows(find_estimate)`; once one batch is
   capped every later one is, and its units' names go to `capped[file]` in the store. The
   call's `cap_usd` is the per-call cap, set here in the pass and nowhere else:
   `Budget::money_left` bounded by `budget_per_pass` (`budget_per_pass` when no cap is finite). On the
   command line `Caps.pass` is `None`, so without the bound `money_left` is the rest of the local
   day and one hung call would be charged all of it. A call that fails, or a reply
   `parse_review` refuses, is a `failures` line (`review failed for a.py: …`) and its units stay
   unreviewed; the reviewer has already put its errand in the ledger. The budget is charged the
   errand's cost (the estimate when it has none), the cap for a `Timeout`, nothing for any
   other failure. In the code, `Pass::send` is that story in order: `units_not_yet_reviewed`,
   `batches_of` (each a `UnitToSend`: the prompt's `ReviewUnit` and the fingerprint it is sent at,
   batched by `batch_by_lines`), then `send_batch` per batch: `must_hold_back` →
   `hold_back_for_a_later_run`, `review_cancel` set → a failure line (`review cancelled for …`), or `review_one_batch` (`ask_for`,
   `cap_for_one_call`, `Progress::Sending`, the call with `review_cancel`, `cost_of_call`, the reply parsed, `Pass::tell_progress`). Each
   returns a `BatchOutcome` (`HeldBack`, `Failed`, `Answered`), and `ReviewResults::with` adds it to
   the results so far (an answer on an open finding the call was not shown is ignored there).
4. **Judge.** `Pass::judge` per raw finding: a cited rule `Steering::lookup` does not find
   is dropped (`with_cited_rule_looked_up`; the finding stays; the log says `cited "x": no rule has that id`); `place` with the finding's own file's units (`units_now`); an
   `elsewhere` finding is stored with no question (`add_elsewhere`). Otherwise `candidates` go to Jev in one
   `ask` batch of `same_problem` questions and the first at P ≥ `SAME_THRESHOLD` wins
   (`same_problem_as_stored`); `WhatToDo::given` (over `match_stored`) says merge (`Pass::merge`), drop, or add as new. A new finding gets the `nitpick`
   question (`add_unverified_or_withheld`, over `ask_nitpick`, which returns a `NitpickAnswer`): confidence at or past `confidence_threshold` is `unverified`, else `withheld`, with
   `asked_on`. A merge into a finding stored by an earlier pass replaces its place,
   statement and evidence (`with_this_runs_version`; printed by `citations` when they hold); a merge into a finding
   this pass found (two findings of one pass that are the same problem) keeps the first
   one's place, statement and evidence (the refresh is for a finding kept by a later pass). Either way it adds one to `seen` and keeps id, status and
   dismissals; a withheld one is asked
   again when `asked_on` changed (`asked_again_if_changed`), a refuted or unsure one is unverified again when its evidence prints
   moved (`unverified_again_if_evidence_moved`); each returns the finding rather than changing it in place, and `Finding::seen_again` counts the sighting. Any `Unsure` (or error) from Jev ends the pass as `Skipped("Jev did not answer")`
   with nothing written (`Halt::JevDidNotAnswer`).
5. **Remember.** The reply's `open` answers count only for ids that call was shown in its
   `ask.open` (a mistyped or invented id is ignored). Those it said *no longer holds* (open or
   unverified) are `resolved` (`resolve_what_no_longer_holds`); those it said *still holds* go into `still_hold`.
   `Pass::drop_new_findings_in_changed_units` (over `units_changed_since_sent`, which reads each sent file's units again, giving `ChangedUnits`): a unit
   whose fingerprint is not what was sent keeps none of the pass's new findings. When the pass
   is `whole` (nothing capped, no failure), an open or unverified finding from before the pass
   whose unit was sent at a fingerprint other than its place's, and that nothing merged into,
   kept (`still_hold`) or resolved, is `resolved` (not found again: `resolve_not_found_again` and `was_fixed`, over the
   answered units `ChangedUnits::not_changed`; `NotFoundAgain::NotChecked` when not every unit was reviewed). The prompt tells the reviewer not
   to raise an open finding again, so `still_hold` is what keeps an unfixed finding open across an
   edit of its unit.
6. **Verify.** `Pass::verify_findings_in_file` for each scoped file that still exists, sharing the pass's
   `Budget` (below). It runs even when nothing in the file was sent, so an unverified finding
   opens once the file has room.
7. **Write.** `Pass::drop_new_findings_in_changed_units` again (a unit may have changed while the checks ran); the
   fingerprints of the sent, answered, unmoved units join `reviewed` and leave `capped`
   (`Pass::mark_reviewed`, `mark_one_reviewed`). Each
   write (the one before a round's checks, and this last one) is a `FindingStore::update` applying
   `merge_into(last_written, working_copy, current)`, `last_written` being what the pass last wrote: each finding
   in the store as it is now takes the pass's version only when its status there is still the
   one the pass last saw (so a dismissal made during the pass stands); the pass's new findings
   are appended; `reviewed` sets are joined; `capped` is the pass's when it changed it. A new
   finding whose id another writer took meanwhile gets a fresh one (`free_id`); `merge_into`
   records it in `Pass::renamed_ids`, applies the map to `working_copy` on every later write (`last_written` is
   kept in written ids through `renamed`). The last write ends `Pass::run`; `Pass::report` then fills `touched`
   (`touched_since` the pass's `stored_before`) and gives `ReviewReport::new` and `touched` as written (`as_written`). Whether every unit was reviewed is `ReviewResults::every_unit_was_reviewed`.

## The run log

`ReviewDeps::log` is a `RunLog` (`ports/run_log.rs`), handed Markdown as the pass goes; it
is `Quiet` in the live wiring, the eval and the tests that do not read it (a closure is a
`RunLog` too, which is how the tests collect it). The text is built with
`domain/log_text.rs`. It is called on
the pass's thread only (a round's claim checks are traced after they join), so it needs no
`Sync`, and the command line's can write to a stderr that is not `Send`. Nothing the pass
decides reads it: the eval's table is the same with or without it. The calls:

- `review_change`: the **Scope** section (`scope_text`), then the **Pass** section (`pass_text`), the caps, what scry spent earlier today, the estimates.
- `Pass::send`: **Units** (each scope unit, to send or already reviewed), **Review calls** (per
  batch: units, model, cap, the open ids, `review_prompt(&ask)` and the reply each in
  `block`, the errand, then the parse or the failure, or the cap that held it back; each piece
  built by one of `ask.rs`'s `…_text` functions, last in the file);
  `Pass::judge_all`: **Findings** (a `sub` heading a raw finding, then `Pass::judge`);
  `Pass::remember`: **Remember** (resolved by the reply, not found again, swept gone by
  `sweep_gone`, dropped as moved; `remember_text`); and after the rounds `Pass::mark_reviewed`: **Reviewed** (`reviewed_text`).
- `Pass::judge` and `Pass::merge`: the statement, the rule as cited and as resolved, the
  evidence, the placement, each same-problem question's P(yes) and the outcome, the nitpick
  question's P(yes) with the confidence against `confidence_threshold`, and the id and status given.
- `Pass::verify_findings_in_file`: a **Verification** section for a file with unverified findings, then per
  round the room, the candidates and the rest; each finding's citations (`cites`
  names the one that failed); each claim check's models (its errands'), escalation, verdict
  and reason, cost, resulting status, and every `Checked::cited` through `citation`.

## Rounds and the budget

`domain/budget.rs` (caps and estimates), `domain/verification.rs` (the rounds' policy) and
`usecases/review_change/verify.rs` (the rounds) are the verification rounds and the caps.

`review_estimate(rows, model)` and `check_estimate(rows)` are the mean of the last 20 priced
ledger rows of product `scry` and verb `review` / `check`, else $0.04 a review ($0.005 when the
model name contains `haiku`) and $0.016 a check. `Budget::new(caps, spent_today)` holds the
`Caps` (`pass`, `day`, `budget`); `allows(e)` is false when `spent + e` passes `pass` or
`budget` (when set) or `spent_earlier_today + spent + e` passes `day`; `charge` adds a cost; `money_left` is the
least left under any finite cap; `day_is_spent(e)` is whether `spent_earlier_today + spent + e`
passes `day`. `cost_of(errands)` sums a call's priced errands.

`Pass::verify_findings_in_file(file, unverified_from_earlier_runs)` takes the file's unverified findings,
best confidence first (`PrioritizedFindings::unverified_in`, the only way to build one, so the order holds), in rounds. A round takes the candidates that would fit if all passed
(`DisplaySlots::pick_for_this_round` returns the round's `FindingsToVerify` and the rest, still `PrioritizedFindings`: most confident first, each taken while a slot would be free if every pick so far turned out true; from the file's `DisplaySlots`: how many it displays and how many of those this run discovered): one
that was unverified before the pass (`unverified_from_earlier_runs`) only while the file's open findings
of shown issue types are under `MAX_SHOWN`; one this pass found while fewer than `MAX_SHOWN` of the
pass's own have opened (only issue types in `cfg.issue_types` count). Then, per finding (`check_cited_lines`, over `check_cited_lines_of`): `citations`
(failure → `unsure`, the reason as verdict); a `Verify::Citations` issue type opens at once; a
`CitationsThenClaim` issue type's check is reserved against the budget at `check_estimate`, and one
that does not fit leaves its finding unverified and stops the rounds (`AfterCitedLines::BudgetRanOut`; the file's rounds end `RoundsEnded::BudgetRanOut`).
When there are checks to run the store is written and `Progress::Stored` (`save_before_paid_checks`, over `Pass::save_working_copy`)
sent, so what opened shows before the checks finish. The round's checks run side by side on
`std::thread::scope` (`run_paid_checks` over `claim_checks_side_by_side`), each recorded in the order started (`record_paid_check`), each a `Claim` of the statement about its evidence: `Right` opens,
`Wrong` is `refuted`, `CannotTell` is `unsure` (verdict, reason and `Checked::cited` stored, the cost
charged and added to the finding's `cost_usd`); an `Err` leaves it unverified, its place free for the
next round, which takes the rest; `DisplaySlots::after_round` returns the slots with what opened counted, and `verify_round` calls itself for the next round with what is left. The rounds end when a round takes nobody.

## Wiring

`adapters/wiring.rs`. `Wiring` is what the command line needs from a wiring: `deps()`, the
review's `ReviewDeps`. `LiveWiring` is the real one, built by `LiveWiring::open(project, env)`,
which is `LiveWiring::open_with(project, env, review, rest)` with one `Cancel` twice (an
editor's server passes two: `review` becomes `review_cancel`, `rest` the claim checker's and
`cancel`):

- Jev first, by `jev_for`: `Jev::from_env` over `[decide]` with the project's ledger, then
  `for_product("scry")`, so its errands are scry's `decide`. No key is an error naming the
  variable (`JEV_API_KEY` unless `[decide] key_env` says otherwise).
- The view, a `FilesView` on the user's `GrammarCache`.
- The researcher by `researcher_for(project, env, view)`: the provider by
  `default_provider_for`; when its kind is `claude` the researcher is `ClaudeCodeResearcher`
  (Claude Code reads the files itself), otherwise `LoopResearcher` over the provider, the view,
  the project's fences and its ledger. The eval's record mode builds its researcher with the same
  function, so a recording asks what `scry` itself would.
- The reviewer, `ResearchReviewer`, which holds the shared researcher (scry's `review`).
- The claim checker, `claim_checker(researcher, decider, fences, view, languages, cancel)`: a
  boxed `ClaimChecker` that runs `sorcerer_models::usecases::check_claim::check_claim` with
  `CheckClaimDeps::defaults(.., "scry")` and the project's `languages`, so its errands are scry's
  `check`, Jev guards it, and its prompt names units the way the claim's files' language does.
- The store (`JsonFindingStore`), the ledger (`JsonlLedger`), the settings (`ScryToml`),
  `SystemClock`, a progress callback that prints nothing, and `Quiet` as the run log (the
  command line puts its own in).

Everything is shared through `Arc`s so `LiveWiring` owns it all and `deps()` lends it.

## The command line

`src/cli/`, parsed with clap (`Cli`, `RunArgs`, `Cmd`; `args_conflicts_with_subcommands`, so
bare files and subcommands do not mix). `main(args, env, out, err)` is what the binary calls:
it takes the process's directory, the terminal's width (`terminal_size`, 100 when stdout is not
a terminal) and `LiveWiring::open` as the wiring, and hands them to `main_with`, which the tests call
with their own directory, width and a `Wiring` of doubles. Every error is one `scry: …` line on
stderr and exit 2; clap's usage errors exit 2, `--help` 0.

A run (`cli/review.rs`, `run`): open the project from the directory, `check_budget`, `scry_config`, the
wiring, then `review_change` with a `ReviewRequest` (`request_from`) of `Since(rev)`, `Files` (joined to the
directory, so `../a.py` works from a subdirectory) or `Working`, `--all`, `--fresh` and
`--budget`, and no caps of its own, so the use case's `Caps { pass: None, day: budget_per_day,
budget: --budget }` hold (the day's cap already counts what scry spent earlier today, so leaving `budget` unset is "the
day's remaining"); then the store is read again and `Findings::shown_in_scope` (domain) takes every open finding
of a shown issue type whose place's file and unit are a scope unit, best confidence first, with no
`MAX_SHOWN`. `print_findings` prints each through `finding_text` (the statement wrapped under itself), or `finding_json`
one object a line. `exit_code` is 1 when a finding is shown, else 2 after a failure or a cap, else 0.
The summary line (`summary_line`, said by `say_summary_line`: stdout, or stderr under `--json`) is skipped when nothing was shown, looked at or spent.

The store commands (`list`, `cost`, `dismiss`, `why`, `log`, a file each in `src/cli/`) need no Jev, so `run_command` does
not require the wiring: `clock_of` asks it for the clock (the tests' `FixedClock` carries the
zone) and falls back to the system clock when `LiveWiring::open` fails. Each calls its use case
over the adapters it builds (`JsonFindingStore`, `JsonlLedger`, `ScryToml`) and prints.
`list_findings` sorts by `status_rank`, then file and line, and prints through `table` (columns padded to their widest
cell). `report_cost` groups the ledger's `scry` errands by local day in the clock's offset
(`BTreeMap` by date): `review` and `check` are counted and summed, `decide` only added to the
total; the waiting units are the store's `capped` sets (a store that does not read is a
`scry:` line on stderr and no waiting line, `waiting_units: null`). `dismiss_finding` is `Findings::dismiss` (with a `Dismissal`, from the `--everywhere` flag)
inside `FindingStore::update`; `explain_finding` re-reads the steering for the rule's text by its id (`why` says when the id
is no longer in `scry.toml`) and each cited file through the view (the
command line prints `cited_range`), reading a cited path only when it is relative and `Fences::allows` it (inside
the root after `..` and symlinks, not fenced): evidence on a finding that was never verified
is whatever path the model wrote (the cited lines, two either side; not `judge::cited_code`, the claim prompt's
window of 8 around the first line).
`held_back_text` is the failures and caps, a stderr line each.

**The run log.** A run goes through `logged`: it builds a `StreamingLog`
(`adapters/markdown_run_log.rs`: the text in a `RefCell`, and stderr, written to as the text
grows when `-v`; the run's other stderr lines go through `StreamingLog::say`), opens the wiring, puts `head` (time from the wiring's clock, the root, the
version, the command line `main_with` joined from its args, the caps, the effective `[scry]`
serialized a `key = value` line each) and the **Steering** section (`steering_section`: the
file, the context's size, each include's size, the rule ids, the size against the cap, or
the error; `head` is `run_text`, `config_text` and `steering_section`), then `run`, which hands the review its `ReviewDeps` with
`log` set to the `StreamingLog` (the use case puts the scope), and at the end
`summary_text`: each of `ReviewReport::touched` with its status, verdict and why it is or is not
printed (`touched_text`, `why_not_printed`); the ledger's new `scry` rows (read before and after the pass, `ledger_rows`) by verb (`spent_text`); failures and
caps (`failures_and_caps_text`); and the `RESULT` line with the exit code. A run that errs or is skipped still gets
a summary (`error_summary_text`, `skipped_summary_text`). `write_run_log` then calls `write_run`, which creates `state/scry/runs/<UTC %Y-%m-%dT%H-%M-%SZ>.md`
(`create_new`, `-2`, `-3` on a clash) and deletes all but the newest `KEEP` (20), newest by
`run_key` (the stamp, then the counter, so `-10` sorts after `-9`); a failure is one stderr
line. A run with scry off writes no log. `scry log` (`cli/log.rs`, `show_log`) reads `runs` without
creating the state directory: what it is asked for is a `LogRequest` (`Text(n)`, `Path(n)` or `List`): the Nth, `--list` (`table` of name, `run_time` in the clock's
offset, `result_of`) or `--path`; none is exit 2. `why` prints a verdict's `cited` under
`Cited by the check:`, a `StoredCitation::label` and `confirmed` or `not found` each. Exit: `[scry] enabled = false` is 0 before any
pass; `Skipped` or an error is 2; anything shown is
1; else 2 when a review failed, a unit was capped or a claim check was held back
(`ReviewReport::checks_capped`), else 0.

`doctor_rows(project, env, cache)` (`cli/doctor.rs`, with `check_setup` and `languages_row`:
doctor probes real adapters, so it is not a use case) reads over `JsonFindingStore::at` and
`JsonlLedger::at`, so nothing creates `.sorcerer/state`: `provider` (`default_provider_for`),
`jev` (`Jev::from_env` without a ledger), `languages` (`[languages] names`, else the
languages whose `files` cover a file `glob("**")` lists, each `Languages::load`ed through the
cache), `steering` (`2 rules, 1 include, context 120 B, 1.4 KB; .sorcerer/scry.toml`,
`no .sorcerer/scry.toml: no rules`, or the error), `store` and `ledger`. `doctor` prints them `name  ok|FAIL  detail` and
exits 2 when any failed.

## The editor's server

`scry lsp` (`cli/lsp.rs`) serves the editor over standard input and output:
`serve(Connection::stdio(), ServerConfig::live(SystemEnv))`, then it exits 0. It is a product
on the host crate ([The language-server host](lsp.md)); `adapters/lsp/` holds what a save
means to scry, and it calls the use cases the command line calls.

**What it is built from.** `ServerConfig<W: Wiring>`: the environment, `open` (how to open the
project at a root: `Project::open` live), the grammar `cache`, the user's `trust` list, the
`settle` wait, the `clock`, and `wire` (a pass's wiring from the project and its two cancels:
`LiveWiring::open_with` live). `ServerConfig::live(env)` is the real one; the tests in
`tests/lsp.rs` give a 20 ms settle, a fixed clock, vendored grammars and a wiring of the pass
doubles, over `Connection::memory()`. The binary has no test hook.

**Which files.** The server serves one project: `open` of the root the editor sent at
initialize (its first workspace folder, else `rootUri`; the working directory when it sent
neither), opened afresh each time it publishes, so a config edit needs no restart. A file is
the project's when `Project::root_above` its directory is the project's root, compared on the
canonical path (`project_file`); a file of a nested project, outside the root, or with no
language is ignored without a word.

**Publishing.** One thread, started with the server (`start_publisher`), publishes in the
order it is asked, so the main loop never waits on a parse or a grammar build and a close
always lands after the open before it. `Publish::File(path)` is sent at an open and at a save,
`Publish::Clear(path)` at a close (an empty list). For a file that is open and the project's,
`Server::diagnostics_for` reads the store afresh (`JsonFindingStore::at`, so a read creates
nothing), takes `shown_in_file` and turns each `Shown` into a diagnostic. Only open documents
are ever published. An error (the config does not parse, the store is from a newer scry) goes
to the editor's log (`window/logMessage`), each text once until the server next publishes
(`log_error`, over `errors_logged`), and publishes nothing; the next save tries again.
`publish_open` (`Publish::AllOpen`) republishes every open document; a pass and a dismissal
call it.

**From a save to a pass** (`passes.rs`). The threads: the main loop (the host's), the
publisher, and job threads from `Jobs` (the settle wait, then the pass itself, and the
overtaking check).

- `Server::saved(file)` publishes the file, adds it to `Passes::pending`, and submits
  `run_pending` under the key `pass` after `settle`, so a burst of saves restarts the wait and
  becomes one pass.
- `run_pending`, on the job's thread: `take_pending` turns the whole pending set into a
  `RunningPass` (its files, two fresh `Cancel`s, the units its last call was sent, its
  progress line), unless one is running (it starts the next itself) or the server is shutting
  down. The pass runs inside `catch_unwind`; whatever happens (an error, a panic) is logged,
  the progress line ends, the pass stops being the running one (`finish`), and if files were
  saved meanwhile the next pass is submitted with no wait. So a pass that dies never leaves the
  server "running".
- `Server::review(pass)`: opens the project, keeps the saved files that are its own
  (`project_file`), reads `[scry]`, builds the wiring with `cfg.wire(project, review_cancel,
  rest)`, and calls `review_change` with `ScopeRequest::Files`, `all` and `fresh` false, and
  `Caps { pass: Some(budget_per_pass), day: budget_per_day, budget: None }`; the run log goes
  to a buffer and `progress` to `told`.
- `told`: `Progress::Sending(units)` remembers the units (for the overtaking check) and begins
  the progress line (`work_done("scry", "reviewing 1 unit")`) or reports on it;
  `Progress::Stored` republishes, once the pass has made a call (a finding that needs no claim
  check shows before the checks end).
- `after_review`: republish when the pass made a call or touched a finding (a put-back resolve);
  end the line, with `review failed, see "scry log"` when a call failed and no save cancelled
  it on purpose; when `ReviewReport::day_spent`, `say_once("day-<local date>", …)` the day's
  message with `budget_per_day`. A run log is written (`write_run`) only when the pass made a
  call or failed, so `scry log` is not filled with empty saves. A clean save says nothing.
- **The wiring fails** (no Jev key, no provider): `cannot_run` logs it always, and says
  `scry is not reviewing: <reason>. Run "scry doctor".` once a session (`say_once("cannot-run")`)
  when `scope_of` the saved files has a unit to review.
- **Overtaking.** A save during a pass, of a file the running call was sent units of, submits
  `overtakes` under the key `overtake`: it reads the units now of the sent units' files, and
  when none of them still has the fingerprint it was sent at, it sets the pass's
  `review_cancel` (only that one: Jev and the claim checks run on) and marks the pass
  `overtaken`. The cancelled call ends as a failure line the progress line does not report;
  the saved file is pending, so the next pass reviews the fresh text at once.
- `Server::shutdown` (the product's `shutdown`) sets both cancels of the running pass, drops
  what is pending and stops new passes; it returns at once.

**A finding as a diagnostic** (`diagnostic(shown, root, text, related)`):

| field | value |
|---|---|
| `range` | the placed line, from character 0 to its length in UTF-16 units |
| `severity` | `Error` for `bug`, `Warning` for every other issue type |
| `source` | `scry` |
| `code` | the issue type, or `<issue type>-stale` |
| `message` | the statement; then `rule: <id>` when it cites a rule; then `(the code changed since this review)` when stale; one per line |
| `relatedInformation` | each evidence, its file and whole lines, message `evidence`; only when the editor announced `relatedInformation` |
| `data` | the finding's id, as a string |

## Test support

`crates/scry/tests/support/mod.rs` holds what the scope, pass and CLI tests share:
`git_repo` (no commit), `git_project`, `write`, `commit`, `project_with_vendored_grammars`
(core's vendored grammars, no network), and three doubles. `StubReviewer` answers a review by the
set of files in its batch (an empty findings list otherwise) and records every `ReviewAsk`.
`StubDecider` answers each question with the first rule whose kind prefixes the question's and
whose predicate holds, else a certain no, and records the questions. `StubReviewer::on_review` runs a hook during each call (a user acting mid-pass).
`FixedClock` is a clock set by hand (`at`, `now`, `offset` to move it, `in_zone` for the
user's offset) and implements the `Clock` port. `Ports::of(project)` builds the store, ledger
and settings adapters a `ReviewDeps` needs. `errand(verb, model, cost)` is a ledger row
as a paid call would append it. The rounds tests (`tests/rounds.rs`) seed unverified findings
straight into the store with `cancel` already reviewed, so no review call runs, and pass a
claim checker closure; the pass tests use one that always errs (an unverified finding stays
unverified).

The command line tests (`tests/cli.rs`) wire a `Doubles` (in `tests/support/`, shared with the
editor's server tests: a vendored-grammar view, `StubReviewer`,
`StubDecider`, `FixedClock`, a checker that says right and the fixture's `Ports`) into `main_with` through
`impl Wiring for &Doubles`, so the reviewer's asks can be read after two runs.

## The eval

`crates/scry/tests/eval.rs` runs each corpus under `corpus/scry/<name>/` through whole passes with
every model call replayed, so it costs nothing and runs in CI's test job. One `#[test]` a corpus
(`bistro_reaches_its_passing_scores` for the Python corpus), each `corpus(name)` then
`run_corpus(&Corpus)` then `assert_passing_scores`, so a failure names its corpus and
`cargo test -p scry --test eval bistro_rs` runs one. `corpus/scry/README.md` is the index of corpora
and says what each file is; each corpus's own `README.md` says how it was made.

- `corpus.toml`, read into `Corpus`: `name`, `language` (written into each case's config),
  `sets` (the table's rows, in order), `[passing_scores]` (`PassingScores`: `v_must_min`, `b_min`,
  `t_mean_max`, `n_mean_max`) and `[baseline]`, the recording they were set from, for people. The
  passing scores are data, reviewed in a diff, and move only by a ruling recorded beside them. A
  corpus with no `[passing_scores]` prints its table and fails "no passing scores yet".
- `base/`, the project as plain files; per case the eval copies it to a temp dir, `git init`s and
  commits it, then `git apply`s `cases/<id>.patch` (left uncommitted) and writes
  `.sorcerer/scry.toml` from the corpus's `scry.toml` (its context and rules, ids as the recorded
  replies cite them) and a config with `[languages] names = ["<language>"]` and
  `[scry] budget_per_pass = 5.0, budget_per_day = 1000.0`. The project's grammars point at core's
  vendored sources; the request is `ScopeRequest::Working` with the caps
  `Caps { pass: Some(budget_per_pass), day: budget_per_day, budget: None }`. `run_case` reads as a
  case's story: `project_for_case`, the replaying ports (`CorpusReviewer`, `recorded_decider`,
  `CorpusChecker`), the pass (`pass_over`), its log written (`case_log`, `write_case_log`),
  `assert_ran`, a look (`print_for_a_look`) and the row (`shown_statements`).
- `reviews/<id>.json`, the recorded review replies in the batches form
  `{"batches": [{"files", "reply", "cost_usd"}]}` (`recorded_findings` reads every batch's findings,
  each citing a rule by id; `the_recorded_replies_cite_the_corpus_rule_ids` checks bistro's are all in
  its `scry.toml`). `CorpusReviewer` answers a batch with the findings, over all the case's batches,
  whose `file` is one of the batch's files (an empty reply otherwise), at $0.04, so a change in
  batching still replays. A case with no file, or a batch with a file no recorded batch holds, fails naming the case and the batch.
- `checks.json`, the checker's recorded reply keyed by the statement with whitespace collapsed:
  `RecordedCheck { verdict, why, evidence }`, the evidence being the places the checker cited with
  whether scry found each (`RecordedEvidence`), kept for people. `CorpusChecker` answers from it
  (`replayed`): the verdict and the `why`, no citations; `CannotTell` for a statement it does not hold.
  A file in the older form (a verdict word for a value) is refused by `read_recorded_checks`.
- `jev/<question_key>.json`, Jev's answers. `RecordedDecider` keys each question by
  `question_key` (FNV-1a over the Jev body without the model, so a question asked the same
  way has the same key). A question with no file
  is an error, the pass is `Skipped`, and the eval fails naming the case.
- `labels.json`, each case's set, the statements that count as a hit, the `unplanted` ones (true
  findings nobody planted; the key may be absent) and its changed files.

Shown is the union over a case's changed files of `Findings::shown_in`; `count` sorts it, in every
set by one rule: a statement in the case's `unplanted` list is counted in `unplanted` and left out of
`shown`; a case is a hit when a shown statement is one of its `hits`; in a T or N case a shown
statement in neither list goes to `to_read`. The eval prints `# <name>`, then `set | cases | with a hit
shown | shown a case | unplanted` and one row a case (the same column), then the "to read" list
(`to_read_text`; absent when empty: a person reads each line, judges it, and copies a true one into
`unplanted`, never to make a number fit), then asserts the corpus's passing scores: bistro's are V-must
at least 16 of 18 with a hit shown, B at least 6 of 8, at most 0.5 shown a T case and an N case on
average.

`SCRY_EVAL_LOG=DIR` writes each case's `scope_text` and pass trace to `DIR/<id>.md`.
`SCRY_EVAL_CASES=b01,vm02` runs only those cases, prints every stored finding and every
question Jev is asked (kind, key, P(yes), the findings), and asserts no passing score. To re-record Jev,
run once by hand with `JEV_API_KEY` set:
`SCRY_RECORD=jev cargo test -p scry --test eval -- --nocapture`; only missing answers are
asked, and the ledger rows land in the temp projects. Delete a file to ask its question again.

To record a corpus, run its test once by hand with `JEV_API_KEY` set and `claude` on `PATH`:
`SCRY_RECORD=all cargo test -p scry --test eval bistro_rs -- --nocapture` (name one corpus: a
statement with no verdict in another corpus would be checked too). `run_corpus` then calls
`run_corpus_with(corpus, only, Record::All, &live)`; `live` builds, per case, the live calls in a
`Live`: a `ResearchReviewer` over `researcher_for`, the real `claim_checker` with Jev guarding, and
a `RecordedDecider` recording with `jev_for`'s Jev. A batch is a miss when its case has no `reviews/<case>.json`, or when one of the batch's files is in none
of the recorded batches (a recording that stopped part way through the case, `recorded_files`):
`CorpusReviewer` asks the live reviewer with the pass's own `ReviewAsk` and model and appends
`{"files", "reply", "cost_usd"}` for each batch (batches sorted by their files). A statement not in
`checks.json` is checked live and the checker's reply (`RecordedCheck::of`: the verdict word `right`,
`wrong` or `cannot_tell`, the `why`, the evidence) written back, sorted (`CorpusChecker::record_check`).
`SCRY_RECORD=checks` (`Record::Checks`) builds the same live calls but uses only the checker: it asks every
check of the cases run once in the run (`should_ask`, `Run.asked_this_run`) and overwrites each entry, while
reviews and Jev replay; to re-ask one case's checks, name it with `SCRY_EVAL_CASES`. Nothing recorded is asked again, so an interrupted recording resumes where it stopped. Each
case prints the running total (`stop_past_the_ceiling`), and the end prints `recorded: N reviews $X, M checks $Y, K jev
answers` (`recorded_text`). Without `SCRY_RECORD=all`, a miss fails naming the case and the batch's files
(`a_partly_recorded_case_asks_for_the_missing_batch` covers the partial case).
`record_mode_from` refuses any `SCRY_RECORD` when `CI` is set, before the first case. The tests
`recording_writes_each_batch_and_the_checkers_reply`, `a_miss_without_record_names_the_case_and_batch`
and `record_is_refused_under_ci` run the mode on a fixture corpus with stubs for the live calls.

Bistro, replayed on 2026-11-18 at scry `8e5e255` (identical over three runs, no Jev key; its
`corpus.toml` baseline), with Jev's 44 answers
re-recorded live that day for the `scry.toml` steering, beside the numbers before it (the
Markdown steering, where a cited heading was matched by substring and Jev mostly saw the whole
Rules section):

| set | cases | with a hit shown | shown a case | before: hit, shown |
|---|---|---|---|---|
| V-must | 18 | 17 | 1.222 | 18, 1.333 |
| B | 8 | 6 | 0.875 | 6, 0.875 |
| T | 18 | 0 | 0.000 | 0, 0.000 |
| N | 8 | 0 | 0.125 | 0, 0.125 |
| V-taste | 6 | 0 | 0.000 | 0, 0.000 |

Two cases changed. vm16 lost its hit: its finding cites `repo-one-op`, and Jev, now shown
that rule's text rather than the whole Rules section, put P(nitpick) at 0.51 (0.22 before), so
its confidence 0.49 is under 0.70 and it is withheld. vm05 shows 2, not 3: the finding citing
`statuses-failures` is now judged the same problem as the one citing `rules-in-commands` at
P 0.51 (0.49 before), so it merges; the case still hits. vm02, vm07 and vm08 each hold two
findings Jev calls the same problem within one pass; they hit because a merge within one pass
keeps the first finding's wording (see the pass, step 4).


bistro-rs (`corpus/scry/bistro-rs/`, the Rust corpus: a std-only Rust bistro with its own frozen
conventions, 32 cases), recorded live once on 2026-11-19 (`SCRY_RECORD=all`: 32 reviews on sonnet
through `claude -p`, $1.86; 8 claim checks, $0.02; 21 Jev answers) and replayed at scry `4df22b0`
(identical over three runs, no Jev key). Its test is `bistro_rs_reaches_its_passing_scores`:

| set | cases | with a hit shown | shown a case | passing score |
|---|---|---|---|---|
| V-must | 10 | 10 | 1.100 | at least 9 |
| B | 6 | 4 | 0.667 | at least 3 |
| T | 10 | 0 | 0.100 | at most 0.5 |
| N | 6 | 0 | 0.167 | at most 0.5 |

The passing scores were set from this first recording by one rule: one case of slack on V-must and
B (hits − 1), so a drift in one Jev answer does not fail CI and a second case changing side does,
and 0.5 shown a case on T and N, as for bistro. They move only by a ruling recorded in the corpus's
`corpus.toml` with its date and reason: up when a scry change raises the replayed number and the
user wants it held, never down to pass. A re-recording (a new review model, a changed prompt) resets
the baseline, and its report lists every case that changed side.
