# bistro: spellbook's Python corpus

conjure's bistro base (`../../conjure/bistro/base`), a small Python restaurant app (`src/bistro/` with
`main.py`, a `booking` service and a `web` layer). `spellbook/generators/` holds `command`, `query` and
`port`, ported from olifant's python-htmx generators: each hole is a placeholder that parses (`todo_field`,
`object`, a `raise NotImplementedError("hole handle: …")` for `handle`), and `purpose`, `failure` and
`value` are derived.

`spellbook/rules.toml` holds the pack's four rules, `commands-use-ports` with two fixes (`use-port`,
`add-port`). `spellbook/regions.toml` declares one owned region, `commands-exports`: `commands/__init__.py`
re-exports each command module (`from .cancel_booking import CancelBooking`), tests left out. `base/` is the
overlay: that `__init__.py` with its markers, laid over the shared base for every case that does not say
`base = "shared"`, so a new command's `= …/__init__.py (exists, skipped)` is now `~ …/__init__.py (region
commands-exports: +1 −0)`.

Cases: `new/` (four), `holes/one-filled`, `check/` (clean, four planted breaks and `region-stale`), `fix/`,
`sync/` (`init-commands-exports` on the shared base, `hand-made-command`), `adopt/` (a baseline then
a check, `check --since HEAD` on a branch, a rule made a warning in `[spellbook.rules]`) and `load/`. The layout and how to add a case are
in [`../README.md`](../README.md).
