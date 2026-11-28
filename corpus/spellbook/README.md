# Spellbook's corpora

Each corpus is a project plus the generators (and, later, rules) a person would keep in
`.sorcerer/spellbook/`, and cases that run the engine on it. `crates/spellbook/tests/corpus.rs`
runs them. It costs nothing, calls no model, touches no network and runs in CI.

| corpus | language | generators | tests |
|---|---|---|---|
| [`bistro/`](bistro/README.md) | Python | `command`, `query`, `port`, `use-port`, `add-port` | `bistro_holds_its_cases`, `bistro_check_cases_find_what_was_planted`, `bistro_holds_its_fix_cases`, `bistro_keeps_its_regions`, `bistro_adopts_check_on_old_code`, `bistro_load_mistakes_say_what_is_wrong` |
| [`bistro-rs/`](bistro-rs/README.md) | Rust | `command`, `port`, `use-port`, `add-port`, `repository` | `bistro_rs_holds_its_cases`, `bistro_rs_check_cases_find_what_was_planted`, `bistro_rs_holds_its_fix_cases`, `bistro_rs_keeps_its_regions`, `bistro_rs_adopts_check_on_old_code`, `bistro_rs_load_mistakes_say_what_is_wrong` |

## What a corpus holds

| path | what it is |
|---|---|
| `corpus.toml` | `name`, `language`, `base`: the project every case starts from, a path to another corpus's `base/`, and `overlay`: this corpus's own folder of files laid over it |
| `base/` | the overlay: the files that carry region markers (bistro-rs's five `mod.rs`, bistro's `commands/__init__.py`), laid over the shared base for every case unless the case says `base = "shared"` (the scry and conjure corpora run models on the shared base, so it is not touched) |
| `spellbook/` | copied to `.sorcerer/spellbook/` in a copy of the base: `generators/<id>/{generator.toml, templates/}`, `rules.toml`, `regions.toml` and `tables.toml` (the lookup tables a template reads, `{= kinds[f.kind].rust =}`) |
| `cases/new/<id>.toml` | a generator run: `generator`, `[inputs]`, and `[[holes]]` (`name`, `kind`, `file`, `line`), one for each place a hole is still open afterwards |
| `expected/new/<id>/<path>` | every file the run wrote or changed (a region's file among them), as a person would accept it: every placeholder in place, parsing |
| `cases/holes/<id>.toml` and `.patch` | a `new`, then the person's edit (the patch, applied with `git apply`) filling a hole; the `[[holes]]` are what stays open |
| `cases/check/<id>.toml` and `.patch` | a planted break (an `import sqlite3` in a command, an allow with and without a reason, an allow naming no rule, a `panic!` in a Rust command): the patch is applied over the base, every file is checked, and the findings equal `expected/check/<id>.json`; `clean` has no patch and no findings |
| `cases/sync/<id>.toml` and `.patch` | a command on the base (after the patch): `mode` (`refresh`, `init NAME`, `check`, `plan`) runs `spellbook sync` with it, or `command` runs any other words after `spellbook` (a `new` on broken markers); `exit` is its exit code. What it printed is `expected/sync/<id>.txt`, and every file it changed is under `expected/sync/<id>/` (no folder: it changed nothing) |
| `cases/adopt/<id>.toml` and `.patch` | check on code with debt: `committed` (patches applied, then the copy committed in a fresh git repository), `patches` (applied after: the branch's work), `commands` (run after `spellbook`, in order: `check --baseline`, `check --since HEAD`, `check` under a `[spellbook.rules]` patch) and `exit` (the last one's). What they printed, each after its `$ spellbook …` line, is `expected/adopt/<id>.txt` |
| `cases/load/<id>/` | a `spellbook/` overlay with one mistake, a `case.toml` (the generator and its inputs, for a mistake in `rules.toml` the `check` file to run it on, or `sync = true` for a mistake in `regions.toml`) and `expected.txt`: the sentence a newcomer reads |

## Adding a case

1. Write `cases/new/<id>.toml` with the generator and inputs (no `[[holes]]` yet).
2. Run `SPELLBOOK_BLESS=1 cargo test -p sorcerer-spellbook --test corpus -- --nocapture`: it writes
   `expected/` and prints the holes as TOML.
3. Read every written file line by line. The expectation is what a person would accept, not whatever
   came out: when a placeholder is wrong, fix the generator, not the expectation.
4. Paste the printed `[[holes]]` into the case, and run the test again without `SPELLBOOK_BLESS`.

A load case is the same: write the overlay and `case.toml`, bless to write `expected.txt`, and read the
sentence. It should say what is wrong and what to write.
