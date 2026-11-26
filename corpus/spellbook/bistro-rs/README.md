# bistro-rs: spellbook's Rust corpus

scry's bistro-rs base (`../../scry/bistro-rs/base`). `spellbook/generators/` holds `command`, `port` and
`use-port` (the fix's generator).
A generator writes the new file only: `pub mod cancel_table;` in `commands/mod.rs` is the person's in
milestone 1 (a region, milestone 2). Until it is added the new file is not compiled; once it is, the file
compiles (warnings allowed) and its test is red on `todo!("hole handle: …")`, as the convention intends.

`spellbook/rules.toml` holds `commands-no-panic` and `commands-use-ports` (a command or query that uses
`std::fs` instead of a port). (`.unwrap()` would be the obvious rule, but a command file holds its tests
and a test may unwrap; a rule cannot yet tell the two apart.)

`commands-use-ports` has one fix, `use-port`, the same shape as Python bistro's: the `use std::fs;` line
goes (or a statement using `std::fs::…` becomes a `stmt` hole saying what it did), the port's trait is
used, the struct keeps an `Rc<dyn Port>` field, `new` takes it, the struct literal in `new` sets it, and
the test helper's `Type::new(…)` call gets an `expr` hole for the in-memory adapter. The `pub mod` line
of a new command in `commands/mod.rs` stays hand-written until regions (milestone 2c).

Cases: `new/` (two), `holes/one-filled`, `check/` (clean and a `panic!` in a command), `fix/` (use-port on
an import, and on a `std::fs::write` statement, which leaves a `stmt` hole) and `load/` (a method named
with another language's separator, `CreateBooking.new`, which Rust spells `CreateBooking::new`). The layout and how to add a case are in
[`../README.md`](../README.md).
