# Sorcerer

A suite of small tools that read your code alongside you: **scry** (a reviewer in the
margin), **conjure** (one reviewed AI-written step), **spellbook** (conventions). They
share one core and compose through files in `.sorcerer/`.

**Status: M1, scry.** The core and the first tool are here; conjure, spellbook and plans
come later.

## Try scry

`cargo install --path crates/scry`, then in a git repository run `scry`: it reviews what your
working tree changed against HEAD, prints the findings that survive a claim check and a nitpick
screen, and exits 1 when any are open. Tell it your project's rules in `.sorcerer/scry.toml`
(`context`, `include`, and `[[rule]]`s with an `id` and `text`; findings cite a rule by its
id). `scry doctor` says whether it can run here; the user book's Scry chapter has the rest.

In an editor, `scry lsp` is a language server: save a file and each finding appears on its line,
a clean save shows nothing. Neovim (`sorcerer.nvim`) and VS Code (`editors/vscode/scry`) start it
for you; the user book's "Scry in your editor" chapter says how.

Scry's quality is measured by the eval (`cargo test -p sorcerer-scry --test eval`), which replays recorded
reviews of labelled cases in Python and Rust (`corpus/scry/`) and holds them to passing scores.

## Try the core

From inside any git repository with Python or Rust files:

```text
# a file's units, their fingerprints, and what changed against HEAD
cargo run -p sorcerer-core --example units -- src/orders.py

# one Jev question and one research errand through each provider you have
# (needs JEV_API_KEY, and claude on PATH or ANTHROPIC_API_KEY; spends a few cents)
cargo run -p sorcerer-models --example ask
```

The first run for a language fetches and compiles its tree-sitter grammar into
`~/.cache/sorcerer/grammars` (you need a C compiler).

## Books

- `book/user/`: using sorcerer (`mdbook serve book/user`)
- `book/internals/`: how it works, so you can change it (start with its Architecture
  chapter: every crate is in layers, checked by `scripts/layers`)

## License

Apache-2.0. Contributions carry a DCO sign-off; see `CONTRIBUTING.md`.
