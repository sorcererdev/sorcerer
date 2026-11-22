# bistro's cases
Each case starts from `base/` (the demo at olifant's `fill` stage, built by
`scripts/slice-python.sh DIR gens`, `behaviour` and `fill`, with `.olifant/` taken out), committed, and
the person's edit applied over it, uncommitted. The edits are olifant's `make_cases.py` `CASES`, same ids,
kinds and lines (32, 31, 54, 20 are the cursor lines; the first line of the body of `handle`); `expect`
is olifant's reference text. A patch is `git diff --cached` of the edit over the base.

**How a record becomes a case.** A line of `.sorcerer/state/conjure/changes.jsonl` names the file, the
line and the trigger (the cursor, or a scry finding id). Take the person's uncommitted diff at that
moment as `cases/<id>.patch`, the file and line as `cases/<id>.toml` (with `finding` when it was asked
from a finding, and the findings file as `cases/<id>.findings.json`), write `expect` by hand, then
record the reply fresh with `CONJURE_RECORD=all`.

## The cases

| id | kind | unit | the edit |
|---|---|---|---|
| `bug-cancel` | bug | `cancel_booking.py:32` | a one-token bug in a complete body |
| `bug-create` | bug | `create_booking.py:54` | a one-token bug in a complete body |
| `green-cancel` | green | `cancel_booking.py:32` | no edit: the body is complete and its tests pass |
| `green-schedule` | green | `get_day_schedule.py:31` | no edit: the body is complete and its tests pass |
| `half-create` | half | `create_booking.py:54` | the body cut after its first statements |
| `half-schedule` | half | `get_day_schedule.py:31` | the body cut after its first statements |
| `nonsense-cancel` | nonsense | `cancel_booking.py:32` | the body replaced by prose or stray statements |
| `nonsense-schedule` | nonsense | `get_day_schedule.py:31` | the body replaced by prose or stray statements |
| `notest-cancel` | no-test | `cancel_booking.py:32` | the unit's test file is missing |
| `notest-count` | no-test | `count_day_bookings.py:20` | the unit's test file is missing |
| `stub-cancel` | stub | `cancel_booking.py:32` | the body is a `raise NotImplementedError` naming the test file |
| `stub-create` | stub | `create_booking.py:54` | the body is a `raise NotImplementedError` naming the test file |
| `syntax-cancel` | syntax | `cancel_booking.py:32` | a missing colon or parenthesis |
| `syntax-create` | syntax | `create_booking.py:54` | a missing colon or parenthesis |
