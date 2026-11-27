# Changelog

## Unreleased

- **A newcomer can start from the README.** The README links the user book and each tool's
  chapter, and has quick starts for Lackey and Spellbook; the Lackey chapter opens with
  Getting started (Claude Code login, a Jev key from typesafe.ai, `lackey doctor`, what to
  write for a first offer and why a comment inside a function may get none).
- **`lackey doctor` warns.** No model provider and no `JEV_API_KEY` are `warn` rows (with how
  to fix them), not `ok`; only a `FAIL` makes it exit 2.
- **A terminal task says its change is on the disk**, not "in your editor, unsaved".
- **A question under a heading in a new note is answered once**: the heading is its section,
  not a second region.
- **Task titles drop the comment mark** (`# `) and a number such as `0.0` no longer ends the
  title's sentence.
- **VS Code**: `ctrl+m a` on a line with no offer says why; each release attaches the `.vsix`
  files, for VS Code forks while Open VSX does not list Lackey and Spellbook.

## 0.3.0 — 2026-11-27

- **Lackey is released.** The coding agent whose whole interface is your editor ships like
  the other three: the `lackey` program in the release (installers
  `sorcerer-lackey-installer.sh`/`.ps1`), `cargo install sorcerer-lackey`, and the
  Lackey extension on the VS Code Marketplace and Open VSX (`sorcererdev.lackey`), which
  offers to download its program like Scry's. It runs Claude Code (`claude -p`) on your own
  subscription, so it needs `claude` installed and signed in; `lackey doctor` checks.
  The test program `lackey-fake-claude` is built only with the `fake-claude` feature, so it
  is in neither the release nor `cargo install`.
- **Notes are files lackey works on.** Notes moved from `.sorcerer/state/lackey/notes/`
  to `.sorcerer/notes/`, the one place in `.sorcerer/` that is not fenced: what you write in a
  note is triaged, offered, researched on save and done like in any file. Old notes move on the
  next start. A link in the notes onto a fenced path stays fenced.
- **A link in an ignored folder is denied too.** Lackey and `claude -p` research name every
  symbolic link out of the project or onto a fenced path, ignored folders included (a
  force-added ignored link is checked out like any file). A task's write is refused when it
  would land, through a link, in a file no task writes (such as `.claude/settings.json`).
- **Writers are told what a spellbook region is.** When a file conjure or lackey may edit
  holds a `spellbook:region`, the writer is told its lines come from files, and that creating
  the file adds its line on save.
- **Lackey starts tasks fast in a large project.** The symbolic links it denies by name are
  kept between tasks; a task's start walks again only the folders that changed since the last
  one, and checks every known link afresh, so a link made since the last task is still denied.
  The look for links now follows only the project's own `.gitignore` and `.ignore` files (not a
  global gitignore or `.git/info/exclude`) and skips `.sorcerer/`, which is fenced as a whole.
  A project that is not a git repository now honours its `.gitignore` there too, so a link in a
  folder it leaves out is no longer named. On a file system that does not move a folder's time
  when an entry changes (FAT on Windows), nothing is kept and every task walks the whole project.
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
- **conjure says what a rules retry dropped.** When the writer is asked again to keep a spellbook
  rule and the second answer loses a call or definition the first added (it deleted a
  `std::fs::write` instead of moving it into a port), Jev judges each drop and the step says the
  doubtful ones: `asked again for commands-use-ports · dropped: std::fs::write in close_table.rs
  (looks unintended)`. Nothing is held back; scry's next review of the file is told.

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
