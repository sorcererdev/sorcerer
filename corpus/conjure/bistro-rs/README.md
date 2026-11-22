# bistro-rs: conjure's Rust corpus

12 cases of the same kinds as bistro (nonsense x2, stub x2, half, bug x2, syntax x2, no-test, green
x2) on a copy of scry's bistro-rs base; the two bugs are asked from a scry finding. `cases.md` says
how each was made. Run by `bistro_rs_reaches_its_passing_scores` in `crates/conjure/tests/eval.rs`;
the layout, the scores and how to record are in [`../README.md`](../README.md).

Passing scores (`corpus.toml`): `parses_min = 12`, `in_scope_min = 12`, `no_change_on_green_min = 2`,
stated on 2026-11-22 before any recording. `forward_min` is set from the first recording by the
corpus rule (hits minus one), with the date, since Rust was never measured.
