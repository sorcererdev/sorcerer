# Conjure's corpora

The eval (`crates/conjure/tests/eval.rs`) runs each corpus's cases through `conjure_change` with the
writer's reply replayed from the corpus's `replies/` directory, and checks the result against the
corpus's passing scores. It costs nothing, calls no model and runs in CI. One `#[test]` a corpus.

| corpus | language | cases | test |
|---|---|---|---|
| [`bistro/`](bistro/README.md) | Python | 14: nonsense, stub, half, bug, syntax, no-test and green, two of each | `bistro_reaches_its_passing_scores` |
| [`bistro-rs/`](bistro-rs/README.md) | Rust | 12 of the same kinds, two asked from a scry finding | `bistro_rs_reaches_its_passing_scores` |

## What a corpus holds

| path | what it is |
|---|---|
| `corpus.toml` | `name`, `language`, `writer` (the `[conjure] writer` alias the replies were recorded with), `[passing_scores]`, and `[baseline]` once recorded |
| `base/` | the project every case starts from, as plain files |
| `cases/<id>.toml` | `file`, `line` (and `end_line` for a selection), `kind` (`nonsense`, `stub`, `half`, `bug`, `syntax`, `no-test`, `green`), `expect` (what a person would call the right next change), optional `finding` (a scry finding id to conjure for) and `also_in_scope` (extra paths the change may touch) |
| `cases/<id>.patch` | the person's uncommitted edit that makes the situation, applied over the committed `base/` (absent when the base is the situation) |
| `cases/<id>.findings.json` | a scry findings file (version 2) copied to `.sorcerer/state/scry/findings.json`, for a case asked from a finding |
| `replies/<id>.json` | the recorded reply: `{"model", "text", "cost_usd", "seconds"}` |
| `labels.json` | `{"forward": [ids]}`: the cases whose recorded change a person read and judged forward |
| `cases.md` | how each case was made, how a record becomes a case, and one line a case on how its recorded reply was labelled |

## The four scores

| score | what it counts |
|---|---|
| `forward` | cases whose id is in `labels.json` `forward` |
| `parses` | cases whose every changed file has no parse error |
| `in_scope` | cases whose changed files are the unit's file, its test file or a file in `also_in_scope` |
| `no_change_on_green` | green cases answered "no change", or whose changed files are only the unit's test file |

Passing scores are stated before a corpus is first recorded, and move only by a ruling written in its
`corpus.toml` with the date and the numbers beside it. When a corpus is first recorded without a
`forward_min`, it is set from that recording by the rule hits minus one.

## Running it

- `cargo test -p sorcerer-conjure --test eval -- --nocapture` replays every corpus and prints each
  table under `# <name>`; `… --test eval bistro_rs -- --nocapture` runs one.
- `CONJURE_EVAL_CASES=stub-create,bug-create` runs only those cases and asserts nothing: a look.
- `CONJURE_RECORD=all` (with `claude` on `PATH`, signed in) asks the live writer for each case with
  no `replies/<id>.json` and writes it; it prints a running total and stops once the total reaches
  `CONJURE_RECORD_CEILING` (dollars, default 0.50), keeping what it wrote, so a rerun asks only for
  the rest. It is refused when `CI` is set. Delete a reply to record that case again.
