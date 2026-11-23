# bistro: spellbook's Python corpus

conjure's bistro base (`../../conjure/bistro/base`), a small Python restaurant app (`src/bistro/` with
`main.py`, a `booking` service and a `web` layer). `spellbook/generators/` holds `command`, `query` and
`port`, ported from olifant's python-htmx generators: each hole is a placeholder that parses (`todo_field`,
`object`, a `raise NotImplementedError("hole handle: …")` for `handle`), and `purpose`, `failure` and
`value` are derived.

Cases: `new/` (four), `holes/one-filled` and `load/` (six mistakes). The layout and how to add a case are
in [`../README.md`](../README.md).
