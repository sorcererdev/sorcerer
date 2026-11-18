# Sorcerer

A suite of small tools that read your code alongside you: **scry** (a reviewer in the
margin), **conjure** (one reviewed AI-written step), **spellbook** (conventions). They
share one core and compose through files in `.sorcerer/`.

**Status: M0, the core.** The libraries scry needs are here; scry itself arrives in M1.

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
- `book/internals/`: how it works, so you can change it

## License

Apache-2.0. Contributions carry a DCO sign-off; see `CONTRIBUTING.md`.
