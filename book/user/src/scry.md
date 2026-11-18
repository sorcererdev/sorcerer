# Scry

Scry reads a change and says what it gets wrong. It looks at the code units a change adds or
alters, with the diff and your project's rules, and asks a model for problems the change has
or introduces: a named code path that does the wrong thing, not a hunch. An empty answer is
normal.

Each finding has a kind, a place, a short statement and the code it cites. Scry checks the
citations, asks the claim checker whether the claim holds, and asks Jev whether it is a
nitpick. Only what survives is shown, at most `max_shown` at a time. The rest is kept, not
lost: a later pass can raise a finding again.

Findings stay in `.sorcerer/state/scry/findings.json` until the code changes or you dismiss
them. Scry spends money, so it stops at a per-pass and a per-day cap.

## Kinds

| kind | means | shown by default |
|---|---|---|
| `bug` | the code does the wrong thing | yes |
| `assumption` | the code relies on something about other code that is not so | yes |
| `convention` | the code breaks a rule in your steering file | yes |
| `design` | readability, maintainability, testability or architecture with no rule behind it | no |

## Configuration

The `[scry]` table in `.sorcerer/config.toml`. An unknown key, or a value out of range, is an
error that names the key and the file.

| key | default | meaning |
|---|---|---|
| `enabled` | `true` | turn scry off for the project |
| `kinds` | `["bug", "assumption", "convention"]` | the kinds shown |
| `find_model` | `"sonnet"` | the model that looks for problems |
| `cut` | `0.7` | the keep line, 0 to 1: a finding is shown only when Jev's keep score (one minus the chance it is a nitpick) is at least this |
| `max_shown` | `3` | findings shown at once, at least 1 |
| `settle` | `2` | how long a finding must keep holding before it is shown, in passes (0 or more) |
| `pass_cost` | `0.25` | dollars one pass may spend |
| `day_cost` | `3.00` | dollars a local day may spend on the project |
| `exclude` | `[]` | globs of files scry skips |

## Steering

Steering is the text the finder reads about your project. It has three layers, joined in
order: the text shipped with sorcerer, then `.sorcerer/scry.md` in your project, then (once
spellbook exists) the rules spellbook enforces.

In `scry.md`, each `###` heading under `## Rules` is a rule a finding can cite. A finding that
cites a rule is a `convention`, and the rule's text goes to Jev with it. `## A nitpick is` and
`## Worth saying is` replace the two sides of Jev's nitpick question; the last one written
wins, and without them the shipped sides apply.

```markdown
## Rules

### Commands return a Result (must)
A command never panics; every failure comes back as an error the caller can show.

### Tests name the behaviour (should)
A test name says what happens, not which function it calls.

## A nitpick is

it is about naming, wording or formatting that no reviewer would block a merge over
```
