# bistro-rs's cases

Each case starts from `base/` (a copy of scry's `corpus/scry/bistro-rs/base/`, so a change to scry's
corpus never moves these recordings), committed, and the person's edit applied over it, uncommitted
(`cases/<id>.patch`, a `git diff --cached` of the edit). A Rust file keeps its tests in itself
(`[test] tests_for` names the file), so a unit's test file is the unit's file. `expect` was written
before any recording. The two `bug-*` cases carry one open scry finding (`cases/<id>.findings.json`)
on the bug's line and are asked as *Conjure for this finding*.

**How a record becomes a case.** A line of `.sorcerer/state/conjure/changes.jsonl` names the file, the
line and the trigger (the cursor, or a scry finding id). Take the person's uncommitted diff at that
moment as `cases/<id>.patch`, the file and line as `cases/<id>.toml` (with `finding` when it was asked
from a finding, and the findings file as `cases/<id>.findings.json`), write `expect` by hand, then
record the reply fresh with `CONJURE_RECORD=all`.

**A case that could not be built as first written (2026-11-22, before any recording).**
`nonsense-create` first had the prose line `if the party is too big say so`: the `if` opens a block
the file never closes, the braces shift, tree-sitter loses the `handle` function altogether, and
conjure answers "put the cursor in a function to conjure" with no model call. That is a real limit of
finding the unit under a broken parse (a function the parser cannot see is not a unit), not a
writer's miss. The case keeps prose that does not open a block.

## The cases

| id | kind | cursor | the edit |
|---|---|---|---|
| `bug-create` | bug | `create_booking.rs:51` | a bug in a complete body, with an open scry finding on its line (asked as Conjure for this finding) |
| `bug-schedule` | bug | `in_memory.rs:42` | a bug in a complete body, with an open scry finding on its line (asked as Conjure for this finding) |
| `green-create` | green | `create_booking.rs:46` | no edit: the body is complete and its tests pass |
| `green-schedule` | green | `get_day_schedule.rs:27` | no edit: the body is complete and its tests pass |
| `half-create` | half | `create_booking.rs:46` | the body cut after the table lookup |
| `nonsense-create` | nonsense | `create_booking.rs:46` | the body replaced by two prose lines (or stray statements) |
| `nonsense-schedule` | nonsense | `get_day_schedule.rs:27` | the body replaced by two prose lines (or stray statements) |
| `notest-slot` | no-test | `slot.rs:55` | the file's `mod tests` removed |
| `stub-create` | stub | `create_booking.rs:46` | the body is `todo!()` |
| `stub-schedule` | stub | `get_day_schedule.rs:27` | the body is `todo!()` |
| `syntax-create` | syntax | `create_booking.rs:71` | a missing `;` or `}` |
| `syntax-slot` | syntax | `slot.rs:83` | a missing `;` or `}` |

## Labels, from the first recording (2026-11-22, haiku)

Read by the builder against `expect` and the anti-goals (a rewrite of a green unit is not forward; a test that enshrines a bug is not forward). The user reads them later: a label overturned is a ruling with the numbers beside it.

- `bug-create`: forward. `>=` back to `>`, nothing else (asked from the finding).
- `bug-schedule`: forward. Put `.filter(|b| b.slot.day == day)` back between `iter()` and `cloned()` (asked from the finding).
- `green-create`: forward. Answered nothing.
- `green-schedule`: forward. Answered nothing.
- `half-create`: forward. Continued after the table lookup with the party check, the taken slot, the booking and its save.
- `nonsense-create`: forward. Turned the two prose lines into the whole body, with the project's own failure messages.
- `nonsense-schedule`: forward. Replaced the prose and `let x = 12;` with the `DaySchedule` of the day's tables and bookings.
- `notest-slot`: NOT forward. Answered `nothing`; the expected change is a new `mod tests` in `slot.rs`. (The first recording of this case, 2026-11-22, answered by deleting a stray `}` my patch had left at the end of the file, a case-building mistake: the patch cut lines 129-155 of a 156-line file. The patch was corrected and the case recorded again; the first reply is not kept.).
- `stub-create`: forward. Wrote the whole body in the expected order.
- `stub-schedule`: forward. Built the `DaySchedule` from `tables()` and `bookings_on(day)`.
- `syntax-create`: forward. Added the `;`, nothing else.
- `syntax-slot`: forward. Put the missing `}` back, nothing else.
