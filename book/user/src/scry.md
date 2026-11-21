# Scry

Scry reads a change and says what it gets wrong. It looks at the code units a change adds or
alters, with the diff and your project's rules, and asks a model for problems the change has
or introduces: a named code path that does the wrong thing, not a hunch. An empty answer is
normal.

Each finding has an issue type, a place, a short statement and the code it cites. Scry checks the
citations, asks the claim checker whether the claim holds, and asks Jev whether it is a
nitpick. Only what survives is shown, at most 3 in a file at a time. The rest is kept, not
lost: a later pass can raise a finding again.

Findings stay in `.sorcerer/state/scry/findings.json` until the code changes or you dismiss
them. The file is versioned: one an older scry wrote (version 1) is not read or
changed; scry exits 2 naming it, and you move it aside to start over. Scry spends money, so it stops at a per-pass and a per-day cap.

## Running scry

```
scry                       # the units your working tree changed against HEAD
scry src/orders.py …       # the changed units of these files (--all: every unit)
scry --since main          # what your branch changed since it left main
scry --json                # the findings as JSON, one object a line
scry --fresh               # look again at units already looked at as they are now
scry --budget 0.50         # spend at most this much on this run
scry doctor                # whether scry can run here
scry list [--status S]...  # every finding in the store, open first (--json)
scry cost [--since DAY]    # what scry spent, a local day a row (--json)
scry dismiss 7f3a2c        # stop showing a finding (--everywhere: in every unit)
scry why 7f3a2c            # a finding in full
scry -v                    # a run, its log streamed to stderr as it goes
scry log [N]               # the latest run's log (2: the one before); --list, --path
scry trust                 # let the editor review this project (--revoke: no longer)
scry lsp                   # the editor's language server (your editor starts it)
```

A run looks at the units in scope it has not already looked at as they are now, then prints
every open finding placed in a unit in scope, best first, the ones found earlier included. So
running `scry` twice prints the same findings twice, and the second run costs nothing:

```
src/orders/cancel.py:41  bug  cancel() returns None when the repository has no order with that id; the
                              caller in api/orders.py:88 indexes the result.  (0.91)
  evidence: src/orders/cancel.py:41-44, src/api/orders.py:86-90
  scry why 7f3a2c
1 finding, 2 units looked at, $0.05
```

Each finding is its place, its issue type and its statement, wrapped to your terminal (100 columns
when the output is not a terminal), then its confidence: one minus Jev's chance that it is a
nitpick. Under it are the lines it cites and the command that shows it in full. The last line
counts the findings, the units sent to the reviewer this run and what the run spent; a run that
had nothing to look at and nothing to show prints nothing.

With `--json`, each finding is one line on stdout:
`{"id", "issue_type", "rule", "statement", "file", "lines": [first, last], "unit", "evidence":
[{"file", "lines"}], "confidence", "verdict", "status"}`. The summary line goes to stderr.

Problems go to stderr, one line each: a review call that failed (`scry: review failed for
src/a.py: …`; its units are looked at again next run), and units or a claim check a cap held
back (`scry: the day's cap stopped 2 units; they wait for the next run`).

A file named like a command (`list`, `cost`, `dismiss`, `why`, `doctor`, `log`, `trust`,
`lsp`) is passed as
`./list`. `--since` takes no files.

### Looking at the store

`scry list` prints a table, `status  issue type  file  line  rule  statement` (`-` when no rule, the
statement cut to 80 characters), open findings first, then by file and line. `--status` (repeat
it) keeps only those statuses (`open`, `unverified`, `withheld`, `refuted`, `unsure`, `elsewhere`,
`dismissed-here`, `dismissed-everywhere`, `resolved`, `gone`). `--json` prints
`{"findings": [{"id", "status", "issue_type", "file", "line", "rule", "statement", "confidence",
"verdict", "seen"}]}`. `waiting`, the word older scry used for `unverified`, still works with `--status`, and a store an older scry wrote reads as before.

`scry cost` reads the ledger: a row for each of your local days, `day  reviews  checks  review $
check $  total $`, then `waiting on the cap: N units`. Jev's questions are in the total only.
`--since 2026-11-01` leaves earlier days out; `--json` prints `{"days": [...],
"waiting_units": N}`. When the finding store does not read, `cost` says so on stderr and
leaves the waiting line out (`waiting_units` is `null`).

`scry dismiss ID` takes the id or a unique prefix of four or more characters and prints
`dismissed 7f3a2c (here)`; a dismissed finding is not shown again in that unit, or in any with
`--everywhere`. `scry why ID` prints Markdown: the statement, the rule cited and its text, the
unit and the line it starts on, each cited range with its code (two lines either side; a
citation outside the project or on a fenced path, such as `.env`, is named but its code is not
printed), the claim checker's verdict and, under `Cited by the check:`, each citation it
gave with `confirmed`, `not found`, or `unit not found, confirmed on line N`, the
confidence, how often
it was seen and its history. An unknown or ambiguous id exits 2. None of these four needs Jev.

### Reading what a run did

Every run (not `list`, `cost`, `dismiss`, `why`, `doctor` or `log`, and not a run with scry
off) writes its log, a Markdown file, to `.sorcerer/state/scry/runs/`, named for when it
started in UTC: `2026-11-18T23-00-00Z.md` (`-2`, `-3` when two start in the same second).
The newest 20 are kept. A log that cannot be written is one line on stderr; the run's output
and exit code do not change. The log holds, in run order:

- **Run**: the time (local and UTC), the project, scry's version, the command line, the caps
  and the effective `[scry]` settings.
- **Steering**: what was understood from `.sorcerer/scry.toml` (see Steering).
- **Scope**: each file considered, in (with its added or changed units) or skipped and why (no
  language, excluded, fenced, not text, deleted).
- **Pass** and **Units**: the caps, what scry spent earlier today, the cost estimates, and each
  unit: to send, or already reviewed at its fingerprint.
- **Review calls**: per batch, its units, the model and cap, the whole prompt and the whole reply,
  the call's cost and time, and what the reply parsed to (or why it did not), or that a cap
  held the batch back.
- **Findings**: each finding as the model gave it (issue type, place, rule, statement, evidence),
  the rule id it cited and whether the steering has it, the unit it was placed in, each same-problem question with Jev's
  P(yes) and what came of it (new, merged, dropped), the nitpick question's P(yes), the
  confidence against `confidence_threshold`, and the id it got.
- **Remember**: what was resolved (the reviewer said it no longer holds, or it was not found
  again) or gone.
- **Verification**: per file and round, the candidates and the room, each finding's citations
  (holding, or which did not), and each claim check: the models, whether it escalated, the
  verdict and reason, every citation the check gave with `confirmed`, `not found` or `unit not
  found, confirmed on line N`, the cost,
  and the status that followed.
- **Reviewed**: the units marked looked at.
- **Summary**: each finding the run added or changed, its status and why it is or is not
  printed; what was spent, by verb; failures; and a last line, `**Result:** 1 finding shown,
  2 units looked at, $0.05, exit 1`.

`scry -v` (`--verbose`) streams the same text to stderr as the run goes; stdout and the exit
code are as without it, so `scry -v --json` still prints only JSON on stdout. `scry log`
prints the latest log, `scry log 2` the one before, `scry log --list` the logs kept, newest
first (file, local time and the result line), and `scry log --path` the latest's path. With no
log yet, it says so on stderr and exits 2.

A finding the checker could not confirm is `unsure` and not shown. To see why:

```
$ scry list --status unsure
status  issue type  file             line  rule  statement
unsure  bug         src/orders.rs    41    -     cancel returns None when the order is missing
$ scry why 7f3a
…
Checked: cannot_tell: cited code not found
Cited by the check:
src/orders.rs Orders.cancel line 41: confirmed
src/orders.rs Order.refund: not found
```

The checker named a unit, `Order.refund`, that the file does not define, and gave no line, so
it could not confirm the claim, and an unconfirmed claim is never shown. Had it given a line
inside the file, the citation would stand on that line (`src/orders.rs Order.refund line 52:
unit not found, confirmed on line 52`) and the verdict would be kept: a name the checker got
wrong does not hide a real bug. A citation on a fenced path, outside the project, or past the
end of its file never confirms. `scry log --list` finds the run
that checked it, and its **Verification** section shows the same check with the models it
went through and what it cost.

### Exit codes

| code | means |
|---|---|
| 0 | nothing open in scope, or scry is off (`[scry] enabled = false`) |
| 1 | open findings in scope |
| 2 | scry could not run: no Jev key, no provider, the store does not read, a bad `--since` rev; or nothing is open but a review call failed or a cap held work back |

So a pre-commit hook is one line, in `.git/hooks/pre-commit`:

```sh
scry || exit $?
```

### Doctor

`scry doctor` checks, a row each, what a run needs, and writes nothing:

- `provider`: the model provider scry would use, and why (`[model] provider`, else `claude`
  on PATH, else `ANTHROPIC_API_KEY`). Scry runs Claude Code itself as the reviewer when
  the provider is `claude`.
- `jev`: Jev's key (`JEV_API_KEY`). Scry does not run without Jev.
- `languages`: each language your files use (or `[languages] names`) loads its grammar.
- `steering`: what `.sorcerer/scry.toml` holds (rules, includes, the context's size), or
  why it does not read.
- `store`: the finding store reads (one from a newer scry does not, and is left alone).
- `ledger`: the cost ledger reads.
- `trust`: whether the editor will review the project: `not needed`, `trusted`, or `not
  trusted: the editor will ask (…)` with what in the config needs trust. Never a failure: the
  command line runs either way.

It exits 2 when any row fails.

### Trust

Some settings in a project's own files run code or move your API key: a provider's `runner`
(a program scry starts), a provider's `url` (where your code and your key are sent), and a
grammar the project's `.sorcerer/languages/<name>/language.toml` names (C source scry compiles
and loads). Your editor starts scry by itself in whatever folder you open, so its server does
not read, parse or review a project that sets any of them until you trust it. A project that
sets none needs no trust: that is every project with no `.sorcerer/` directory, and most with
one.

- The editor asks once a session, at the first save in such a project: *Trust* or *Not now*.
- `scry trust` in the project trusts it from a terminal, and prints what in its config needed
  it (`trusted: /home/you/shop`, then `[model.providers.mine] runner = /opt/run`). It trusts
  the root even when nothing needs trust today. `scry trust --revoke` takes it off the list.
- The list is `trusted.toml` in `$XDG_CONFIG_HOME/sorcerer/`, else `~/.config/sorcerer/`
  (on Windows, `%APPDATA%\sorcerer\`). It
  holds project roots: trust is by path, so a change you pull into a trusted project is
  trusted, as its build scripts are.

The command line itself never asks: typing `scry` in a project is the consent.

## What a pass looks at

A pass looks at the units a change adds or alters, compared with a base:

- `scry` alone: every file that differs from `HEAD` in your working tree, untracked files too.
- `scry <file>…`: the files you name, against `HEAD`. (Saving a file in the editor is the
  same, for that file.)
- `--all`: every unit of each file in scope, not only the changed ones.
- `--since <rev>`: the files your branch changed since it left `<rev>`. Scry compares against
  the merge base, the commit where your branch and `<rev>` last agree, so changes made on
  `<rev>` after you branched are not yours and are not reviewed.

If you undo a change so that a unit is again what the base has, a finding scry raised on
that change is resolved at the next pass that covers the file, with no model call: the code
it was about is gone. If you commit the change instead, the finding stays open, since the
committed code still has the problem.

A file nobody committed yet, or any file in a repository with no commit, is all new: every
unit is in scope. A renamed file is a new file at its new path. Scry leaves out files no
language covers, files matching `exclude`, fenced files (`.env*`, `.sorcerer/`, your
`[model] not_evidence`) and files that are not text.

## Issue types

| issue type | means | shown by default |
|---|---|---|
| `bug` | the code does the wrong thing | yes |
| `assumption` | the code relies on something about other code that is not so | yes |
| `convention` | the code breaks a rule in your steering | yes |
| `design` | readability, maintainability, testability or architecture with no rule behind it | no |

## Configuration

The `[scry]` table in `.sorcerer/config.toml`. An unknown key, or a value out of range, is an
error that names the key and the file.

| key | default | meaning |
|---|---|---|
| `enabled` | `true` | turn scry off for the project: `scry` then looks at nothing, says so on stderr and exits 0, even with no Jev key or provider |
| `issue_types` | `["bug", "assumption", "convention"]` | the issue types shown |
| `review_model` | `"sonnet"` | the model that reviews the change |
| `confidence_threshold` | `0.7` | 0 to 1: a finding is shown only when its confidence (one minus Jev's chance that it is a nitpick) is at least this |
| `budget_per_pass` | `0.25` | dollars one pass may spend (the editor's cap) |
| `budget_per_day` | `3.00` | dollars scry may spend on the project in a local day |
| `exclude` | `[]` | globs of files scry skips |

## What it costs

A pass that finds changed units not yet reviewed makes one review call for them, about $0.04
on sonnet (about $0.005 with `review_model = "haiku"`, which misses more). Each `bug` or
`assumption` that survives the nitpick question gets a claim check, about $0.016. A pass with
nothing new to look at costs nothing. Jev's questions cost about $0.00005 each and are not
counted against the caps. Once the ledger has priced calls, scry estimates from the last 20
of each.

Scry checks the caps before every paid call, against what that call is likely to cost:

- `budget_per_pass`: what one pass may spend.
- `budget_per_day`: what scry may spend in your local day, counting the passes before this one.
- `--budget USD` on the command line: what the run may spend.

A review call that times out (after 420 s) is counted at the cap it ran under, which is never
more than `budget_per_pass`, even on the command line where no pass cap applies: one hung call does
not use up the day.

A unit a cap holds back is not reviewed; scry records it and the next pass with room takes
it. A finding whose claim check does not fit stays unverified, and is checked on a later pass. Findings
are checked best first, and only until 3 are open in the file: a claim the checker
refutes frees its place for the next one.

## Steering

Steering is what the reviewer is told about your project. It lives in `.sorcerer/scry.toml`
(commit it), which has exactly three keys:

- `context`: prose about the project the reviewer should know: what it is, how it is laid out.
- `include`: files read in as more context, in the order listed: paths relative to the
  project root, such as `docs/conventions.md`. Not globs.
- `[[rule]]`: a rule the project keeps, each an `id` and its `text`.

```toml
context = """
Bistro is a booking service in ports and adapters: commands and queries go
through ports, and adapters hold no logic."""

include = ["docs/conventions.md"]

[[rule]]
id = "result"
text = """
Commands return a Result (must)
A command never panics; every failure comes back as an error the caller can show."""

[[rule]]
id = "test-names"
text = "A test name says what happens, not which function it calls."
```

An id is lowercase letters, digits and hyphens, starts with a letter, has no double or
trailing hyphen, and is at most 40 characters: `result`, `web-commands`, `clock`. Ids are
unique in the file.

The reviewer is shown the context, each include, then the rules as `- id: text`, and asked to
cite a rule by its id. A finding that cites a rule is usually a `convention`, and the rule's
text goes to Jev with it when Jev judges whether the finding is a nitpick (a finding that
cites no rule goes with all the rules). A finding that cites an id the file does not have (or
the rule's words instead of its id) keeps its statement and loses the rule; the run log says
so. The two sides of Jev's nitpick question are fixed: a nitpick is about taste or polish;
worth saying is what leaves wrong behaviour, a failure, lost data, a security hole or a real
maintenance cost.

Steering is configuration, so a mistake is an error, not a warning: scry exits 2 naming
`.sorcerer/scry.toml` and the key. An unknown key, a malformed or repeated id, an empty
`text` or include, and an include that is missing, not text, absolute, outside the project or
fenced (such as `.env` or anything under `.sorcerer/`) are each refused. The rendered steering
may be at most 32 KB (about 8,000 tokens, sent with every review call); over that is an
error naming its size and its largest part, and nothing is cut. With no `scry.toml`, the
reviewer gets no steering, which is fine.

A `.sorcerer/scry.md` (an older scry's steering) is an error whenever it is there: move its
rules into `scry.toml` and delete it.

`scry doctor`'s `steering` row says what was understood (`2 rules, 1 include, context 120 B,
1.4 KB; .sorcerer/scry.toml`), and each run log has a **Steering** section after the run's
head: the file, the context's size, each include, the rule ids and the size against the cap.
