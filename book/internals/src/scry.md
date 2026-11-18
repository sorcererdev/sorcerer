# Scry

`crates/scry`: the code reviewer, ported from olifant's `inspect`. Library `scry`, binary
`scry`.

## Module map

- `src/config.rs`: `ScryConfig`, `scry_config`.
- `src/kind.rs`: `Kind`, `Verify`, `KINDS`, `kind`.
- `src/steering.rs`: `Steering`, `steering_of`, `steering`.
- `library/scry.md`: the shipped steering layer.

Later tasks add the store, the find call, the judge, the pass and the CLI here.

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
