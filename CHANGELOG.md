# Changelog

## Unreleased

- **Apprentice starts tasks fast in a large project.** The symbolic links it denies by name are
  kept between tasks; a task's start walks again only the folders that changed since the last
  one, and checks every known link afresh, so a link made since the last task is still denied.
  The look for links now follows only the project's own `.gitignore` and `.ignore` files (not a
  global gitignore or `.git/info/exclude`) and skips `.sorcerer/`, which is fenced as a whole.
- **Adopting `spellbook check` on code with debt.** `spellbook check --baseline` records today's
  findings in `.sorcerer/spellbook/baseline.toml`; they show as `(existing)` and never fail, so only
  new breakage does. `spellbook check --since main` judges a branch on the lines it touched.
  `[spellbook.rules]` in `config.toml` makes a rule `hint`, `warn`, `error` or `off` for the project.
  A baseline entry knows the function around its finding and the finding's whole line, so the same
  call written elsewhere is new, and `--since` never lets a pasted copy of recorded debt pass.
- **A fix's import keeps sorted imports sorted.** When the imports' last block is in order, `end of
  imports` puts the new one where it belongs instead of at the end.
- **Spellbook for VS Code:** the plan's title names the inputs taken without asking (an enum with
  one choice), such as `· app bistro, service booking (the only choices)`.
- **conjure tells you what a step did.** After every step the editor shows one message, such as
  `conjure: changed seat_party__test.py (1 line), seat_party.py (2 lines) · <goal>` or
  `conjure: no change · <goal>`, so a change to a file you are not looking at is not missed.

## 0.2.0 — 2026-11-24

Spellbook joins scry and conjure.

- **spellbook**, your project's conventions as data: generators (templates with holes an AI or
  you fill, answered input by input, previewed as a plan before anything is written) and rules
  (tree-sitter queries, errors unless they say otherwise; `spellbook check`, with
  `# spellbook:allow rule — why` to let one place break one). Works without a model; conjure
  fills holes when asked. Installers `sorcerer-spellbook-installer.sh`/`.ps1`, or
  `cargo install sorcerer-spellbook`.
- **Spellbook in the editors.** `spellbook lsp`; in Neovim (`sorcerer.nvim`) and in the new
  Spellbook VS Code extension: New (`ctrl+m n`), Check into Problems (`ctrl+m k`), holes as
  snippet placeholders you Tab through, hole and rule marks.
- **Generator inputs with fixed choices** (`choices = [...]` on an enum input).
- **Conjure in VS Code** under `ctrl+m` (`c`, `u`, `t`, `w`) with a conjured mark; a line the
  parser lost is conjured as a broken region to repair.
- **scry**: `list` shows ids, `cost` hides an empty waiting line, `why` uses local time.
- **Windows**: a grammar compiles when the user's folder has letters outside Windows-1252.
- The README is a worked first run.

## 0.1.0 — 2026-11-23

The first release: scry and conjure.

- **scry**, a reviewer in the margin: reviews what you changed and keeps each finding that
  survives a claim check and a nitpick screen. Python and Rust.
- **conjure**, one small change where you ask: `conjure FILE:LINE` prints one; in an editor,
  *Conjure here* (or *Conjure for this finding*) lands it in your buffers and marks it, with
  Undo and Try harder. About $0.001 a change.
- **Three operating systems.** Windows, macOS and Linux, tested in CI on each; one platform
  adapter holds every difference (paths, user directories, the C compiler, starting and stopping
  a runner and everything it started).
- **The editor clients.** `scry lsp` and `conjure lsp` are language servers. Neovim
  (`sorcerer.nvim`, conjure opt-in) starts them; the Scry and Conjure VS Code extensions find
  the program or offer to download their own version, checked against its sha256, and are
  published to the Marketplace and Open VSX with this release.
- **Installers.** Shell and PowerShell installers for each program
  (`sorcerer-scry-installer`, `sorcerer-conjure-installer`), and `cargo install sorcerer-scry
  sorcerer-conjure`.
- **Prebuilt grammars.** The Python and Rust grammars are downloaded prebuilt from the release
  on first use, so no C compiler is needed; any failure falls back to compiling, and a project's
  own grammar is always compiled locally. `scry doctor` says which it did.
- **Spending caps.** $0.25 a pass and $3 a day by default.
- **The eval's two gates.** The recorded reviews of the labelled cases in Python and Rust must
  pass their scores with the project's rules, and a second table holds the bare (no rules)
  scores.
