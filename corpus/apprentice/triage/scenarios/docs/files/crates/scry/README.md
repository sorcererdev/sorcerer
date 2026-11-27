# sorcerer-scry

Scry reads the code you changed and says what it gets wrong, on the line it is wrong.

## Install and run

```sh
cargo install sorcerer-scry     # or use the installers in the root README
scry --version
```

Needs a model account (Claude Code signed in, or `ANTHROPIC_API_KEY`) and a `JEV_API_KEY`.

Main commands:

- `scry`: review the units your working tree changed (`--all`, `--since REV` widen it).
- `scry list`, `scry why <id>`, `scry dismiss <id>`: read, explain or hide findings.
- `scry cost`, `scry doctor`: what it spent, and whether it can run here.
- `scry lsp`: the editor's server, so findings show as diagnostics.

## Example

```sh
$ scry
src/orders/cancel.py:41  bug  cancel() returns None when no order has that id; the
                              caller in api/orders.py:88 indexes the result.  (0.91)
```

Findings stay in `.sorcerer/state/scry/findings.json` until the code changes or you dismiss them.

## Inside

A library (`scry`) and a binary. Layers: `domain/`, `ports/`, `adapters/`, `usecases/`, `cli/`. It builds on `sorcerer-core`, `sorcerer-models` and `sorcerer-lsp`.

Book: [Scry](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/scry.md) (using it), [Scry internals](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/scry.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
