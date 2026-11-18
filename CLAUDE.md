# Sorcerer: notes for agents

## The books are part of every change

`book/user/` (using sorcerer) and `book/internals/` (how it works, so you can change it) are
kept current in the same branch as the code. A chapter that disagrees with the code is a bug.

- A change to behaviour, commands or config keys updates the user chapter that covers it.
- A new or reshaped module, data flow or seam updates the internals book, naming the
  functions and files a reader would open.
- CI (`scripts/book-check`) refuses a branch that changes `crates/` or `library/` without
  touching `book/`. When no chapter needs changing, say why in a `Book: <reason>` trailer.

## Other conventions

- The engine knows no language: languages are files under `library/languages/<name>/`.
  Tests that name a language live under `crates/*/tests/`, never in `src/`
  (`scripts/no-language-names`).
- No test touches the network or calls a model.
- Commits are signed off (`git commit -s`).
