# Scry

`crates/scry`: the code reviewer, ported from olifant's `inspect`. Library `scry`, binary
`scry`.

## Module map

- `src/config.rs`: `ScryConfig`, `scry_config`.
- `src/kind.rs`: `Kind`, `Verify`, `KINDS`, `kind`.
- `src/steering.rs`: `Steering`, `steering_of`, `steering`.
- `src/scope.rs`: `ScopeWhy`, `Scope`, `ScopedUnit`, `ScopedFile`, `scope_of`.
- `src/judge.rs`: `place`, `candidates`, `same_problem`, `finding_text`, `SAME_CUT`, `Match`,
  `match_stored`, `nitpick`, `keep_of`, `asked_on`, `cited_code`, `citations`.
- `tests/support/mod.rs`: git fixtures, `project_with_vendored_grammars`, and the doubles
  `StubFinder`, `StubDecider`, `FixedClock`.
- `library/scry.md`: the shipped steering layer.

Later tasks add the pass and the CLI here.

## Config

`scry_config(&Project)` reads `[scry]` through core's `Config::table`, which is strict about
unknown keys (`deny_unknown_fields`), then range-checks: `cut` in 0..=1, `max_shown` at least
1, `settle`, `pass_cost` and `day_cost` not negative, and every entry of `kinds` a name in
`KINDS`. Each failure is an `Error::Config` naming the key and `.sorcerer/config.toml`.
`ScryConfig::default()` holds the shipped values.

## Kinds

`KINDS` is the table of kinds: a `name`, the `sentence` the find prompt prints after it, whether
it is `shown` by default, and its `Verify` (`Citations`, or `CitationsThenClaim` where the
claim checker also runs). The sentences are those of the golden find prompt, word for word.
A new kind is a new row. `[scry] kinds` chooses which rows are shown.

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

`src/finding.rs` holds the record. `Finding` has an `Id` (six hex digits, stable for its
life), `kind`, optional `rule`, `statement`, a `Place` (file, unit, lines, `unit_start`,
`Fingerprint`), `Vec<Evidence>` (file, lines, and the `print` of the cited code when verified),
`Scores` (`keep`, and `asked_on`, what the nitpick judgement was asked about), an optional
`StoredVerdict` (`right`, `wrong`, `cannot_tell`, or the citation failure, with its reason),
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

`src/store.rs` is `ScryStore` (`findings`, plus `inspected` and `capped`, each file to a set
of unit names), `Versioned` at version 1, opened by `store(&Project)` at
`.sorcerer/state/scry/findings.json` through core's `Store`. A file from a newer scry is
`Unreadable` and never rewritten. `new_id` hashes time, file, lines and statement with FNV and
keeps six digits, rehashing with a counter while taken. `get` takes an id or a unique prefix of
at least four characters. `dismiss` sets `dismissed-here` or `dismissed-everywhere`. `shown_in`
is a file's open findings whose kind is in `[scry] kinds`, best `keep` first, at most
`max_shown`.

## The find call

`src/find.rs` is inspect's find call, ported word for word. A `FindAsk` carries the pack's
`FindUnit`s (file, name, `lines`, enclosing `header`, `text`, `in_progress`), the diff, the
steering text, `reported` (always empty in v1; spellbook's slot) and the `open` findings to
re-answer. `find_prompt` renders it; `tests/golden/find-prompt.txt` is inspect's golden, matched
byte for byte. Unit lines are numbered `{:>5}  {text}` from the unit's first line.

`parse_find` reads the reply: JSON is located from fenced chunks last-first, then the first `{`
to the last `}`, then the first `[` to the last `]` (the array first when it opens first, so a
bare array of findings is not mistaken for its first object); the first that decodes wins. A
finding needs one of the four kinds, a file, a statement and `lines` (`[a, z]`, one element, or
`line`); a bad finding is dropped, a bad evidence entry is dropped and the finding kept. `open`
answers need `holds` to be exactly `still holds` or `no longer holds`; a numeric id is accepted.
Only a reply with no JSON at all is an `Err`.

`pack_units` keeps units in order in packs of at most 600 numbered lines; a longer unit goes
alone. `Finder` is the seam: `find(ask, model, cap_usd, cancel)` returns a `Found` (the reply
text and its `Errand`). `ResearchFinder` is the live one, one `Research` (product `scry`, verb
`find`, tools Read/Grep/Glob, `FIND_TIMEOUT` 420 s); the corpus gate swaps in recorded replies
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
with `--no-renames`). For a file that exists, the units are `compare_units(view.units_at(file,
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
order and the first with P(yes) at or past `SAME_CUT` (0.5) matches. `finding_text` shows each
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

`keep_of` turns the answer into keep = 1 − P(nitpick): `Yes(Unknown)` is 0, `No(Unknown)` and
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

## Test support

`crates/scry/tests/support/mod.rs` holds what the scope, pass and CLI tests share:
`git_repo` (no commit), `git_project`, `write`, `commit`, `project_with_vendored_grammars`
(core's vendored grammars, no network), and three doubles. `StubFinder` answers a find by the
set of files in its pack (an empty findings list otherwise) and records every `FindAsk`.
`StubDecider` answers each question with the first rule whose kind prefixes the question's and
whose predicate holds, else a certain no, and records the questions. `FixedClock` is a clock
set by hand (`at`, `now`, `offset`); the pass's `Clock` trait is implemented for it there.
