# bistro-rs: spellbook's Rust corpus

scry's bistro-rs base (`../../scry/bistro-rs/base`). `spellbook/generators/` holds `command`, `port`,
`use-port` (the fix's generator) and `repository`, whose `fields` is a list input (`item = "{name}:{kind}"`,
each kind checked against `spellbook/tables.toml`): its template loops over the items, writing one struct
field per item (`Option<…>` for an item marked `?`, a doc line on a `bool`) and `COLUMNS` joined with
`loop.last`.
A generator writes the new file only; its `pub mod` line is the `modules` region's (`spellbook/regions.toml`):
`modules` keeps one `pub mod` line per sibling file in every `src/{service}/{kind}/mod.rs` (commands,
models, queries and web/bookings), `ports` one per port folder in `ports/mod.rs`. `base/` is the overlay: those
five `mod.rs` files with their markers, laid over the shared base for every case that does not say `base =
"shared"`, so a new command's `mod.rs` line is in the same plan as its file, and the new file compiles at once
(its test red on `todo!("hole handle: …")`, as the convention intends).

`spellbook/rules.toml` holds `commands-no-panic` and `commands-use-ports` (a command or query that uses
`std::fs` instead of a port). (`.unwrap()` would be the obvious rule, but a command file holds its tests
and a test may unwrap; a rule cannot yet tell the two apart.)

`commands-use-ports` has two fixes. The first, `use-port`, is the same shape as Python bistro's: the `use std::fs;` line
goes (or a statement using `std::fs::…` becomes a `stmt` hole saying what it did), the port's trait is
used, the struct keeps an `Rc<dyn Port>` field, `new` takes it, the struct literal in `new` sets it, and
the test helper's `Type::new(…)` call gets an `expr` hole for the in-memory adapter. The second, `add-port`,
runs the `port` generator and then `use-port` in one plan (`[[uses]]`): the port's two files, its `pub mod`
line in `ports/mod.rs` (the `ports` region) and the rewiring.

Cases: `new/` (three: a command, a port, and a repository over four fields), `holes/one-filled`, `check/` (clean, a `panic!` in a command, `region-stale`), `fix/`
(use-port on an import, and on a `std::fs::write` statement, which leaves a `stmt` hole; add-port on an
import and on the write), `sync/` (set up on the shared base, up to date, a hand-made file, a deleted file, a hand edit in the
block, broken markers stopping `sync` and `new`, `--check`, `--plan`), `adopt/` (`check --since HEAD` on a branch, a rule turned off in
`[spellbook.rules]`) and `load/` (a method named with another
language's separator, `CreateBooking.new`, which Rust spells `CreateBooking::new`; five region mistakes; a `for` with no `end`; an item part the pattern lacks). The layout and how to add a case are in
[`../README.md`](../README.md).
