# sorcerer-spellbook

Spellbook keeps a project's conventions as data: generators that write every file of a convention (with holes that say what belongs there), and rules that check the conventions hold.

## Install and run

```sh
cargo install sorcerer-spellbook     # or use the installers in the root README
spellbook doctor
```

Works without any model. Generators and rules live in `.sorcerer/spellbook/`.

Main commands:

- `spellbook list`, `spellbook show <generator>`: what the project can generate.
- `spellbook new <generator>`: write its files (`--plan` only shows them); `spellbook holes` lists holes still open.
- `spellbook check`: run the rules; `spellbook fix`: apply a rule's fix to a finding.
- `spellbook sync`: refresh the owned regions (lists between markers) from the files on disk.
- `spellbook lsp`: the editor's server.

## Example

```sh
spellbook list
spellbook new command --plan
spellbook check
```

## Inside

A library (`spellbook`) and a binary, in the same layers as scry. It builds on `sorcerer-core` and `sorcerer-lsp`.

Book: [Spellbook](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/spellbook.md) (using it), [Spellbook internals](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/spellbook.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
