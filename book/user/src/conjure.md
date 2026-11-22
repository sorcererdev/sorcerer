# Conjure

Conjure writes one small change where you ask. Put the cursor in a function, say what you
want by leaving a stub, a comment or a failing test, and conjure reads what is around it and
writes the next small thing: fill a body, continue what you started, make a failing test pass,
write a missing test. It names its goal in one sentence and never touches your disk: the
change is for you to read, keep or throw away.

In your editor, conjure is a language server (`conjure lsp`): you ask from the code-action
menu, the change lands in your buffers, and a mark on each changed file offers Undo, Try harder
and Show why. On the command line, `conjure FILE:LINE` prints the change it would make.

## In your editor

Start `conjure lsp` as a language server for your project (the editor extensions come next; any
editor that starts a language server can start it today; in Neovim:
`vim.lsp.start({ name = "conjure", cmd = { "conjure", "lsp" }, root_dir = vim.fs.root(0, ".git") })`).

**Asking.** Put the cursor in a function, or select lines in one, and open the code-action menu
(`vim.lsp.buf.code_action()` in Neovim, `Ctrl+.` in VS Code). *Conjure here* asks for one change
there. On a line where scry has an open finding, *Conjure for this finding* asks for a change led
by that finding. Outside a function nothing is offered (`put the cursor in a function to
conjure` if you ask anyway).

**Waiting.** A progress line, `conjure: conjuring a change`, for the few seconds the writer takes.
Asking again meanwhile is refused with `conjure: a change is already being conjured; this one was
not started`; the running one goes on.

**The change lands.** One edit carries every file: a file you have open is edited where you see
it (only the lines the writer changed); a file you do not have open opens as an unsaved buffer; a
new file is created by the editor and filled. Nothing is saved: the change is in your buffers,
yours to read, keep or undo. Conjure reads your unsaved text when it asks, and the edit is made
against exactly that text: if you type in a file the change touches while it is being conjured,
nothing lands and you are told `conjure: the file changed while the change was conjured; your
text stands`. An editor that cannot create files gets the rest of the change and the line
`conjure: this editor cannot create files; not created: <paths>`.

**The marks.** On the first changed line of the file you asked in, an information mark says what
the change did:

```
fill total · haiku · $0.001 · 4 s · parses · also changed: b.py, a__test.py
```

the goal, the model, the cost, the time, whether the file parses (or `does not parse (line 12)`),
the other files, and `not written: <paths>` when the writer named a path conjure may not write
(below). Each other changed file has its own mark, `conjured with a.py: fill total` (and `does not
parse (line N)` when it does not). Marks are diagnostics: they show in Neovim's diagnostic list and
VS Code's Problems panel. One change is marked at a time.

**The actions on a mark**, in the code-action menu on its line:

- *conjure: Undo* puts every file back as it was before the change, in one edit: a new file is
  deleted (emptied, and said, in an editor that cannot delete files). If you have edited a
  changed file since the change landed, Undo does nothing and says `conjure: <file> was edited
  after the change landed; undo it with the editor's own undo`: your typing is never thrown away.
- *conjure: Try harder* undoes the change, then asks the stronger model (`[conjure] try_harder`)
  with what the first writer read; it may read and search the project first. Its change lands
  and is marked like the first. It is not offered when `try_harder = ""`.
- *conjure: Show why* opens `.sorcerer/state/conjure/why-<id>.md`: what the change did, the
  situation the writer read, its reply as it came, and what the code said about the function
  before.
- *conjure: Show `<file>`*, one for each other changed file, opens it with the change selected.

**When a mark goes away.** Saving a changed file keeps the change; undoing it (with the action,
or with the editor's own undo until every file is back) undoes it; Try harder replaces it; the
next change keeps it. Closing a file hides its mark until you open it again. Conjure records each
change once, at the choice that ended it (`kept`, `undone`, `replaced`), or `dropped` when the
editor refused the edit and `unanswered` when the editor closed with the change still marked;
`conjure log` lists them.

**No change.** When the writer finds nothing to do, a mark on the function's first line says
`no change · <goal>`, with Try harder and Show why. It goes at your next edit or save.

**Failure.** A model that does not answer (a timeout, say) is one message, `conjure: the model did
not answer: <why>`, and no mark. A spent day says its sentence (below) and calls nothing.

**Trust.** A project whose config runs code or moves your key (a provider's `runner` or `url`, a
grammar of its own) is not conjured in until you trust it: asking says `conjure: this project is
not trusted; run "conjure trust" in it`. `conjure trust` and `scry trust` keep one list.

An editor must apply edits from a server tied to a document's version (`workspace/applyEdit` with
`documentChanges`; one that announces file operations is taken to). One that cannot is offered
nothing, and told once: `conjure: this editor cannot apply edits from a server (workspace/applyEdit
with documentChanges)`.

## On the command line

```
conjure src/cart.py:12            # one change at line 12 (the function the line is in)
conjure src/cart.py:12-20         # the change is about these lines
conjure src/cart.py:12 --finding 3fa9c0   # for a scry finding (see `scry list`)
conjure src/cart.py:12 --try-harder       # the stronger model, which may read the project first
conjure doctor                    # whether conjure can run here
conjure cost [--since DAY] [--json]   # what conjure spent, a local day a row
conjure log [N]                   # the last N changes the editor recorded (10 by default)
conjure trust                     # let the editor conjure in this project (--revoke: no longer)
conjure lsp                       # the editor's language server, over standard input and output
```

A run prints what it would change and writes nothing:

```
goal: Fill CreateBooking.handle with the table, party and slot checks, then save the booking.
--- src/bistro/booking/commands/create_booking.py
@@ -47,4 +47,13 @@
...
parses
haiku · $0.001 · 4 s
```

First the goal, then each file the change touches as a diff (a new file says `(new file)`), and
under it `parses` or `does not parse (line 12)`: conjure parses what it wrote with the file's
grammar and says where it does not. A path the writer named but conjure refused to write is
listed as `not written: <paths>` (below). The last line is the model, what the call cost and
how long it took. When nothing needs doing, or what the writer wrote equals what is there, the
first line is `no change · <goal>`.

The exit code is 0 when a change was conjured (or none was needed) and 2 when conjure could not
run: the cursor is in no function (`put the cursor in a function to conjure`), the day's
budget is spent, the model failed, or the arguments are wrong. Each call is recorded in the
ledger, so `conjure cost` and `scry cost` share one account. The command line keeps no record
of changes; `conjure log` shows what the editor recorded.

## What conjure reads

For the function at the line, in this order, each part cut as shown:

- the whole file, and its test file, or where its tests would go (`[test]` in the language file);
- the files it imports, at most 8, their first 120 lines each (`[imports]` and `imports.scm`);
- what you have been doing: the uncommitted changes in files of the same language, cut at 300
  lines, and the names of up to 20 new files;
- scry's open findings near the function, at most 5, as context the writer may act on or not
  (conjure reads scry's `.sorcerer/state/scry/findings.json` when it is version 2);
- your project's rules, from `.sorcerer/scry.toml` (its `context`, its `include`d files and its
  rules), cut at 3000 characters;
- with `--finding`, that finding leads.

Nothing a model may not read is sent: a file under `[model] not_evidence`, a `.env` file, anything
under `.sorcerer/` or `.git/`, and anything outside the project (an import that resolves through a
symbolic link out of it, a language template that names an absolute path) is left out, from the
file you ask in (conjure refuses it), the test file, the related files and the uncommitted work.
The situation says so in one line: `Left out, because models may not read them: <paths>.`

A function whose body is a stub, a body half written, a failing test, a missing test file: none
is refused. Conjure continues your work in progress and keeps what already works.

## What conjure may write

The writer answers with whole files. A file lands as a change only when its path is inside the
project, not under `.git/` or `.sorcerer/`, not a `.env` file, not outside the project through a
symbolic link, not in your `[model] not_evidence` and not matched by `[conjure] exclude` (globs ignore case where your file system does, and
are matched on the path with links resolved, so a link into `gen/` is excluded too). The
paths that fail these are dropped and named in `not written:`. A file the writer answers twice
keeps its last version, whatever the spelling (`src/a.py` and `./src//a.py` are one file; `SRC/a.py` is too where your
file system ignores case, as macOS and Windows do by default, and another file where it does not), and a file equal to what is there is no change. A file with CRLF line
endings stays CRLF, and only the lines the writer changed are edited.

Conjure itself writes nothing to your project, and only its own memory under
`.sorcerer/state/conjure/`.

## Settings

```toml
# .sorcerer/config.toml
[conjure]
writer = "haiku"        # the model that writes the first attempt
try_harder = "sonnet"   # the stronger one; "" turns Try harder off
budget_per_day = 3.00   # dollars; calls stop when the day has spent it
exclude = []            # globs of files conjure never writes, such as "gen/**"
```

A key it does not know is an error naming it. The models are named by alias and the provider is
the project's default (see [What sorcerer is](what-sorcerer-is.md)); `conjure doctor` shows what
each alias means on it.

## What it costs

A change on haiku is about $0.001 and 4 seconds. Try harder asks the stronger model, which may
read, grep and glob the project before it writes, and costs at most $0.25 (a few cents in
practice). On `claude` the cap is enforced by Claude Code itself (`--max-budget-usd`); on a
provider run through sorcerer's own tool loop it is checked between the model's turns, so the last
turn may pass it. The day's budget is $3.00: a first call is refused once the day has spent it, and
Try harder is refused when the day could not pay its $0.25. The day is your local day. A call that failed after it was sent (a timeout, say) counts $0.02 toward
the day, since it was probably billed. The day's cap is soft, as scry's is: the call that crosses
it, or two runs started together, may go past it. The message names the setting:

```
conjure: today's budget ($3.00) is spent. Conjuring resumes tomorrow; "conjure cost" shows the days, and [conjure] budget_per_day sets the budget.
```

## What conjure never does

- write to your disk: the change lands in your editor's buffers, and saving it is yours;
- run on its own: it works where you ask;
- refuse a stub, a green function or a missing test file;
- name a language: what a test file is called and how an import finds its file are in the
  language file ([Adding a language](adding-a-language.md));
- call a model in a test: its tests and its eval replay recorded replies.

`conjure doctor` prints a row for git, the provider and what its aliases mean, your settings, the
languages (and whether each has its test and import tables), scry's findings file, the ledger,
the records and whether the editor's server needs the project trusted. `conjure trust` puts the
project on the same trust list as `scry trust`, for a project whose config runs code or moves
your key.
