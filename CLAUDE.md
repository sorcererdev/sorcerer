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

## How the code reads

1. **One level per body.** A function either names its steps or does one step's detail. Never both.
2. **A function that tells a story is short.** The top function of a file reads as the list of
   what happens, in order; each line is a call whose name says the step.
3. **Section comments become function names.** A comment such as `// the units to send, in batches`
   marks where a helper belongs.
4. **Run-log and report text lives in its own functions** (`log_…`, `…_text`), so the logic reads
   without it.
5. **Helpers sit below their caller, private, in reading order.** A reader goes down the file and
   does not leave it. No helper file of unrelated odds and ends.
6. **Arguments that travel together become a small struct with methods.**
7. **Names.** The established word first; a made-up word only where none exists. No abbreviation a
   newcomer cannot expand (`st`, `fp`, `raws`, `t`). A name says what the thing is for, not how it
   is made (`units_not_yet_reviewed`, not `filter_units`).
8. **Doc comments.** One plain sentence of intent on every function whose name does not already
   say it all. A module's opening comment lists its steps in order, using the helpers' names. A
   comment inside a body explains a *why* the code cannot (a rule, a trap), never a *what*.

What the rules do not ask for: a wrapper that only renames one call; a helper named after
mechanics (`process_item`, `handle_reply_2`); a split so fine that the detail is more than two
jumps from the story; comments that repeat the code.

clippy stops a function over 100 lines; an exception is an #[expect] with its reason. The limit is a tripwire: a shorter function is still held to the rules.
scry reviews changed code against these rules (.sorcerer/scry.toml); keep the two in step.
