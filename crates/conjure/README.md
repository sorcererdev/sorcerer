# sorcerer-conjure

Conjure writes one small change where you ask, shows it in your editor, and marks it so you can undo it.

## Install and run

```sh
cargo install sorcerer-conjure     # or use the installers in the root README
conjure doctor
```

Needs a model account (Claude Code signed in, or `ANTHROPIC_API_KEY`) and a git project.

Main commands:

- `conjure FILE:LINE[-END]`: write a change at that line or range.
- `conjure --finding <id>`: write the fix for a scry finding.
- `--try-harder`: ask the stronger model, which may read the project first.
- `conjure log`, `conjure cost`, `conjure doctor`: what it did, spent, and whether it can run.
- `conjure lsp`: the editor's server (code actions, undo).

## Example

```sh
conjure src/cart.py:12
```

The change lands in the file with a marker; undo it from the editor.

## Inside

A library (`conjure`) and a binary, in the same layers as scry. It reads scry's findings and the project's rules through `sorcerer-core`, and calls models through `sorcerer-models`.

Book: [Conjure](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/conjure.md) (using it), [Conjure internals](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/conjure.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
