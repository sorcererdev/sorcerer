# bistro: conjure's Python corpus

olifant's 14 measured cases (`experiments/conjure-2026-11/make_cases.py`), ported: the table-booking
demo at its `fill` stage (every test green), with one person's edit a case. `cases.md` says how each
was made. Run by `bistro_reaches_its_passing_scores` in `crates/conjure/tests/eval.rs`; the layout,
the scores and how to record are in [`../README.md`](../README.md).

Passing scores (`corpus.toml`): `forward_min = 12` of 14 (haiku measured 14 of 14 in olifant on
2026-11-15; the rule is within ten points of the best), `parses_min = 14`, `in_scope_min = 14`,
`no_change_on_green_min = 2`. They were stated on 2026-11-22 before any recording.
