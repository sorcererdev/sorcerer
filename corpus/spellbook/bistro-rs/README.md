# bistro-rs: spellbook's Rust corpus

scry's bistro-rs base (`../../scry/bistro-rs/base`). `spellbook/generators/` holds `command` and `port`.
A generator writes the new file only: `pub mod cancel_table;` in `commands/mod.rs` is the person's in
milestone 1 (a region, milestone 2). Until it is added the new file is not compiled; once it is, the file
compiles (warnings allowed) and its test is red on `todo!("hole handle: …")`, as the convention intends.

`spellbook/rules.toml` holds `commands-no-panic`. (`.unwrap()` would be the obvious rule, but a command
file holds its tests and a test may unwrap; a rule cannot yet tell the two apart.)

Cases: `new/` (two), `holes/one-filled` and `check/` (clean and a `panic!` in a command). The layout and how to add a case are in
[`../README.md`](../README.md).
