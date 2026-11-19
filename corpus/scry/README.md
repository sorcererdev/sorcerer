# The scry corpus

The eval (`crates/scry/tests/eval.rs`) runs 58 labelled cases over a small Python web app,
bistro, through whole review passes with every model call replayed from the recordings here,
and scores what scry shows against the passing scores. It costs nothing and runs in CI.

The sets: 18 V-must (a change that breaks a rule the project must keep: a finding should be
shown), 8 B (a planted bug: shown), 18 T (a clean change: nothing shown), 8 N (a change a
reviewer might nitpick: nothing shown), 6 V-taste (breaks a rule of taste: shown is allowed,
not required).

| path | what it is, and how it was made |
|---|---|
| `base/` | bistro as plain files: the app every case starts from. |
| `cases/<id>.toml`, `cases/<id>.patch` | one case each: its set, scenario, the rule it touches, the lines planted, what a reviewer should say, how it was made, and the files it changes; the patch is the change, applied to `base/` uncommitted. Written by hand and checked against `base/` when the corpus was built. |
| `steering.md` | bistro's rules, the steering the reviewer was given when the replies were recorded. |
| `labels.json` | per case: its set, the statements that count as a hit, and its changed files. |
| `reviews/<case>.json` | one recorded reviewer reply per case: what sonnet answered (2026-11-15), one reply per changed file merged into one per case. |
| `checks.json` | the recorded claim-check verdicts, by the statement checked (whitespace collapsed). |
| `jev/<key>.json` | Jev's recorded answers, one file per question, named by its `question_key`. 43 recorded live by the eval (`SCRY_RECORD_JEV=1`, 2026-11-18); 3 (`246641af322a93f4`, `28d40b419ee79956`, `aa472c0c767b131a`) carried over from the original measurement's recording (same keys), for the questions the same-pass merge rule asks; `5988dcc32dcd0b79` and `8d40d48fd13b0094` are live answers the eval no longer asks. |

The cases, steering, labels, replies and verdicts were carried over from an earlier
measurement of the same reviewer (see the internals book, Scry, "Where scry came from"); the
script that converted them is in this repository's history (`scripts/port-scry-corpus`, last
run for commit `081e1ec`).

## Running it

- `cargo test -p scry --test eval -- --nocapture` runs every case and prints the table.
- `SCRY_EVAL_CASES=b01,vm02` runs only those cases and prints each stored finding and every
  question Jev is asked, without asserting the passing scores: a look, not a measure.
- `SCRY_EVAL_LOG=DIR` writes each case's run log (scope, review calls, findings, rounds) to
  `DIR/<id>.md`.
- `SCRY_RECORD_JEV=1` with `JEV_API_KEY` set asks Jev the questions that have no recording
  and writes them under `jev/`: a paid call (a fraction of a cent a question), run by hand,
  never in CI. Delete a file to ask its question again.

## Building a corpus

Material for building a corpus (generation scripts, the conventions it was written against,
case plans) lives beside it, under `corpus/scry/<name>/`, so anyone can rebuild or extend it
from the repository alone.
