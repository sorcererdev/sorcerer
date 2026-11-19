# Scry

`crates/scry`: the code reviewer, ported from olifant's `inspect`. Library `scry`, binary
`scry`.

## Module map

- `src/config.rs`: `ScryConfig`, `scry_config`.
- `src/issue_type.rs`: `IssueType`, `Verify`, `ISSUE_TYPES`, `issue_type`.
- `src/steering.rs`: `Steering`, `steering_of`, `steering`.
- `src/scope.rs`: `ScopeWhy`, `Scope`, `ScopedUnit`, `ScopedFile`, `scope_of`.
- `src/judge.rs`: `place`, `candidates`, `same_problem`, `finding_text`, `SAME_THRESHOLD`, `Match`,
  `match_stored`, `nitpick`, `confidence_of`, `asked_on`, `cited_code`, `citations`.
- `src/pass.rs`: `pass`, `PassDeps`, `Clock`, `SystemClock`, `Checker`, `Progress`, `Caps`,
  `Report`, `Outcome`, `merge_into`.
- `src/rounds.rs`: `find_estimate`, `check_estimate`, `Budget`, `cost_of`, and the
  crate-private `verify` (with `Around`, `Verified`).
- `src/live.rs`: `Live`, `Wired`, `jev_for`, `checker_over`, `PRODUCT`.
- `src/log.rs`: the run log: `Trace`, `quiet`, `section`, `sub`, `block`, `usd`, `count`,
  `citation`, `scope_text`, `runs_dir`, `write_run`, `runs`, `run_key`, `run_time`,
  `result_of`, `KEEP`, `RESULT`.
- `src/cli.rs`: `main`, `main_with`, `doctor_rows`.
- `src/main.rs`: the binary, `cli::main` with the real environment.
- `tests/support/mod.rs`: git fixtures, `project_with_vendored_grammars`, and the doubles
  `StubFinder`, `StubDecider`, `FixedClock`.
- `library/scry.md`: the shipped steering layer.

## Config

`scry_config(&Project)` reads `[scry]` through core's `Config::table`, which is strict about
unknown keys (`deny_unknown_fields`), then range-checks: `confidence_threshold` in 0..=1,
`budget_per_pass` and `budget_per_day` not negative (nor NaN), and every entry of `issue_types`
a name in `ISSUE_TYPES`. The keys are `enabled`, `issue_types`, `review_model`,
`confidence_threshold`, `budget_per_pass`, `budget_per_day` and `exclude`; at most
`MAX_SHOWN` (3, a constant in `config.rs`) findings are shown in a file. Each failure is an `Error::Config` naming the key and `.sorcerer/config.toml`.
`ScryConfig::default()` holds the shipped values.

## Issue types

`ISSUE_TYPES` is the table of issue types: a `name`, the `sentence` the find prompt prints after it, whether
it is `shown` by default, and its `Verify` (`Citations`, or `CitationsThenClaim` where the
claim checker also runs). The sentences are those of the golden find prompt, word for word.
A new issue type is a new row. `[scry] issue_types` chooses which rows are shown.

## Steering

`steering(&Project)` is `steering_of(&[shipped, project file])`, where the shipped layer is
`library/scry.md` via `include_str!`. `steering_of` joins the layers with a blank line and
splits the result into sections (`heading`, `sections`). Rules are headings of level three or
more under `## Rules`, in file order. `nitpick` and `worth` come from the last non-empty
`## A nitpick is` and `## Worth saying is`, else the shipped sides.

`resolve_rule` matches a rule a finding cites: ignoring case, the cited text inside a heading
or a heading inside the cited text, first in file order. An unresolved rule is the caller's to
drop; the finding stays. `rule_text` is the body of a named rule, and `rules_section` is the
whole `## Rules` section, which Jev reads when a finding cites no rule.

## The finding and its store

`src/finding.rs` holds the record. `Finding` has a `FindingId` (six hex digits, stable for its
life), `issue_type`, optional `rule`, `statement`, a `Place` (file, unit, lines, `unit_start`,
`Fingerprint`), `Vec<Evidence>` (file, lines, and the `print` of the cited code when verified),
`Scores` (`confidence`, and `asked_on`, what the nitpick judgement was asked about), an optional
`StoredVerdict` (`right`, `wrong`, `cannot_tell`, or the citation failure, with its reason,
and `cited`, each `StoredCitation` behind it with `confirmed` and `unit_missed` (skipped when
false; `StoredCitation::outcome` words it for the log and `why`): what the claim check cited, or
for a citation failure the finding's own evidence; `serde(default)` and skipped when empty,
so a store from before it reads and one with nothing cited is written as before),
`status`, `seen`, `first`, `last`, `history`, and the `model` and `cost_usd` of the find.
Every status change goes through `Finding::set_status`, which appends to `history`.

`Status`, written kebab-case in the file:

- `open`: shown; it passed the checks and the nitpick bar.
- `waiting`: found, not yet checked (a cost cap held the pass back).
- `withheld`: judged a nitpick; kept, not shown.
- `refuted`: the claim checker said the claim is wrong.
- `unsure`: the checker could not tell, or the citations did not hold.
- `elsewhere`: its lines are in no unit in scope.
- `dismissed-here`: the user dismissed it for this unit.
- `dismissed-everywhere`: the user dismissed it everywhere.
- `resolved`: the code changed and it no longer holds.
- `gone`: its unit no longer exists.

`src/store.rs` is `ScryStore` (`findings`, plus `reviewed`, each file to the set of unit
fingerprints a pass has looked at, and `capped`, each file to the units a cost cap held back), `Versioned` at version 2 (a version-1 file is unreadable and left alone), opened by `store(&Project)` at
`.sorcerer/state/scry/findings.json` through core's `Store`. A file from a newer scry is
`Unreadable` and never rewritten. `new_id` hashes time, file, lines and statement with FNV and
keeps six digits, rehashing with a counter while taken. `get` takes an id or a unique prefix of
at least four characters. `dismiss` sets `dismissed-here` or `dismissed-everywhere`. `shown_in`
is a file's open findings whose issue type is in `[scry] issue_types`, best confidence first, at most
`MAX_SHOWN`.

## The find call

`src/find.rs` is inspect's find call, ported word for word. A `FindAsk` carries the pack's
`FindUnit`s (file, name, `lines`, enclosing `header`, `text`, `in_progress`), the diff, the
steering text, `reported` (always empty in v1; spellbook's slot) and the `open` findings to
re-answer. `find_prompt` renders it; `tests/golden/find-prompt.txt` is inspect's golden, matched
byte for byte. Unit lines are numbered `{:>5}  {text}` from the unit's first line.

`parse_find` reads the reply: JSON is located from fenced chunks last-first, then the first `{`
to the last `}`, then the first `[` to the last `]` (the array first when it opens first, so a
bare array of findings is not mistaken for its first object); the first that decodes wins. A
finding needs one of the four issue types (the reply's `kind` field), a file, a statement and `lines` (`[a, z]`, one element, or
`line`); a bad finding is dropped, a bad evidence entry is dropped and the finding kept. `open`
answers need `holds` to be exactly `still holds` or `no longer holds`; a numeric id is accepted.
Only a reply with no JSON at all is an `Err`.

`pack_units` keeps units in order in packs of at most 600 numbered lines; a longer unit goes
alone. `Finder` is the seam: `find(ask, model, cap_usd, cancel)` returns a `Found` (the reply
text and its `Errand`). `ResearchFinder` is the live one, one `Research` (product `scry`, verb
`find`, tools Read/Grep/Glob, `FIND_TIMEOUT` 420 s); the eval swaps in recorded replies
through the same trait.

## The scope

`src/scope.rs`. `scope_of(project, view, why, all, cfg)` turns a `ScopeWhy` (`Saved`, `Named`,
`Working`, `Since(rev)`) into a `Scope`: the `base` compared against, the `ScopedUnit`s to
review and the `ScopedFile`s the pass touched. The base is `HEAD`, or for `Since` the view's
`merge_base(rev)` (a bad rev is that `Error::Git`, naming it); with no commit yet it is empty
and every file is all new. The files are the named ones, or the view's `changed_since(base)`
(every file, by `glob("**")`, with no commit). A file is dropped when no language covers it,
`[scry] exclude` matches it, `Fences::allows` refuses it, or its text is `NotText`.

A file that is gone but was at the base is a `ScopedFile` with `deleted` and no units, so the
pass can sweep its findings. A rename is a deletion and an addition (`changed_since` runs
with `--no-renames`). Each file considered and left out goes to `Scope::skipped` with why
(`no language`, `excluded by [scry] exclude`, `fenced`, `not text`, `deleted, and not at the
base`), for the run log. For a file that exists, the units are `compare_units(view.units_at(file,
base), view.units(file))`'s `Added` and `Changed`, or every unit with `all` or no commit; a
base whose units do not read counts as none. `ScopedFile::diff` is `view.diff(file, base)`, or
the file's text when that is empty; `units_now` names every unit now, for the gone sweep.

`header_of` is olifant's `headerOf`: the innermost unit strictly enclosing this one (any
kind, another name), and its definition line trimmed. `def_line` finds that line as the first
line before the body holding the unit's own name, so a decorator above it is passed over.

## Judging a finding

`src/judge.rs` is inspect's `Judge.hs`: what happens to a `RawFinding` after the find call.

`place(raw, scoped, units_now)` gives its `Place` and first status. Among the `ScopedUnit`s of
its file whose span overlaps its lines, the one holding its first line wins, then the
narrowest; that is `Open`. With none it is `Elsewhere`, placed in the narrowest unit of
`units_now` (the file's units now) holding its first line, if any. The place carries that
unit's name, first line and fingerprint.

`candidates(store, raw)` are the stored findings worth the same-problem question, in store
order: not `resolved` or `gone`, sharing the file, the rule (the pass resolves the cited rule
with `Steering::resolve_rule` first), or a cited range in the same file. The pass asks them in
order and the first with P(yes) at or past `SAME_THRESHOLD` (0.5) matches. `finding_text` shows each
side as `bug at a.py:41; rule: none; cancel() returns None`. The question, `same_problem`:

- kind `same`, instructions "Do these two code review findings describe the same problem?"
- yes: "one fix would fix both: the same defect or rule break at the same code, however it is
  worded"; no: "they need separate fixes: different defects, or the same kind of defect at
  different code"; unsure not allowed.
- context `finding A`, `finding B`, `code at A`, `code at B` (each `cited_code`).

`match_stored(stored, new_unit)` is the merge table: open, waiting, withheld, refuted, unsure
and elsewhere merge; dismissed-here drops in the unit it was dismissed in and is new in any
other (or none); dismissed-everywhere drops; resolved and gone are new.

`nitpick(steering, raw, cited)` is the second question:

- kind `nitpick`, instructions "Is this code review finding a nitpick?"
- yes is `Steering::nitpick`, no is `Steering::worth` (the shipped sides, or the project's
  `## A nitpick is` / `## Worth saying is`); unsure not allowed.
- context `finding kind`, `finding`, `place` (`file:first`), `cited code`, then `project rule`
  (the cited rule's text) or, citing none, `project rules` (`Steering::rules_section`).

`confidence_of` turns the answer into confidence = 1 − P(nitpick): `Yes(Unknown)` is 0, `No(Unknown)` and
`Unsure` are 1. `asked_on(unit_fp, rule_text, steering)` is seven hex digits of FNV-1a over the
unit's fingerprint, the rule text the question carried and the two sides; a withheld finding is
asked again only when it changes. `cited_code(text, line)` is eight lines either side, each
`{:>4} {text}`.

`citations(view, fences, evidence)` checks each cited range by its first line: a relative path,
allowed by `Fences::allows` (so not `.env*`, not `.sorcerer/`, not outside the root), readable
through the view, first line in `1..=lines`. The end line is ignored, as olifant's checker
clamps it (`Server/Checker.hs` `confirm`): 50-60 of a 59-line file holds. Any failure is
`Err("cited code not found")`, no evidence at all `Err("no evidence cited")`; both make the
finding unsure. Each `Evidence` carries a print: the enclosing unit's fingerprint
(`unit_at`), else seven hex digits of the whole file's FNV-1a.

## The pass

`src/pass.rs` is inspect's `Pass.hs` (`inspectPass`, `mergeInto`), design §6.4 steps 1–5
and 8. `pass(project, scope, caps, fresh, deps, cancel)` takes a `Scope` from `scope_of` and
`PassDeps`: the `CodebaseView`, the `Finder`, the `Decider` (Jev), the claim `Checker`, a
`Clock` (`SystemClock` live, `FixedClock` in tests), a `progress` callback told
`Progress::Found(n)` after each find call that answered and `Progress::Stored` after the
write, and a `trace` (below). It returns `Outcome::Ran(Report)` (`looked_at` units sent and answered, the `new` ids,
`spent`, `capped` units, one `failures` line per failed call, `whole`, `checks_capped`
when a claim check did not fit, and `touched`, every finding the pass added or changed: those
of `Run::mine` not equal to the store as read, mapped through the renames), or
`Outcome::Skipped` when `[scry] enabled` is false or Jev did not answer. The command line
(`cli::go`) checks `enabled` itself before the wiring (`Live::open`, which needs a Jev key and
a provider) and exits 0, so a pre-commit hook
in a project that turned scry off lets the commit through; any other `Skipped` exits 2. The steps, in
`Run::go`:

1. **Lock and read.** `state/scry/pass.lock` is taken with `File::lock` (blocking) for the
   whole pass, so two passes never interleave. The store is read once; `Unreadable` is
   `Error::StoreUnreadable` and nothing is touched. The pass works on a copy, `Run::mine`.
   The ledger is read once too: its `find` and `check` rows give the estimates and, through
   core's `cost::spent_in` in the `Clock`'s offset, what scry spent earlier in the local day,
   which starts the pass's `Budget` (Jev's `decide` rows are not counted, design §6.9).
2. **What to send, and the gone sweep.** The scope's units whose fingerprint is not in
   `reviewed[file]` (every one with `fresh`). `Run::sweep_gone`: an open, waiting or withheld
   finding of a scoped file whose unit is not in `units_now`, or whose file was deleted, is
   `gone`.
3. **Find.** `pack_units` packs them; each pack is a `FindAsk` with its files' diffs
   (`diff_of`), the steering text, `reported` empty, and the open and waiting findings on its
   units (`Run::open_on`). Before each call `Budget::allows(find_estimate)`; once one pack is
   capped every later one is, and its units' names go to `capped[file]` in the store. The
   call's `cap_usd` is the per-call cap, set here in the pass and nowhere else:
   `Budget::room` bounded by `budget_per_pass` (`budget_per_pass` when nothing bounds the room). On the
   command line `Caps.pass` is `None`, so without the bound `room` is the rest of the local
   day and one hung call would be charged all of it. A call that fails, or a reply
   `parse_find` refuses, is a `failures` line (`find failed for a.py: …`) and its units stay
   unreviewed; the finder has already put its errand in the ledger. The budget is charged the
   errand's cost (the estimate when it has none), the cap for a `Timeout`, nothing for any
   other failure.
4. **Judge.** `Run::judge` per raw finding: a cited rule `Steering::resolve_rule` cannot
   resolve is dropped (the finding stays); `place` with the finding's own file's units; an
   `elsewhere` finding is stored with no question. Otherwise `candidates` go to Jev in one
   `ask` batch of `same_problem` questions and the first at P ≥ `SAME_THRESHOLD` wins;
   `match_stored` says merge (`Run::merge`), drop, or new. A new finding gets the `nitpick`
   question (`Run::confidence`): confidence at or past `confidence_threshold` is `waiting`, else `withheld`, with
   `asked_on`. A merge into a finding stored by an earlier pass replaces its place,
   statement and evidence (printed by `citations` when they hold); a merge into a finding
   this pass found (two findings of one pass that are the same problem) keeps the first
   one's place, statement and evidence (olifant `d8fc391` scopes the refresh to a finding
   kept by a later pass). Either way it adds one to `seen` and keeps id, status and
   dismissals; a withheld one is asked
   again when `asked_on` changed, a refuted or unsure one waits again when its evidence prints
   moved. Any `Unsure` (or error) from Jev ends the pass as `Skipped("Jev did not answer")`
   with nothing written (`Halt::Silent`).
5. **Remember.** The reply's `open` answers count only for ids that call was shown in its
   `ask.open` (a mistyped or invented id is ignored). Those it said *no longer holds* (open or
   waiting) are `resolved`; those it said *still holds* go into `held`.
   `Run::drop_moved` (over `Run::moved`, which reads each sent file's units again): a unit
   whose fingerprint is not what was sent keeps none of the pass's new findings. When the pass
   is `whole` (nothing capped, no failure), an open or waiting finding from before the pass
   whose unit was sent at a fingerprint other than its place's, and that nothing merged into,
   kept (`held`) or resolved, is `resolved` (not found again). The prompt tells the finder not
   to raise an open finding again, so `held` is what keeps an unfixed finding open across an
   edit of its unit.
6. **Verify.** `rounds::verify` for each scoped file that still exists, sharing the pass's
   `Budget` (below). It runs even when nothing in the file was sent, so a waiting finding
   opens once the file has room.
7. **Write.** `Run::drop_moved` again (a unit may have changed while the checks ran); the
   fingerprints of the sent, answered, unmoved units join `reviewed` and leave `capped`. Each
   write (the one before a round's checks, and this last one) is a `Store::update` applying
   `merge_into(written, mine, current)`, `written` being what the pass last wrote: each finding
   in the store as it is now takes the pass's version only when its status there is still the
   one the pass last saw (so a dismissal made during the pass stands); the pass's new findings
   are appended; `reviewed` sets are joined; `capped` is the pass's when it changed it. A new
   finding whose id another writer took meanwhile gets a fresh one (`free_id`); `merge_into`
   records it in `Run::renames`, applies the map to `mine` on every later write (`written` is
   kept in written ids through `renamed`), and `pass` maps `Report::new` through it.

## The trace

`PassDeps::trace` is a `log::Trace`, `&dyn Fn(&str)`, handed Markdown as the pass goes; it is
`log::quiet` in the live wiring, the eval and the tests that do not read it. It is called on
the pass's thread only (a round's claim checks are traced after they join), so it needs no
`Sync`, and the command line's can write to a stderr that is not `Send`. Nothing the pass
decides reads it: the eval's table is the same with or without it. The calls:

- `pass`: the **Pass** section, the caps, what scry spent earlier today, the estimates.
- `Run::go`: **Units** (each scope unit, to send or already reviewed), **Find calls** (per
  pack: units, model, cap, the open ids, `find_prompt(&ask)` and the reply each in
  `log::block`, the errand, then the parse or the failure, or the cap that held it back),
  **Findings** (a `sub` heading a raw finding, then `Run::judge`), **Remember** (resolved by
  the reply, not found again, swept gone by `sweep_gone`, dropped as moved) and, after the
  rounds, **Reviewed**.
- `Run::judge` and `Run::merge`: the statement, the rule as cited and as resolved, the
  evidence, the placement, each same-problem question's P(yes) and the outcome, the nitpick
  question's P(yes) with the confidence against `confidence_threshold`, and the id and status given.
- `rounds::verify`: a **Verification** section for a file with waiting findings, then per
  round the room, the candidates and the rest; each finding's citations (`judge::cites`
  names the one that failed); each claim check's models (its errands'), escalation, verdict
  and reason, cost, resulting status, and every `Checked::cited` through `log::citation`.

## Rounds and the budget

`src/rounds.rs` is inspect's verification rounds and caps (`Pass.hs`: `findEstimate`,
`claimEstimate`, `spentToday`, the `rounds` loop), design §6.4 steps 6–7 and §6.9.

`find_estimate(rows, model)` and `check_estimate(rows)` are the mean of the last 20 priced
ledger rows of product `scry` and verb `find` / `check`, else $0.04 a find ($0.005 when the
model name contains `haiku`) and $0.016 a check. `Budget::new(caps, spent_today)` holds the
`Caps` (`pass`, `day`, `budget`); `allows(e)` is false when `spent + e` passes `pass` or
`budget` (when set) or `today + spent + e` passes `day`; `charge` adds a cost; `room` is the
least left under any finite cap. `cost_of(errands)` sums a call's priced errands.

`verify(store, file, cfg, budget, deps, cancel, around)` takes the file's waiting findings,
best confidence first, in rounds. A round takes the candidates that would fit if all passed: one
that was waiting before the pass (`Around::old_waiting`) only while the file's open findings
of shown issue types are under `MAX_SHOWN`; one this pass found while fewer than `MAX_SHOWN` of the
pass's own have opened (only issue types in `cfg.issue_types` count). Then, per finding: `citations`
(failure → `unsure`, the reason as verdict); a `Verify::Citations` issue type opens at once; a
`CitationsThenClaim` issue type's check is reserved against the budget at `check_estimate`, and one
that does not fit leaves its finding waiting and stops the rounds (`Verified { capped }`).
When there are checks to run the store is written (`Around::save`) and `Progress::Stored`
sent, so what opened shows before the checks finish. The round's checks run side by side on
`std::thread::scope`, each a `Claim` of the statement about its evidence: `Right` opens,
`Wrong` is `refuted`, `CannotTell` is `unsure` (verdict, reason and `Checked::cited` stored, the cost
charged and added to the finding's `cost_usd`); an `Err` leaves it waiting, its place free for the
next round, which takes the rest. The rounds end when a round takes nobody.

## The live wiring

`src/live.rs`. `Wired` is what the command line needs from a wiring: `deps()`, the pass's
`PassDeps`, and `cancel()`. `Live` is the real one, built by `Live::open(project, env)`:

- Jev first, by `jev_for`: `Jev::from_env` over `[decide]` with the project's ledger, then
  `for_product("scry")`, so its errands are scry's `decide`. No key is an error naming the
  variable (`JEV_API_KEY` unless `[decide] key_env` says otherwise).
- The provider by `default_provider_for`. When its kind is `claude` the researcher is
  `ClaudeCodeResearcher` (Claude Code reads the files itself); otherwise `LoopResearcher` over
  the provider, the view, the fences and the ledger.
- The view, a `FilesView` on the user's `GrammarCache`.
- The finder, `LiveFinder`, which lends the shared researcher to a `ResearchFinder` per call
  (scry's `find`).
- The claim checker, `checker_over(researcher, decider, fences, view, languages, cancel)`: a
  boxed closure that runs `sorcerer_models::checker::check` with
  `CheckerDeps::defaults(.., "scry")` and the project's `languages`, so its errands are scry's
  `check`, Jev guards it, and its prompt names units the way the claim's files' language does.
- `SystemClock`, a progress callback that prints nothing, and `log::quiet` as the trace (the
  command line puts its own in).

Everything is shared through `Arc`s so `Live` owns it all and `deps()` lends it.

## The command line

`src/cli.rs`, parsed with clap (`Cli`, `RunArgs`, `Cmd`; `args_conflicts_with_subcommands`, so
bare files and subcommands do not mix). `main(args, env, out, err)` is what the binary calls:
it takes the process's directory, the terminal's width (`terminal_size`, 100 when stdout is not
a terminal) and `Live::open` as the wiring, and hands them to `main_with`, which the tests call
with their own directory, width and a `Wired` of doubles. Every error is one `scry: …` line on
stderr and exit 2; clap's usage errors exit 2, `--help` 0.

A run (`run`): open the project from the directory, `scry_config`, the wiring, then
`scope_of` with `Since(rev)`, `Named` (files joined to the directory, so `../a.py` works from a
subdirectory) or `Working`; `pass` with `Caps { pass: None, day: budget_per_day, budget: --budget }`
(the day's cap already counts what scry spent earlier today, so leaving `budget` unset is "the
day's remaining"); then the store is read again and `shown_in_scope` takes every open finding
of a shown issue type whose place's file and unit are a scope unit, best confidence first, with no
`MAX_SHOWN`. `finding_text` prints each (the statement wrapped under itself), or `finding_json`
one object a line. The summary line is skipped when nothing was shown, looked at or spent.

The store commands (`list`, `cost`, `dismiss`, `why`, `log` in `src/cli.rs`) need no Jev, so `go` does
not require the wiring: `clock_of` asks it for the clock (the tests' `FixedClock` carries the
zone) and falls back to the system clock when `Live::open` fails. `list` sorts by
`status_rank`, then file and line, and prints through `table` (columns padded to their widest
cell). `cost` groups the ledger's `scry` errands by local day in the clock's offset
(`BTreeMap` by date): `find` and `check` are counted and summed, `decide` only added to the
total; the waiting units are the store's `capped` sets (a store that does not read is a
`scry:` line on stderr and no waiting line, `waiting_units: null`). `dismiss` is `ScryStore::dismiss`
inside `Store::update`; `why` re-reads the steering for the rule's text and each cited file for
`cited_range`, reading a cited path only when it is relative and `Fences::allows` it (inside
the root after `..` and symlinks, not fenced): evidence on a finding that was never verified
is whatever path the model wrote (the cited lines, two either side; not `judge::cited_code`, the claim prompt's
window of 8 around the first line).
`held_back` prints the failures and caps.

**The run log.** A run goes through `logged`: it builds a `RunLog` (the text in a `RefCell`,
and stderr, written to as the text grows when `-v`; the run's other stderr lines go through
`RunLog::say`), opens the wiring, puts `head` (time from the wiring's clock, the root, the
version, the command line `main_with` joined from its args, the caps, the effective `[scry]`
serialized a `key = value` line each), then `run`, which hands the pass a `PassDeps` with
`trace` set to `RunLog::put`, puts `log::scope_text` after `scope_of`, and at the end
`summary_text`: each of `Report::touched` with its status, verdict and why it is or is not
printed; the ledger's new `scry` rows (read before and after the pass) by verb; failures and
caps; and the `log::RESULT` line with the exit code. A run that errs or is skipped still gets
a summary. `log::write_run` then creates `state/scry/runs/<UTC %Y-%m-%dT%H-%M-%SZ>.md`
(`create_new`, `-2`, `-3` on a clash) and deletes all but the newest `KEEP` (20), newest by
`run_key` (the stamp, then the counter, so `-10` sorts after `-9`); a failure is one stderr
line. A run with scry off writes no log. `scry log` (`show_log`) reads `log::runs` without
creating the state directory: the Nth, `--list` (`table` of name, `run_time` in the clock's
offset, `result_of`) or `--path`; none is exit 2. `why` prints a verdict's `cited` under
`Cited by the check:`, a `StoredCitation::label` and `confirmed` or `not found` each. Exit: `[scry] enabled = false` is 0 before any
pass; `Skipped` or an error is 2; anything shown is
1; else 2 when a find failed, a unit was capped or a claim check was held back
(`Report::checks_capped`), else 0.

`doctor_rows(project, env, cache)` reads only (the store through `Store::at`, the ledger
through `Ledger::at`, so nothing creates `.sorcerer/state`): `provider` (`default_provider_for`),
`jev` (`Jev::from_env` without a ledger), `languages` (`[languages] names`, else the
languages whose `files` cover a file `glob("**")` lists, each `Languages::load`ed through the
cache), `steering`, `store` and `ledger`. `doctor` prints them `name  ok|FAIL  detail` and
exits 2 when any failed.

## Test support

`crates/scry/tests/support/mod.rs` holds what the scope, pass and CLI tests share:
`git_repo` (no commit), `git_project`, `write`, `commit`, `project_with_vendored_grammars`
(core's vendored grammars, no network), and three doubles. `StubFinder` answers a find by the
set of files in its pack (an empty findings list otherwise) and records every `FindAsk`.
`StubDecider` answers each question with the first rule whose kind prefixes the question's and
whose predicate holds, else a certain no, and records the questions. `StubFinder::on_find` runs a hook during each call (a user acting mid-pass).
`FixedClock` is a clock set by hand (`at`, `now`, `offset` to move it, `in_zone` for the
user's offset) and implements the pass's `Clock`. `errand(verb, model, cost)` is a ledger row
as a paid call would append it. The rounds tests (`tests/rounds.rs`) seed waiting findings
straight into the store with `cancel` already reviewed, so no find call runs, and pass a
claim checker closure; the pass tests use one that always errs (a waiting finding stays
waiting).

The command line tests (`tests/cli.rs`) wire a `Doubles` (a vendored-grammar view, `StubFinder`,
`StubDecider`, `FixedClock` and a checker that says right) into `main_with` through
`impl Wired for &Doubles`, so the finder's asks can be read after two runs.

## The eval

`crates/scry/tests/eval.rs`, `the_eval_reaches_its_passing_scores`: inspect's 58 bistro cases
run through whole passes with every model call replayed, so it costs nothing and runs in CI's
test job. The corpus is `corpus/scry/` (its `README.md` says where each file came from;
`scripts/port-scry-corpus` wrote it from olifant's recordings and the notes' cases):

- `base/`, bistro as plain files; per case the eval copies it to a temp dir, `git init`s and
  commits it, then `git apply`s `cases/<id>.patch` (left uncommitted) and writes
  `.sorcerer/scry.md` from `steering.md` (1a's steering) and a config with
  `[languages] names = ["python"]` and `[scry] budget_per_pass = 5.0, budget_per_day = 1000.0`. The
  project's grammars point at core's vendored sources; the scope is `scope_of(Working)`.
- `finds/<id>.json`, 1a's sonnet findings for the case. `CorpusFinder` answers a pack with the
  findings whose `file` is one of the pack's files (an empty reply otherwise), at $0.04.
- `checks.json`, 1a's claim-check verdicts keyed by the statement with whitespace collapsed;
  the checker closure answers from it, `CannotTell` for a statement it does not hold.
- `jev/<question_key>.json`, Jev's answers. `RecordedDecider` keys each question by
  `question_key` (FNV-1a over the Jev body without the model, the same bytes as olifant's
  `questionKey`, so a question asked the same way has olifant's key). A question with no file
  is an error, the pass is `Skipped`, and the eval fails naming the case.
- `labels.json`, each case's set, the statements that count as a hit, and its changed files.

Shown is the union over a case's changed files of `ScryStore::shown_in`; a case is a hit when
a shown statement is one of its `hits`. The eval prints `set | cases | with a hit shown |
shown a case` and one row a case, then asserts inspect's passing scores: V-must at least 16 of 18 with
a hit shown, B at least 6 of 8, at most 0.5 shown a T case and an N case on average.

`SCRY_EVAL_LOG=DIR` writes each case's `scope_text` and pass trace to `DIR/<id>.md`.
`SCRY_EVAL_CASES=b01,vm02` runs only those cases, prints every stored finding and every
question Jev is asked (kind, key, P(yes), the findings), and asserts no passing score. To re-record Jev,
run once by hand with `JEV_API_KEY` set:
`SCRY_RECORD_JEV=1 cargo test -p scry --test eval -- --nocapture`; only missing answers are
asked, and the ledger rows land in the temp projects. Delete a file to ask its question again.

Replayed on 2026-11-18 (identical over three runs), at inspect's measured numbers:

| set | cases | with a hit shown | shown a case |
|---|---|---|---|
| V-must | 18 | 18 | 1.333 |
| B | 8 | 6 | 0.875 |
| T | 18 | 0 | 0.000 |
| N | 8 | 0 | 0.125 |
| V-taste | 6 | 0 | 0.000 |

vm02, vm07 and vm08 each hold two findings Jev calls the same problem within one pass; they
hit because a merge within one pass keeps the first finding's wording (see the pass, step 4).
Most Jev answers were recorded live; three were seeded from olifant's recording, whose keys
are the same (`corpus/scry/README.md` lists them).
