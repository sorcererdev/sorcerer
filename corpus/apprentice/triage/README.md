# Apprentice's triage corpus

The eval (`crates/apprentice/tests/triage_eval.rs`) cuts each case with the real grammars, runs
`triage_change` with Jev's answers replayed from `jev/`, and checks `corpus.toml`'s passing
scores. It costs nothing and runs in CI.

| path | what it is |
|---|---|
| `cases.toml` | 83 cases, one region each: the scenario, the file, `prior` edits (typed and looked at earlier: in the session) and the `edit` of the last look, each `[find, replace]`; the `region` as the cutter cuts it; the `label` (`instruction`, `question`, `claim`, `ask` for any of them, `none`); `hard` for a negative built to look like an ask; `held_out` for the 12 cases written after the wording was chosen |
| `scenarios/<name>/files/` | the files the cases edit, as the session left them |
| `scenarios/<name>/session.patch` | the session's changes from the merge base: `docs` is sorcerer's crate-readmes commit, `py` and `rs` are scry's bistro "cancel a booking" cases (t09, t05) |
| `jev/<key>.json` | Jev's recorded answers, one per question, by `question_key` |
| `tools/measure.py`, `tools/score.py` | the measurement of the context strategies against live Jev (2026-11-27) |

## Running it

- `cargo test -p sorcerer-apprentice --test triage_eval -- --nocapture` prints the score and every
  case it got wrong; `APPRENTICE_EVAL_VERBOSE=1` prints every case.
- `APPRENTICE_RECORD=jev` (with `JEV_API_KEY`) asks the live Jev what has no recording and deletes
  the recordings no case asks any more: about a cent for the whole corpus, run by hand, refused
  when `CI` is set. After changing the questions or the context, delete `jev/` and record again,
  then set the passing scores from the new table.
- `python3 -I tools/measure.py RUN_DIR [STRATEGY ...]` (from this directory, with `JEV_API_KEY`)
  asks live Jev each case under each strategy (`a` the region alone, `b` with the lines around and
  the section, `c` and the latest change, `d` and the session's changes, `e` two requests: Jev
  picks the changes that matter, then classifies; a suffix picks the wording, `6` the shipped
  one) and writes the answers; `python3 -I tools/score.py RUN_DIR [KIND_CUT ADDRESSED_CUT]` scores
  them with the old rule. The report with the table is
  `notes/durandal/apprentice-triage-context-report-2026-11-27.md`.

The labels are the author's: a claim stated as a comment (`# this never raises`) and an
explaining comment are a soft border, and Jev mostly reads both as the file's own content.
