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

## What a task is

A task is one instruction you accepted. It runs as one Claude Code session (`claude -p`, on
your own login, in the project root) with everything Claude Code has: subagents, web research,
reading and searching your files, running commands. What it may not do:

- **Write a file any other way than `write_file`.** Claude Code's `Edit`, `Write`, `MultiEdit`
  and `NotebookEdit` are denied. A `write_file` lands in your editor as one undoable change to
  the buffer, never to the disk: saving is yours.
- **Read what is fenced.** `.env` files, `.git/`, `.sorcerer/` and your `[model] not_evidence`
  globs are denied to it, as they are to every sorcerer tool, so it cannot read its own journal
  or the ledger.
- **Run a command without asking you.** The first time it wants to run `uv run pytest`, you are
  asked: *Allow once*, *Always for this project (uv run pytest)* or *Deny*. *Always* remembers
  the command's first words (at most three, stopping at an option or a path) in
  `.sorcerer/state/apprentice/permissions.json`; a command that chains others (`&&`, `;`, `|`,
  a redirect) is always asked about.
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
writing over you.

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
charged its whole cap, since its end cannot be seen.

Every call is a row of the project's ledger (`.sorcerer/state/ledger.jsonl`) under product
`apprentice`: `decide` (Jev's triage), `answer`, `check`, `task`. `apprentice cost` prints them a
local day a row.

## The commands

| command | what it does |
|---|---|
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
