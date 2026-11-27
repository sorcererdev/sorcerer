# The language corpora

One tiny project per shipped language, `corpus/languages/<name>/` (`<name>` is the folder under
`crates/sorcerer-core/library/languages/`). Four harnesses run every corpus, read only this data,
and list every language that fails; adding a language adds a corpus and no harness code. No model,
no network: the grammars come from `crates/sorcerer-core/tests/grammars/` (Python, Rust) or
`target/test-grammars/` (run `scripts/test-grammars` once).

| harness | checks |
|---|---|
| `crates/sorcerer-core/tests/languages.rs` | the grammar is on disk, every query compiles, every `base/` file of the language parses, the units equal `expected/units.txt` (a language without units has none) |
| `crates/spellbook/tests/languages.rs` | the `check` cases (`clean` finds nothing), the `new` cases (when the language has `[holes]`), the `sync` cases (when it has `comment`), the `fix` cases, and that the `new` and `fix` cases leave a hole of every `[holes]` kind |
| `crates/lackey/tests/languages.rs` | each `cases/regions/<id>.toml` cuts the regions in `expected/regions/<id>.txt` |
| `crates/conjure/tests/languages.rs` | a language with units: conjure asked in `[conjure] file` finds the test file and related files in `expected/related.txt` |

## What a corpus holds

| path | what it is |
|---|---|
| `corpus.toml` | `name`, `language`, `base = "base"`; a language with units adds `[conjure] file`, the file conjure is asked in |
| `base/` | 2 to 4 files: a code language has a model, a service with one stub, its test, one relative import; a config format one realistic file; Markdown a README with sections |
| `spellbook/` | laid in as `.sorcerer/spellbook/`: a generator `thing` (one hole of each `[holes]` kind), `rules.toml` (one rule, with a fix for a code language), `regions.toml` (one region, where the language has comments; its markers already set up in `base/`) |
| `cases/check/clean.toml`, `cases/check/planted.toml` + `.patch` | as in `corpus/spellbook/` |
| `cases/new/thing.toml`, `cases/fix/<id>.toml` + `.patch`, `cases/sync/init.toml` + `unmarked.patch` | as in `corpus/spellbook/`; `init` takes the markers out, then sets the region up again |
| `cases/regions/<id>.toml` + `.patch` | `file` and `patch`: the person's edit, cut by lackey |
| `expected/` | the goldens: `units.txt`, `regions/<id>.txt`, `related.txt`, and spellbook's `check/`, `new/`, `fix/`, `sync/` |

`python/` and `rust/` are the worked examples. Write the cases, then run the harnesses with
`LANGUAGES_BLESS=1` to write `expected/`, read every file line by line (the golden is what a person
would accept, not whatever came out), paste the printed `[[holes]]` into the `new` and `fix` cases,
and run them again without it.
