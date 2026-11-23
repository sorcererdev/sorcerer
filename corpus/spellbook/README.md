# Spellbook's corpora

Each corpus is a project plus the generators (and, later, rules) a person would keep in
`.sorcerer/spellbook/`, and cases that run the engine on it. `crates/spellbook/tests/corpus.rs`
runs them. It costs nothing, calls no model, touches no network and runs in CI.

| corpus | language | generators | tests |
|---|---|---|---|
| [`bistro/`](bistro/README.md) | Python | `command`, `query`, `port` | `bistro_holds_its_cases`, `bistro_load_mistakes_say_what_is_wrong` |
| [`bistro-rs/`](bistro-rs/README.md) | Rust | `command`, `port` | `bistro_rs_holds_its_cases` |

## What a corpus holds

| path | what it is |
|---|---|
| `corpus.toml` | `name`, `language`, and `base`: the project every case starts from, a path to another corpus's `base/` |
| `spellbook/` | copied to `.sorcerer/spellbook/` in a copy of the base: `generators/<id>/{generator.toml, templates/}` |
| `cases/new/<id>.toml` | a generator run: `generator`, `[inputs]`, and `[[holes]]` (`name`, `kind`, `file`, `line`), one for each place a hole is still open afterwards |
| `expected/new/<id>/<path>` | every file the run wrote, as a person would accept it: every placeholder in place, parsing |
| `cases/holes/<id>.toml` and `.patch` | a `new`, then the person's edit (the patch, applied with `git apply`) filling a hole; the `[[holes]]` are what stays open |
| `cases/load/<id>/` | a `spellbook/` overlay with one mistake, a `case.toml` (the generator and its inputs) and `expected.txt`: the sentence a newcomer reads |

## Adding a case

1. Write `cases/new/<id>.toml` with the generator and inputs (no `[[holes]]` yet).
2. Run `SPELLBOOK_BLESS=1 cargo test -p sorcerer-spellbook --test corpus -- --nocapture`: it writes
   `expected/` and prints the holes as TOML.
3. Read every written file line by line. The expectation is what a person would accept, not whatever
   came out: when a placeholder is wrong, fix the generator, not the expectation.
4. Paste the printed `[[holes]]` into the case, and run the test again without `SPELLBOOK_BLESS`.

A load case is the same: write the overlay and `case.toml`, bless to write `expected.txt`, and read the
sentence. It should say what is wrong and what to write.
