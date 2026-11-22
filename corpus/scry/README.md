# Scry's corpora

The eval (`crates/scry/tests/eval.rs`) runs each corpus's labelled cases through whole review passes
with every model call replayed from the recordings in the corpus's directory, and checks the result
against the corpus's passing scores. It costs nothing and runs in CI. One `#[test]` a corpus.

| corpus | language | cases | test |
|---|---|---|---|
| [`bistro/`](bistro/README.md) | Python | 58: 18 V-must, 8 B, 18 T, 8 N, 6 V-taste | `bistro_reaches_its_passing_scores` |
| [`bistro-rs/`](bistro-rs/README.md) | Rust | 32: 10 V-must, 6 B, 10 T, 6 N | `bistro_rs_reaches_its_passing_scores` |

The sets: **V-must** (a change that breaks a rule the project must keep: a finding should be shown),
**B** (a planted bug: shown), **T** (a clean change: nothing shown), **N** (a change a reviewer might
nitpick: nothing shown), **V-taste** (breaks a rule of taste: shown is allowed, not required).

## What a corpus holds

| path | what it is |
|---|---|
| `corpus.toml` | `name`, `language` (the one `[languages]` names in each case's project), `sets` (the table's rows, in order), `[passing_scores]` (`v_must_min`, `b_min`, `t_mean_max`, `n_mean_max`), `[baseline]` (the recording the passing scores were set from), and `[bare]` (`sets`, its own `[bare.passing_scores]` with no `v_must_min`, `[bare.baseline]`). A corpus with no `[passing_scores]` fails its test after printing its table. |
| `base/` | the project every case starts from, as plain files |
| `cases/<id>.toml`, `cases/<id>.patch` | one case each: the patch is the change, applied to `base/` uncommitted |
| `scry.toml` | the steering the reviewer reads |
| `labels.json` | per case: its set, the statements that count as a hit, the `unplanted` ones (true findings nobody planted, left out of the shown count) and its changed files |
| `reviews/<case>.json` | the recorded review replies: `{"batches": [{"files": [...], "reply": "...", "cost_usd": …}]}`. Replayed by file: a batch's reply is every recorded finding (over all the case's batches) whose `file` is one of the batch's files, so a change in batching still replays. A case with no file, or a batch with a file no recorded batch holds (a recording stopped part way), fails the eval. |
| `bare/` | the same recordings (`reviews/`, `checks.json`, `jev/`) made with no steering, and `labels.json` (per case, the exact statements of the bare findings that are hits). Read by the bare table only |
| `checks.json` | the checker's recorded reply by the statement checked (whitespace collapsed): `verdict` (`right`, `wrong`, `cannot_tell`), `why` and the `evidence` it cited |
| `jev/<key>.json` | Jev's recorded answers, one file per question, named by its `question_key` |

## Running it

- `cargo test -p sorcerer-scry --test eval -- --nocapture` runs every corpus and prints each table under
  `# <name>`; `cargo test -p sorcerer-scry --test eval bistro_rs -- --nocapture` runs one.
- `SCRY_EVAL_CASES=b01,vm02` runs only those cases (of the corpus whose test runs) and prints each
  stored finding and every question Jev is asked, without asserting the passing scores: a look, not
  a measure.
- `SCRY_EVAL_LOG=DIR` writes each case's run log (scope, review calls, findings, rounds) to
  `DIR/<id>.md`.
- `SCRY_RECORD=all` (with `JEV_API_KEY` and `claude` on `PATH`) records what a corpus lacks: the
  review of a case with no `reviews/<case>.json`, the claim check of a statement not in
  `checks.json`, and Jev's answers; name one corpus (`… --test eval bistro_rs`). It prints a running
  total a case and stops past `SCRY_RECORD_CEILING` (default 5, claude-reported USD); recorded cases
  are kept, so a rerun asks only for the rest.
- `SCRY_RECORD=checks` (same keys) asks every claim check of the cases run again and overwrites its
  entry in `checks.json`; reviews and Jev replay. Name the cases with `SCRY_EVAL_CASES`.
- `SCRY_RECORD=jev` with `JEV_API_KEY` set asks Jev the questions that have no recording and writes
  them under `jev/`: a paid call (a fraction of a cent a question), run by hand, never in CI (the eval
  refuses `SCRY_RECORD` when `CI` is set). Delete a file to ask its question again.

## Building a corpus

Material for building a corpus (generation scripts, the conventions it was written against, case
plans) lives beside it, under `corpus/scry/<name>/`, so anyone can rebuild or extend it from the
repository alone (`bistro-rs/tools/` is the harness the Rust corpus was built with).
