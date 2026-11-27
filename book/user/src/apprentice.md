# Apprentice

Apprentice is a coding agent whose whole interface is your editor. There is no chat: you write
in your code (a comment, a sentence in a docstring, a stub) and apprentice answers where you
wrote, offers to do what you asked, and when you accept, works in your buffers while you keep
typing. Everything it makes is a change in your editor that you review, edit and undo with the
editor's own tools.

The person is the sorcerer; the apprentice works in their workshop and everything it makes is
checked by them. It is a **prototype**: it is built from source, it is not on crates.io and no
release carries it yet (the question of running it on a subscription is being asked first).

```
cargo install --git https://github.com/sorcererdev/sorcerer sorcerer-apprentice
```

## In your editor

The server is `apprentice lsp`, one per project. It works on two clocks:

- **Idle** (one and a half seconds after you stop typing): what you changed since its last look
  (for a file just opened, since you last saved it, so what you typed while the server was
  starting counts) is cut into *regions* (a changed comment, a paragraph of a file no language covers, a
  function whose body does nothing yet, or a bare hunk of code) and each new one is asked of
  Jev: is it a question, a claim, an instruction or an aside, and is it addressed to a reader?
  Only a clear answer (0.70 for both) counts; anything else is nothing and shows nothing. A body
  that does nothing yet is an instruction without asking. Jev is cheap, not free: it has its own
  cap (`triage_per_day`). Without `JEV_API_KEY` only the empty bodies are offered.

  Only your typing is triaged. Left out are the lines a Sorcerer tool landed in the file in the
  last ten minutes (spellbook's fixes and syncs, a conjure step, an apprentice task: each notes
  what it lands in `.sorcerer/state/tool-edits.jsonl` before it sends the edit), lines that were
  already in the file (moved, as sorting imports does, or re-indented), a change that is the old
  text laid out again (the same once spaces, line breaks, brackets, commas, semicolons and quote
  marks are taken out, as a formatter rewrapping a call or adding a trailing comma does) and
  lines of layout only (blank, or a lone bracket). So format-on-save and a spellbook sync cost no
  triage and offer nothing. A formatter that changes words is still read as an edit.
- **Save**: each question in the saved file is answered by one read-only research call (one to
  three sentences, the code cited as `path:line`, Read, Grep and Glob only), and each claim is
  checked by the claim checker scry uses (haiku, Jev's guards, sonnet when they doubt).

What you see, in the margin (diagnostics of source `apprentice`):

| region | the mark | its code actions |
|---|---|---|
| a question | `apprentice: <the answer>`, its cost and time; the cited code on hover | *Show code*, *Keep this answer* (written in below as a comment, or as a paragraph), *Dismiss* |
| a claim | `apprentice: holds`, `does not hold: <why>` or `cannot tell: <why>` | *Show code*, *Dismiss*; a claim that does not hold also offers the task |
| an instruction | a faint hint, `apprentice: I can do this · ~$0.50 · sonnet` | `apprentice: do this · ~$0.50 · sonnet`, *Dismiss* |

Nothing runs a task by itself: an instruction is only offered, with its price (the median of
your finished tasks once there are five, else `estimate_start`). Every classified region also
offers four labels, `apprentice: this was a question / a claim / an instruction / nothing`: a
pick is a line of `.sorcerer/state/apprentice/labels.jsonl`, and so is a *Dismiss*. That file
is the prototype's measure of whether apprentice understood you.

Two regions on one line (a function whose body does nothing yet and the comment in it) show one
hint and one row of each action: *do this* runs the larger region (the function, comment and
all); *Dismiss* and a label apply to both.

A mark lives on its text: it moves when lines are added above it, and goes once you change the
words. A checkout or a pull (`.git/HEAD` moved) starts every look over, saying nothing.

**While a task runs**, the status line says what it does (`apprentice: refuse late
cancellations · reading booking.py · $0.00`, then `writing cancel_booking.py`, `running uv`,
`subagent: reading routes.py`; `getting ready` while the agent loads its own tools), and the place it writes shows `⟳ apprentice: writing…`. A
command waiting for your permission says `waiting for you: ls`, then `running ls`, or `denied:
ls` when you said no. The amount is what the calls seen so far would cost at the model's price. Its
changes land in your buffers, unsaved, each one undoable with your editor's undo. A question it
asks on a line is a mark there (`apprentice asks: …`) with one code action per option; a
command it wants to run is asked in a list. *Stop this task* is offered on any line while it
runs.

**When it ends**: one message, `apprentice: done · refuse late cancellations · 1 file · $0.71 ·
1m35s`, and on the first changed line of each file it changed a mark, `✦ apprentice: refuse late
cancellations · 1 file · $0.71` (on a file other than the one you asked in, `· other file ·`
in place of the count), with *Show diff* (the file before the task against now, in your
editor's diff view), *Undo this task* (every file back to its text before, in one undoable edit;
a file you edited since is left alone and named; a final newline your editor added, or its line
endings, are not an edit) and *Show journal*. Review the hunks with your
usual tools (gitsigns, the SCM view); saving every file it changed keeps the task, and its marks
go. A task the server was running when it stopped is recorded `interrupted`. If the server ends
while a task runs (the editor closes or crashes, or its server is killed), the agent stops after
its next tool call: that call fails, saying the task is over, and nothing more is read, run or
spent.

The editor clients for Neovim and VS Code are below. Any other editor that speaks LSP
gets the marks, the code actions, the questions in a list, the edits and the notes (opened over
the current window, and the diff as a file); grey suggested lines are a message there.

## In Neovim

Install [sorcerer.nvim](https://github.com/sorcererdev/sorcerer.nvim) and turn apprentice on:

```lua
require("sorcerer").setup({ apprentice = { enabled = true } })
```

The plugin starts `apprentice lsp` on every buffer under a project root, reporting each edit at
once (`debounce_text_changes = 0`, so a task never writes over what you typed a moment ago and
your edits reach the agent after its next tool call). It draws what the server sends only to it:
grey suggested lines under a line (`SorcererGhost`; gone at your next edit of that line), the
agent's notes in a vertical split beside your code, and *Show diff* as a scratch buffer of the
text before beside the live one, both in diff mode, in a tab of their own (`:tabclose` ends it). Marks are `vim.diagnostic`; the code actions
are your code-action key's. The keys:

| key | does |
|---|---|
| `<leader>ma` | `apprentice: do this` on the line, whatever its price |
| `<leader>mq` | `apprentice: Stop this task` |
| `<leader>md` | `apprentice: Show diff` |
| `<leader>mz` | `apprentice: Undo this task` |
| `<leader>mj` | `apprentice: Show journal` |
| `<leader>mx` | `apprentice: Dismiss` |
| `<leader>ml` | the four labels, to pick one |

`require("sorcerer").status()` says `apprentice: <task> · <what it does> · <spent>` while a task
runs. `:checkhealth sorcerer` checks that `apprentice` is found.

## In VS Code

The **Apprentice** extension (`editors/vscode/apprentice/`) is not on the Marketplace while
apprentice is a prototype. Install it from a `.vsix`: CI's `apprentice-vsix` artifact, or
`npm ci && npm run package` in that folder, then `code --install-extension
apprentice-0.2.0.vsix`. It finds `apprentice` from the `apprentice.path` user setting, then your
PATH, `~/.cargo/bin` and `~/.local/bin`; with none it says the `cargo install` line above, once
per version.

It starts one `apprentice lsp` per workspace folder, for every file under it, and sends each of
your edits at once (so a task never writes over what you typed a moment ago). Marks are VS
Code's diagnostics and the code actions are the lightbulb's; the progress of a task is in the
status bar; the agent's messages are notifications, and a command it wants to run is a
notification with *Allow once*, *Always for this project (…)* and *Deny*: closing it, or leaving
it for two minutes, denies the command. With *Do Not Disturb* on the question is hidden under
the bell, so it is denied when the two minutes are up unless you open it there. What the
extension draws itself: a suggested line as grey text after the line it belongs under (`⤷ …`,
the whole of it on hover; your next edit of that line takes it away), the agent's notes in the
column beside your code, *Show diff* as VS Code's diff view of the text before the task against
the live buffer (in a tab of its own), and a violet sparkle in the gutter on each file's first
line a finished task changed. The keys, the same letters as Neovim's:

| keys | does |
|---|---|
| `ctrl+m a` | `apprentice: do this` on the line, whatever its price |
| `ctrl+m q` | `apprentice: Stop this task` |
| `ctrl+m d` | `apprentice: Show diff` |
| `ctrl+m z` | `apprentice: Undo this task` |
| `ctrl+m j` | `apprentice: Show journal` |
| `ctrl+m x` | `apprentice: Dismiss` |
| `ctrl+m l` | the four labels, to pick one |

On macOS `ctrl` is the Control key. Change a binding in Keyboard Shortcuts (search
`apprentice.keys`).

## What a task is

A task is one instruction you accepted. It runs as one Claude Code session (`claude -p`, on
your own login, in the project root) with everything Claude Code has: subagents, web research,
reading and searching your files, running commands. What it may not do:

- **Write a file any other way than `write_file`.** Claude Code's `Edit`, `Write`, `MultiEdit`
  and `NotebookEdit` are denied. A `write_file` lands in your editor as one undoable change to
  the buffer, never to the disk: saving is yours. Some paths it never writes at all, even
  through `write_file`: `.sorcerer/`, `.git/`, and the config a tool obeys the next time it
  starts in the project (`.claude/`, `.mcp.json`, `.envrc`, anywhere in the tree), since a
  change there could run code later without anyone asking. The list is
  `crates/apprentice/library/never-written.toml`.
- **Read what is fenced.** `.env` files, `.git/`, `.sorcerer/` and your `[model] not_evidence`
  globs are denied to it, as they are to every sorcerer tool, so it cannot read its own journal
  or the ledger. Claude Code's file tools (Read, Grep, Glob, and a file you `@`-mention) refuse
  them, and so do the commands Claude Code reads paths from, such as `cat .env`. Any command
  that names a fenced path plainly (`git show HEAD:.env`, `ls .git`) is refused before you are
  asked, even one you allowed always; one that spells it some other way (`X=.env; cat $X`) is
  caught by you, since every command is asked about. Nothing outside the project can be read with the file tools.
  apprentice itself never hands it a fenced file either: what you type in an open `.env`, a
  `not_evidence` file or a task's journal is left out of the edits it is fed and of its first
  message, a fenced file is never triaged, and an instruction written in one does not start.
- **Run a command without asking you.** Every command is asked about, even the ones Claude Code
  counts as read-only and runs unasked elsewhere (`cat`, `grep`, `git log`): the first time it
  wants to run `uv run pytest -q`, you are asked: *Allow once*, *Always for this project (uv run
  pytest -q)* or *Deny*. The question comes in your editor's picker; an editor with no picker
  gets the same choices as code actions on the line of your instruction. A command of several
  lines (a heredoc, a script) shows its first line in the picker or on the mark, and the whole
  command in a message before the picker or in the mark's hover. *Always* remembers that exact command line, arguments and all, in
  `.sorcerer/state/apprentice/permissions.json`; any other command line is asked about again, so
  allowing `grep foo src/` never lets `grep -r KEY .` run.

  *Always* is never offered for a command that chains others (`&&`, `;`, `|`, a redirect), has a
  glob in an argument (`cat s*/keys.txt`), or reads a whole tree: a recursive `grep`, `rg`,
  `git grep`, `git log -p`, `git show`, `git diff`, `find … -exec`, `xargs`, `cp -r`, `tar` and
  the others in apprentice's list (`crates/apprentice/library/tree-readers.toml`), however the
  program is spelled (`rg.exe`, `C:\tools\GREP.EXE`). Those can
  read `.env` without naming it, so each run is yours to allow once.

  **Always on a test runner or a build tool runs your project's code without asking.** `uv run
  pytest -q`, `make test` or `npm test` run whatever the project's code says, and the agent
  writes that code: a `conftest.py`, a `Makefile` target, a package script. Once you have
  allowed such a command always, the next task can change that code (in your buffer, which you
  then save, or on the disk when the task runs from the terminal) and run it unasked, and that
  code can read `.env` or send anything anywhere: the fences bind the agent's own tools, not the
  programs it runs. This is the risk *always* accepts; allow a test runner once instead when
  that matters to you.

  A `permissions.json` from before (which remembered first words, such as `grep`) is rewritten
  the first time it is read: a remembered program name on its own (`grep`, `ls`) and a tree
  reader (`git show`) are dropped; longer entries (`uv run pytest`) stay, as that exact line.
- **Chat.** Nobody reads its output. It speaks only through six tools, the *mediums*:

| medium | what you see |
|---|---|
| `notify` | a short message, once |
| `mark` | a line in the margin of a file, with hover text |
| `ghost` | grey lines under a line, not in the buffer: a line you may want to type |
| `write_file` | the change in your buffer, undoable |
| `note` | a markdown file beside your code (`.sorcerer/state/apprentice/notes/`) |
| `ask` | a decision to make: choices on a line, or a list to pick from |

When it ends a turn without using any of them, its last text is posted for it (a message when
short, a note when long), so nothing it decided to say is lost.

**Your edits win.** After every tool call the agent is handed what you changed in the files
since it last looked, as diffs, under "their edits are authoritative". A `write_file` to a file
you changed since it read it is refused with the file's text now, so it rebases instead of
writing over you. The same holds for edits you had not saved when the task started: the agent
reads files from the disk, so its first `write_file` to a buffer with unsaved edits is refused
with the buffer's text (one extra step for it, only then).

A question (`ask`) waits for you for two minutes (`ask_timeout_seconds`); then the agent is
told to carry on with its default.

## Usage and the caps

| key in `[apprentice]` | meaning |
|---|---|
| `model` | the model a task runs on: `sonnet` |
| `budget_per_task` | the most one task may spend, in dollars: 2.00 (passed to `claude` as `--max-budget-usd`, which may overshoot by one call) |
| `budget_per_day` | the most apprentice may spend in a local day: 5.00. When it is spent, nothing starts |
| `triage_per_day` | the most Jev's triage may spend in a local day: 0.20 |
| `max_turns` | the most turns a task may take: 60 |
| `ask_timeout_seconds` | how long a question waits: 120 |
| `estimate_start` | what an offer says a task costs before five tasks have finished: 0.50 |

On a subscription you pay no dollars per call: the numbers are *usage*. The cost Claude Code
reports (`total_cost_usd`) is what the same calls would have cost on the API; it is the only
number Claude reports, so it is the one apprentice records, caps and shows. A task you stop is
charged what its stream showed: every call's tokens, at the model's shipped price (cache reads
at a tenth of the input price, cache writes at 1.25 times, or twice for an hour), so a stop
after 12 seconds costs cents, not $2.00. It undercounts by the call that was running when you
stopped it. A stop before any call was seen, a task that ended without reporting its end, and
an end that reports no cost are charged the whole cap, since their cost cannot be seen.

Every call is a row of the project's ledger (`.sorcerer/state/ledger.jsonl`) under product
`apprentice`: `decide` (Jev's triage), `answer`, `check`, `task`. `apprentice cost` prints them a
local day a row.

## The commands

| command | what it does |
|---|---|
| `apprentice lsp` | the editor's server; your editor's client starts it |
| `apprentice FILE:LINE "instruction"` | one task from the terminal: its changes are written to the disk, what it said is printed when it ends, and its questions take their default (so a command is never allowed from the terminal). For trying the harness without an editor |
| `apprentice cost [--since YYYY-MM-DD] [--json]` | what it spent, a day a row: triage, answers, checks, tasks |
| `apprentice log [N]` | the last tasks: id, state, files, cost, place, title |
| `apprentice doctor` | whether it can run here: `claude` (tasks need it), the provider (questions and claims), Jev's key, the config, the ledger, the records, trust |
| `apprentice trust [--revoke]` | trusts the project for the editor's server, as `scry trust` does |
| `apprentice mcp`, `apprentice hook EVENT` | the helpers a task's agent runs; not for you |

## Where it keeps things

Everything is under `.sorcerer/state/apprentice/`, which git never sees: `tasks.jsonl` (one
line each time a task changes state), `tasks/<id>/journal.md` (every message the agent sent and
through which medium, every question and answer, every refused write, the diffs it was fed),
`tasks/<id>/before/<path>` (each changed file's text before the task, for undo and the diff
view), `notes/`, `permissions.json`. The Claude Code session itself stays where Claude keeps it,
under your `~/.claude/projects/`; apprentice keeps only its id.
