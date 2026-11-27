# Apprentice

Apprentice is a coding agent whose whole interface is your editor. There is no chat: you write
in your code (a comment, a sentence in a docstring, a stub) and apprentice answers where you
wrote, offers to do what you asked, and when you accept, works in your buffers while you keep
typing. Everything it makes is a change in your editor that you review, edit and undo with the
editor's own tools.

The person is the sorcerer; the apprentice works in their workshop and everything it makes is
checked by them. It is an early release, the newest of the tools.

**It runs Claude Code on your own subscription.** Each task is one Claude Code session
(`claude -p`) on your own login, so you need Claude Code installed and signed in (`claude` on your
PATH); tasks count against your Claude usage (see [Usage and the caps](#usage-and-the-caps)).
`apprentice doctor` checks that `claude` is there.

Install it with the release's installer. macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.ps1 | iex"
```

Or with Rust: `cargo install sorcerer-apprentice`. The installers put `apprentice` in
`~/.local/bin`.

## In your editor

The server is `apprentice lsp`, one per project. It works on two clocks:

- **Idle** (one and a half seconds after you stop typing): what you changed since its last look
  (for a file just opened, since you last saved it, so what you typed while the server was
  starting counts) is cut into *regions* (a changed comment, a paragraph of a file no language covers, a
  function whose body does nothing yet, or a bare hunk of code) and each new one is asked of
  Jev: is it a question, a claim, an instruction or an aside (the file's own content), and do
  you want it answered, checked or done now? Jev reads the region with its context, not alone:
  the 20 lines above it and 10 below, for a paragraph the heading it sits under and whether
  that section holds anything else, and your session's changes (everything changed since the
  branch split off `origin/HEAD`, else `main` or `master`, your file first, up to about 3,000
  tokens), then what you typed since the last look. So `Fill this in` under an empty
  `## Example` heading is offered, and the same words in a finished section, or `TODO(ada):
  later`, are not. A fenced file never goes, as a region or in the session's changes. It acts
  when Jev gives the kinds other than aside together at least 0.25 and "now" at least 0.12
  (low cuts, set on a labelled corpus where Jev keeps most of its weight on aside even for real
  asks); anything else is nothing and shows nothing. A body that does nothing yet is an
  instruction without asking. A triage costs about $0.00016 (3,800 input tokens) and a tenth of
  a second; Jev is cheap, not free: it has its own cap (`triage_per_day`). Without
  `JEV_API_KEY` only the empty bodies are offered.

  Only your typing is triaged. Left out are the lines a Sorcerer tool landed in the file in the
  last ten minutes (spellbook's fixes and syncs, a conjure step, an apprentice task: each notes
  what it lands in `.sorcerer/state/tool-edits.jsonl` before it sends the edit), lines that were
  already in the file (moved, as sorting imports does, or re-indented), a change that is the old
  text laid out again (the same once spaces, line breaks, brackets, commas, semicolons and quote
  marks are taken out, as a formatter rewrapping a call or adding a trailing comma does) and
  lines of layout only (blank, or a lone bracket). So format-on-save and a spellbook sync cost no
  triage and offer nothing. A formatter that changes words is still read as an edit.
- **Save**: each question in the saved file is answered by one read-only research call (one to
  three sentences, the code cited as `path:line`, Read, Grep and Glob only; sonnet, at most $0.25
  a call), and each claim is checked by the claim checker scry uses (haiku, Jev's guards, sonnet
  when they doubt). While a call runs its line says `⟳ apprentice: researching…` (a claim:
  `checking…`) and the status line `researching: <the question>`, so you know something is
  spending; saving again meanwhile does not ask the same question twice. A call that fails says
  why on its line, `apprentice: could not answer: <why> · save to ask again`, and the next save
  asks again.

What you see, in the margin (diagnostics of source `apprentice`):

| region | the mark | its code actions |
|---|---|---|
| a question | `apprentice: <the answer>`, its cost and time; the cited code on hover. A long answer shows its first sentence and `<key> opens the whole answer`: it waits in a note until you open it, so no window opens while you type | *Show code*, *Open the answer* (a long one, beside your code), *Keep this answer* (written in below as a comment, or as a paragraph), *Dismiss* |
| a claim | `apprentice: holds`, `does not hold: <why>` or `cannot tell: <why>` | *Show code*, *Dismiss*; a claim that does not hold also offers the task |
| an instruction | a faint hint, `apprentice: I can do this · ~$0.50 · sonnet` | `apprentice: do this · ~$0.50 · sonnet`, *Dismiss* |

Nothing runs a task by itself: an instruction is only offered, with its price (the median of
your finished tasks once there are five, else `estimate_start`). Every classified region also
offers four labels, `apprentice: this was a question / a claim / an instruction / nothing`: a
pick is a line of `.sorcerer/state/apprentice/labels.jsonl`, and so is a *Dismiss*. That file
is apprentice's measure of whether apprentice understood you.

Two regions on one line (a function whose body does nothing yet and the comment in it) show one
hint and one row of each action: *do this* runs the larger region (the function, comment and
all); *Dismiss* and a label apply to both.

A mark lives on its text: it moves when lines are added above it, and goes once you change the
words. A checkout or a pull (`.git/HEAD` moved) starts every look over, saying nothing.

**While a task runs**, its line says so from the start (`⟳ apprentice: working…`, then what it
does, `⟳ apprentice: reading booking.py`), and the status line says what it does (`apprentice: refuse late
cancellations · reading booking.py · $0.00`, then `writing cancel_booking.py`, `running uv`,
`subagent: reading routes.py`; `getting ready` while the agent loads its own tools), and the place it writes shows `⟳ apprentice: writing…`. A
command says `waiting until allowed: uv` until it is allowed (by a line you allowed always, or by
you), and one waiting for your permission says `waiting for you: ls`, then `running ls`, or `denied:
ls` when you said no. A chain is named by its programs, each once and at most three: `ls; cat
pyproject.toml` is `denied: ls, cat`. The amount is what the calls seen so far would cost at the model's price. Its
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
agent's notes and a long answer you open in a vertical split beside your code (your cursor stays
put), and *Show diff* as a scratch buffer of the
text before beside the live one, both in diff mode, in a tab of their own (`:tabclose` ends it). Marks are `vim.diagnostic`; the code actions
are your code-action key's. The keys:

| key | does |
|---|---|
| `<leader>ma` | `apprentice: do this` on the line, whatever its price |
| `<leader>mo` | `apprentice: Open the answer`: a long answer, beside your code |
| `<leader>mq` | `apprentice: Stop this task` |
| `<leader>md` | `apprentice: Show diff` |
| `<leader>mz` | `apprentice: Undo this task` |
| `<leader>mj` | `apprentice: Show journal` |
| `<leader>mx` | `apprentice: Dismiss` |
| `<leader>ml` | the four labels, to pick one |

The `⟳` at the end of a line being worked on (`SorcererWorking`; scry's `reviewing…` and
conjure's `writing…` too) spins while the work runs; `working = { animate = false }` in setup
keeps it still. `require("sorcerer").status()` says `apprentice: <task> · <what it does> ·
<spent>` while a task runs and `apprentice: researching: <the question>` while a question is
answered: put it in your status line to see it there. `:checkhealth sorcerer` checks that `apprentice` is found.

## In VS Code

Install the **Apprentice** extension (`sorcererdev.apprentice`) from the Extensions view (the
Marketplace; VS Code forks such as VSCodium use Open VSX), or `code --install-extension
sorcererdev.apprentice`. It finds `apprentice` from the `apprentice.path` user setting, then your
PATH, `~/.cargo/bin` and `~/.local/bin`; with none it offers to download the release of its own
version (checked against its sha256), or says the install line above, once per version.

It starts one `apprentice lsp` per workspace folder, for every file under it, and sends each of
your edits at once (so a task never writes over what you typed a moment ago). Marks are VS
Code's diagnostics and the code actions are the lightbulb's; the progress of a task, and of a
question being answered, is in the status bar (a spinning `$(sync~spin)` while anything is
worked on, beside grey `⟳ apprentice: researching…` at the end of its line); the agent's messages are notifications, and a command it wants to run is a
notification with *Allow once*, *Always for this project (…)* and *Deny*: closing it, or leaving
it for two minutes, denies the command. With *Do Not Disturb* on the question is hidden under
the bell, so it is denied when the two minutes are up unless you open it there. What the
extension draws itself: a suggested line as grey text after the line it belongs under (`⤷ …`,
the whole of it on hover; your next edit of that line takes it away), the agent's notes and a long answer you open (`ctrl+m o`) in the column beside your code, your
cursor staying put, *Show diff* as VS Code's diff view of the text before the task against
the live buffer (in a tab of its own), and a violet sparkle in the gutter on each file's first
line a finished task changed. The keys, the same letters as Neovim's:

| keys | does |
|---|---|
| `ctrl+m a` | `apprentice: do this` on the line, whatever its price |
| `ctrl+m o` | `apprentice: Open the answer`: a long answer, beside your code |
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
  through `write_file`: `.sorcerer/` (but its notes, `.sorcerer/notes/`), `.git/`, and the config a tool obeys the next time it
  starts in the project (`.claude/`, `.mcp.json`, `.envrc`, anywhere in the tree), since a
  change there could run code later without anyone asking. The list is
  `crates/apprentice/library/never-written.toml`.
- **Read what is fenced.** `.env` files, `.git/`, `.sorcerer/` (but the notes) and your
  `[model] not_evidence` globs are denied to it, as they are to every sorcerer tool, so it cannot read its own journal
  or the ledger. Claude Code's file tools (Read, Grep, Glob, and a file you `@`-mention) refuse
  them, and so do the commands Claude Code reads paths from, such as `cat .env`. Any command
  that names a fenced path plainly (`git show HEAD:.env`, `ls .git`) is refused before you are
  asked, even one you allowed always; one that spells it some other way (`X=.env; cat $X`) is
  caught by you, since every command is asked about. Nothing outside the project can be read with the file tools.
  A symbolic link that leads out of the project or onto a fenced file is denied by name; the
  links are looked for when each task starts, so a link made since the last task is denied too.
  The look walks again only the folders that changed since the last task, so a large project
  starts its tasks fast; a folder it could not open is looked at again at every task until it
  opens. On a drive that does not record when a folder changes (FAT on Windows, such as some USB
  sticks) every task looks through the whole project.
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
| `note` | a markdown file beside your code, in `.sorcerer/notes/` (see [Notes](#notes)) |
| `ask` | a decision to make: choices on a line, or a list to pick from |

When it ends a turn without using any of them, its last text is posted for it (a message when
short, a note when long), so nothing it decided to say is lost.

**Web research is not asked about.** `WebSearch` and `WebFetch` run unasked, so a task can look
things up. The other side of that: a prompt injection planted in a file the agent reads (a README,
a comment in a dependency) could have it fetch a URL that carries your code out in its query.
Fenced files never reach the agent, so what could leave is the unfenced project, which you already
let it read.

**Your edits win.** After every tool call the agent is handed what you changed in the files
since it last looked, as diffs, under "their edits are authoritative". A `write_file` to a file
you changed since it read it is refused with the file's text now, so it rebases instead of
writing over you. The same holds for edits you had not saved when the task started: the agent
reads files from the disk, so its first `write_file` to a buffer with unsaved edits is refused
with the buffer's text (one extra step for it, only then).

**Spellbook regions.** When the file you asked in holds a spellbook region, the task's first
message says that lines inside a `spellbook:region` are generated from files (to add one, create
the file, and saving adds its line); so does the answer to a `write_file` of a file that holds
one.

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

## Notes

Notes live in `.sorcerer/notes/`: a task's notes (`t3-why-is-it-slow.md`) and the long answers
you open (`r12.md`). It is the one place in `.sorcerer/` that is not fenced, so a note is a file
like any other. Write in one and apprentice works on it as it does on your code: what you type
is triaged (a heading and *Give me the step by step files to update here.* is offered as a task),
a question is researched when you save, and *do this* may write its answer into the note. You can
add notes of your own there too. Git sees the directory: commit your notes or add it to your
`.gitignore`, as you like.

The rest of `.sorcerer/` stays fenced, and so does anything a note points at: a link in
`.sorcerer/notes/` onto `.sorcerer/state/` or out of the project is denied, a `.env` file there is
still a secret, and your `not_evidence` globs still hold inside it. Notes written by an older
version under `.sorcerer/state/apprentice/notes/` move here when apprentice next starts (a note
whose name is already taken stays where it was).

## Where it keeps things

Notes are in `.sorcerer/notes/` (above). Everything else is under `.sorcerer/state/apprentice/`,
which git never sees: `tasks.jsonl` (one line each time a task changes state),
`tasks/<id>/journal.md` (every message the agent sent and through which medium, every question
and answer, every refused write, the diffs it was fed), `tasks/<id>/before/<path>` (each changed
file's text before the task, for undo and the diff view), `permissions.json`. The Claude Code session itself stays where Claude keeps it,
under your `~/.claude/projects/`; apprentice keeps only its id.
