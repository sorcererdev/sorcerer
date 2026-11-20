# bistro: scry's Python corpus

The eval (`crates/scry/tests/eval.rs`, test `bistro_reaches_its_passing_scores`) runs 58
labelled cases over a small Python web app, bistro, through whole review passes with every model call replayed from the recordings here,
and scores what scry shows against the passing scores. It costs nothing and runs in CI.

The sets: 18 V-must (a change that breaks a rule the project must keep: a finding should be
shown), 8 B (a planted bug: shown), 18 T (a clean change: nothing shown), 8 N (a change a
reviewer might nitpick: nothing shown), 6 V-taste (breaks a rule of taste: shown is allowed,
not required).

| path | what it is, and how it was made |
|---|---|
| `corpus.toml` | the corpus for the eval: its language, sets, passing scores and the baseline they were set from. |
| `base/` | bistro as plain files: the app every case starts from. |
| `cases/<id>.toml`, `cases/<id>.patch` | one case each: its set, scenario, the rule it touches, the lines planted, what a reviewer should say, how it was made, and the files it changes; the patch is the change, applied to `base/` uncommitted. Written by hand and checked against `base/` when the corpus was built. |
| `scry.toml` | bistro's steering: the context and its 13 rules (`ports`, `web-commands`, `adapters-main`, `rules-in-commands`, `failures-returned`, `statuses-failures`, `queries-read`, `clock`, `repo-one-op`, `rule-tests`, `docstring`, `main-keywords`, `union-lines`). Converted on 2026-11-18 from the Markdown rules file the replies were recorded with (`git show ac87c44^:corpus/scry/steering.md`): the context is the paragraph between its title and `## Rules`, each `###` heading became a rule in order, its text the heading line then its body, every word kept; the ids are the ones the recorded replies already cite, so no recording's `rule` changed (the eval's `the_recorded_replies_cite_the_corpus_rule_ids` checks it: 33 of the 42 findings cite one, 9 cite none). |
| `labels.json` | per case: its set, the statements that count as a hit, and its changed files. |
| `reviews/<case>.json` | one recorded reviewer reply per case: what sonnet answered (2026-11-15), one reply per changed file merged into one per case. They were recorded under an older prompt wording (Markdown steering, the field `kind`); on 2026-11-18 the field `kind` was renamed `issue_type` in each, a field-name conversion and nothing else, so they replay through today's parser. On 2026-11-19 each was put in the batches form (`{"batches": [{"files", "reply", "cost_usd"}]}`, one batch holding the case's changed files and the old reply, unchanged) when the corpus moved from `corpus/scry/` to `corpus/scry/bistro/`, by a one-off script: for each case, `files` is `labels.json`'s `changed`, sorted. |
| `checks.json` | the recorded claim-check verdicts, by the statement checked (whitespace collapsed). |
| `jev/<key>.json` | Jev's recorded answers, one file per question, named by its `question_key`: all 44 recorded live by the eval (`SCRY_RECORD_JEV=1`, now `SCRY_RECORD=jev`) on 2026-11-18, from an empty directory, so it holds exactly the questions the eval asks. |

The cases, rules, labels, replies and verdicts were carried over from an earlier
measurement of the same reviewer (see the internals book, Scry, "Where scry came from"); the
script that converted them is in this repository's history (`scripts/port-scry-corpus`, last
run for commit `081e1ec`).

## Running it

See `../README.md`: `cargo test -p scry --test eval -- --exact bistro_reaches_its_passing_scores --nocapture` runs
this corpus alone.
