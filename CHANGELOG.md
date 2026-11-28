# Changelog

## Unreleased

- **Spellbook: what a pack needs to start a project.** `spellbook init --pack <repo>[@rev]` copies a
  pack's `.sorcerer/` into the project (keeping the project's own files, refusing a project that has
  a spellbook) and records `{ name, repo, rev }` in `.sorcerer/pack.toml`. A `create` edit may
  `copy = "files/<path>"` a file unchanged (binary too: fonts, minified scripts). `{= timestamp =}`
  is built in (UTC `YYYYMMDDHHMMSS`, `--set timestamp=…` fixes it). `[notes] after = ["…"]` prints
  what to do next once the plan lands (in `--json` as `plan.notes`). Templates keep another template
  language's `{{ … }}` and `{% … %}` as text, so a generator can write an askama or Jinja page; only
  `{{ name }}` around one of the generator's own names is still refused. A generator already ran in
  an empty project; the new `starter` corpus proves it.

- **Breaking: the editor keys are grouped by product.** In both editors the keys you use most sit
  right under the prefix (`<leader>m` in Neovim, `ctrl+m` in VS Code): `a` do this (lackey), `f`
  fix this, `g` generate… (spellbook), `n` next step (conjure), `u` undo the last change, `w`
  why is this flagged? (scry), `q` stop the running task (lackey). Each product has its own group:
  `s` scry, `c` conjure, `b` spellbook, `l` lackey. The descriptions are plain words with no
  `sorcerer:` before them; which-key shows each group under its product's name, and VS Code's
  Command Palette lists each command under its product (**conjure: Next step**). A Neovim
  `keys = { … }` that names an old key is listed by `:checkhealth sorcerer`. Before → now:

  | does | Neovim before → now (setup name before → now) | VS Code before → now |
  |---|---|---|
  | generate… (spellbook new) | `<leader>mn` → `<leader>mg`, `<leader>mbg` (`new` → `generate`, `spellbook_generate`) | `ctrl+m n` → `ctrl+m g`, `ctrl+m b g` |
  | fix this | `<leader>mf` → `<leader>mf`, now the spellbook fix else the line's preferred quick fix; spellbook's only: `<leader>mbf` (`fix` → `fix`, `spellbook_fix`) | `ctrl+m f` → `ctrl+m f` (same fallback); spellbook's only: `ctrl+m b f` |
  | next step (conjure here) | `<leader>mc` → `<leader>mn`, `<leader>mcn` (`conjure` → `next_step`, `conjure_next`) | `ctrl+m c` → `ctrl+m n`, `ctrl+m c n` |
  | undo the conjured change | `<leader>mu` → `<leader>mcu` (`undo` → `conjure_undo`) | `ctrl+m u` → `ctrl+m c u` |
  | undo the last change, any tool (new) | `<leader>mu` (`undo`) | `ctrl+m u` |
  | try the change again with more effort | `<leader>mt` → `<leader>mct` (`try_harder` → `conjure_try_again`) | `ctrl+m t` → `ctrl+m c t` |
  | why this step (conjure) | `<leader>mw` → `<leader>mcw` (`why` → `conjure_why`) | `ctrl+m w` → `ctrl+m c w` |
  | why is this flagged? (scry, new) | `<leader>mw`, `<leader>msw` (`why`, `scry_why`) | `ctrl+m w`, `ctrl+m s w` |
  | review this file now (scry, new) | `<leader>msr` (`scry_review`) | `ctrl+m s r` |
  | dismiss the finding (scry, new) | `<leader>msx` (`scry_dismiss`) | `ctrl+m s x` |
  | list findings (scry, new) | `<leader>msl` (`scry_list`) | `ctrl+m s l` |
  | show the code it cites (scry, new) | `<leader>msc` (`scry_code`) | `ctrl+m s c` |
  | check | `<leader>mk` (the project) → `<leader>mbk` (this file; `:Spellbook check` is still the project) (`check` → `spellbook_check`) | `ctrl+m k` → `ctrl+m b k` (the project) |
  | sync regions | `<leader>ms` → `<leader>mbs` (`sync` → `spellbook_sync`) | `ctrl+m s` → `ctrl+m b s` |
  | list holes (new) | `<leader>mbh` (`spellbook_holes`, `:Spellbook holes`) | `ctrl+m b h` |
  | do this (lackey) | `<leader>ma` → `<leader>ma`, `<leader>mla` (`do` → `do`, `lackey_do`) | `ctrl+m a` → `ctrl+m a`, `ctrl+m l a` |
  | stop the running task | `<leader>mq` → `<leader>mq`, `<leader>mlq` (`stop` → `stop`, `lackey_stop`) | `ctrl+m q` → `ctrl+m q`, `ctrl+m l q` |
  | show the diff | `<leader>md` → `<leader>mld` (`diff` → `lackey_diff`) | `ctrl+m d` → `ctrl+m l d` |
  | undo the task | `<leader>mz` → `<leader>mlz` (`undo_task` → `lackey_undo`) | `ctrl+m z` → `ctrl+m l z` |
  | journal | `<leader>mj` → `<leader>mlj` (`journal` → `lackey_journal`) | `ctrl+m j` → `ctrl+m l j` |
  | open the answer | `<leader>mo` → `<leader>mlo` (`open_answer` → `lackey_open_answer`) | `ctrl+m o` → `ctrl+m l o` |
  | dismiss (lackey) | `<leader>mx` → `<leader>mlx` (`dismiss` → `lackey_dismiss`) | `ctrl+m x` → `ctrl+m l x` |
  | label what you wrote | `<leader>ml` → `<leader>mll` (`label` → `lackey_label`) | `ctrl+m l` → `ctrl+m l l` |

- **Undo the last change** (`<leader>mu`, `ctrl+m u`) takes back the latest change conjure,
  lackey or spellbook made, whichever it was, and says whose (`undo: conjure's last change
  (a.py:12)`).
- **scry has keys**: why, review this file now (the server's new `scry.review` command reviews
  a file at once), dismiss, list findings and show code.
- **spellbook lists holes** in the editor: `:Spellbook holes` and `<leader>mbh` in Neovim,
  **spellbook: List holes** (`ctrl+m b h`) in VS Code.
- **lackey's mark says what it is**: `✦ lackey changed this line in task "refuse late
  cancellations" · 1 file · $0.71` (was `✦ lackey: refuse late cancellations · …`; a file other
  than the one the task was asked in says `· asked in another file ·`, was `· other file ·`).

## 0.4.0 — 2026-11-27

- **Fourteen more languages.** Sorcerer ships TypeScript, TSX, JavaScript, Go, Java, C#, Bash,
  SQL, HTML, CSS, Markdown, YAML, TOML and JSON beside Python and Rust, each as data under
  `crates/sorcerer-core/library/languages/` with a pinned grammar. The code languages are
  reviewed by scry, conjured in (with their test file and imports where the language has a
  convention), and get spellbook's holes, anchors and fixes and lackey's stub offers; the config
  and markup formats get rules, generators, owned regions (in `<!-- -->` or `/* */` where the
  comment closes) and lackey's comments. The user book's new Languages chapter has the table
  and each language's limits.
- **A language with no units is never reviewed or loaded by scry or conjure.** Editing a README,
  a `package.json` or a stylesheet costs nothing and downloads nothing (`skipped (no units)`).
- **Existing projects:** `.html`, `.css`, `.md`, `.toml`, `.yml` and `.json` files now have a
  language, so `spellbook check` and the doctors load their grammars (a download on first use)
  and lackey cuts comments and hunks in them; Markdown is still cut by paragraph.
- **The release carries 16 grammars on each of 5 platforms** (160 files, about 28 MB a platform;
  SQL's is the largest at about 13 MB). The release job fails on a library over the 20 MB scry
  downloads, and the smoke test checks that a TypeScript grammar is downloaded too.
- **New language keys** for a language of your own: `grammar.dir` (a repository holding two
  grammars), `comment_end` (closed comments), `@preamble`, `@prose`/`@heading` captures,
  `[imports] separator` and `{= dir =}`, `[test] dir_replace`. See Adding a language.
- **`spellbook holes` lists a hole once** when a generated file deleted by hand is generated
  again (it listed each hole twice).

## 0.3.1 — 2026-11-27

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
