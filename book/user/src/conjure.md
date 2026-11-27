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

Install `conjure` (the user book's install chapter has the lines; the release installers are
`sorcerer-conjure-installer.sh` and `.ps1`, or `cargo install sorcerer-conjure`). It needs
`claude` on your PATH, signed in, and a project you have trusted (`conjure trust` in it, when
the project's own config asks for anything that runs code); `conjure doctor` checks both.

**Neovim.** Install [sorcerer.nvim](https://github.com/sorcererdev/sorcerer.nvim) and turn
conjure on, which it keeps off until you ask:

```lua
require("sorcerer").setup({ conjure = { enabled = true } })
```

The plugin binds the keys: `<leader>mc` conjures here (select lines first for a range),
`<leader>mu` undoes the change, `<leader>mt` tries harder and `<leader>mw` says why. Each runs the
code action of that name on the cursor's line, so the line offers what it always did. `keys = false`
binds none, `keys = { undo = false }` skips one, and a key you already mapped is never overwritten
(`:checkhealth sorcerer` lists it).

The plugin starts `conjure lsp` on the files you open, one server per project root, and does two
things the server relies on: it reports every edit at once (`debounce_text_changes = 0`, so a
change never lands over what you typed a moment ago), and it attaches conjure to every buffer
under the project root, including the files a change opens and the files it creates, which in
Neovim are buffers not yet on disk (a file the server cannot see counts as not landed). Marks show in `vim.diagnostic` (`vim.diagnostic.setloclist()`), and a conjured change draws `✦` in the sign
column with its line number tinted (`SorcererConjured`, linking `DiagnosticSignOk`), apart from a
hole's `◇` and a rule finding's `≡`; `conjure.sign = false` gives Neovim's own display back.
`require("sorcerer").status()` says `conjure: conjuring a change` while one runs (joined to
scry's line when both work), and `:checkhealth sorcerer` checks that `conjure` is found. If you
do not use the plugin, `vim.lsp.start({ name = "conjure", cmd = { "conjure", "lsp" }, root_dir =
vim.fs.root(0, ".git"), flags = { debounce_text_changes = 0 } })` for every buffer under the
root does the same.

**Holes.** A hole that [spellbook](spellbook.md) left (a placeholder that parses and says what
belongs there) is code in a function like any other, so its line offers *Conjure here* with no
setup. With spellbook's `conjure_holes = true`, `:Spellbook new` runs it on the first hole once the
files are written; conjure writes the unit whole, so the holes of one unit are one change, marked
and undoable.

**VS Code.** The `Conjure` extension (`editors/vscode/conjure`) starts `conjure lsp` for each
workspace folder. It finds the binary from the `conjure.path` setting (user settings only; a
workspace's settings cannot choose the program it runs), then `PATH`, `~/.cargo/bin` and
`~/.local/bin`; with none, it offers to download its own version's binary, checked against its
checksum, as the Scry extension does. The actions are in the quick-fix menu (`Ctrl+.`), the
marks in the Problems panel and, for a conjured change, as a green gutter icon and an
overview-ruler tick. The keys are the Neovim plugin's, under a `ctrl+m` chord on every system
(`ctrl` is Control on macOS too): `ctrl+m c` Conjure here, `ctrl+m u` Undo, `ctrl+m t` Try
harder, `ctrl+m w` Show why; a line with no such action says so in the status bar. On Windows
and Linux the chord replaces VS Code's "Toggle Tab Key Moves Focus" (`ctrl+m`); rebind any of
them in Keyboard Shortcuts (search `conjure.keys`).

**Asking.** Put the cursor in a function, or select lines in one, and open the code-action menu
(`<leader>mc` in Neovim, or `vim.lsp.buf.code_action()`; `Ctrl+.` in VS Code). *Conjure here* asks for one change
there. On a line where scry has an open finding, *Conjure for this finding* asks for a change led
by that finding. Outside a function nothing is offered (`put the cursor in a function to
conjure` if you ask anyway).

**When the code does not parse.** A stray line such as `if the party is too big say so` can open a
block the file never closes, and the parser then loses the whole function. Conjure does not give
up there: it takes the stretch of code the parser marked as an error around your cursor, tells the
writer that the code does not parse and is to be repaired, and asks for the change as usual
(*Conjure here* is offered on those lines too). Only a line that is in no function and in no such
stretch still gets `put the cursor in a function to conjure`.

**Waiting.** A progress line, `conjure: conjuring a change`, for the few seconds the writer takes,
and grey text at the end of the first line of the unit being changed, `⟳ conjure: writing…`
(Neovim and VS Code; it follows the line as you type and goes once the change has landed, or has not).
Asking again meanwhile is refused with `conjure: a change is already being conjured; this one was
not started`; the running one goes on.

**The change lands.** One edit carries every file: a file you have open is edited where you see
it (only the lines the writer changed); a file you do not have open opens as an unsaved buffer; a
new file is created by the editor and filled in your buffer (VS Code creates it on disk, empty;
Neovim makes only the buffer, and the file is on disk once you save it). Nothing is saved:
the change is in your buffers, yours to read, keep or undo. Conjure reads your unsaved text when
it asks, and the edit is made against exactly that text: if a file the change touches changes
while it is being conjured (you type in it, or a tool rewrites it on disk, or a new file it would
create appears), nothing lands and you are told `conjure: the file changed while the change was
conjured; your text stands`. Once the editor says it applied the edit, conjure checks that each
file reads as the change made it. When one does not (an editor may skip a file and still say it
applied the edit), you are told `conjure: the change did not land as sent in <files>; the other
files were changed, and conjure will not undo them`, the marks end with ` · did not land as sent:
<files>, so no Undo`, and the change is recorded `landed-in-part`. Each file is named by its name,
or by its path when two files of the change share a name (`src/ports/mod.rs`,
`src/ports/walk_in_log/mod.rs`). An editor that cannot create files gets the rest of the change and the line
`conjure: this editor cannot create files; not created: <paths>`.

**The marks.** On the first changed line of the file you asked in, an information mark says what
the change did:

```
conjured: fill total · haiku · $0.001 · 4 s · parses · also changed: b.py, a__test.py
```

it starts `conjured:` and the goal (its first word in lower case), then the model, the cost, the time, whether the file parses (or `does not parse (line 12)`),
the other files, and `not written: <paths>` when the writer named a path conjure may not write
(below). Each other changed file has its own mark, `conjured with a.py: fill total` (the goal the same way, and `does not
parse (line N)` when it does not). Marks are diagnostics: they show in Neovim's diagnostic list and
VS Code's Problems panel. One change is marked at a time.

**The actions on a mark**, in the code-action menu on its line:

- *conjure: Undo* puts every file back as it was before the change, in one edit: a new file is
  deleted (emptied, and said, in an editor that cannot delete files). If you have edited a
  changed file since the change landed, Undo does nothing and says `conjure: <file> was edited
  after the change landed; undo it with the editor's own undo`: your typing is never thrown away.
  A file you put back by hand needs nothing. While a change is being conjured, Undo waits its
  turn: it is refused with the busy sentence.
- *conjure: Try harder* undoes the change, then asks the stronger model (`[conjure] try_harder`)
  with what the first writer read; it may read and search the project first. Its change lands
  and is marked like the first. It is not offered when `try_harder = ""`. If the editor does
  not report the undo within 3 seconds, Try harder does not start (`conjure: the editor did not
  report the undo, so Try harder was not started`) rather than write over text it cannot see.
- *conjure: Show why* opens `.sorcerer/state/conjure/why-<id>.md`: what the change did, the
  situation the writer read, its reply as it came, and what the code said about the function
  before.
- *conjure: Show `<file>`*, one for each other changed file, opens it with the change selected.
  Two changed files that share a name are told by their paths (`Show src/ports/mod.rs`, `Show
  src/ports/walk_in_log/mod.rs`), as in the step's message.

**When a mark goes away.** Saving a changed file keeps the change: that file's mark goes, and the
other files keep theirs, now ending ` · kept: a file was saved, so no Undo`, until each is saved
or closed, so an unsaved part of the change never slips by unmarked. Undoing it (with the action,
or with the editor's own undo until every file is back) clears every mark; Try harder replaces
it; the next change keeps it. Closing a file hides its mark until you open it again (once the
change is kept, closing a file ends its mark). Conjure records each change once, at the choice
that ended it (`kept`, `undone`, `replaced`), or `dropped` when nothing landed,
`landed-in-part` (above) and `unanswered` when the editor closed with the change still marked;
`conjure log` lists them.

**Told after every step.** When a step ends, the editor shows one message, so a change to a file
you are not looking at is not missed: `conjure: changed seat_party__test.py (1 line),
seat_party.py (2 lines) · <goal>` (each file by its name, or its path from the root when two share
a name; the lines are the ones changed). A step that changed nothing says `conjure: no change ·
<goal>`, or, when the writer's goal is `nothing`, `conjure: no change · nothing left that I can see;
add a test case or a comment that says what comes next`. A file refused or dropped is a warning
instead (below).

**No change.** When the writer finds nothing to do, a mark on the function's first line says
`no change · <goal>`, with Try harder and Show why. It goes at your next edit or save.

**Failure.** A model that does not answer (a timeout, say) is one message, `conjure: the model did
not answer: <why>`, and no mark. A spent day says its sentence (below) and calls nothing.

**Trust.** A project whose config runs code or moves your key (a provider's `runner` or `url`, a
grammar of its own) is not conjured in until you trust it: asking says `conjure: this project is
not trusted; run "conjure trust" in it`. Until then conjure does not parse its files (a
grammar of the project's own is never built or loaded), so *Conjure here* is offered on any line.
`conjure trust` and `scry trust` keep one list.

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
conjure cost [--since DAY] [--json]   # what conjure spent, a local day a row: changes, try harder, retries, cost
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
budget is spent, the model failed, or the arguments are wrong. A change held back by your
spellbook rules (below) is `no change · held back: …`, exit 0. Each call is recorded in the
ledger, so `conjure cost` and `scry cost` share one account. The command line keeps no record
of changes; `conjure log` shows what the editor recorded.

## What conjure reads

For the function at the line, in this order, each part cut as shown:

- the whole file, and its test file, or where its tests would go (`[test]` in the language file);
- the files it imports, at most 8, each whole (`[imports]` and `imports.scm`);
- near each imported file, the files in its folder that use a name it defines, then the file one
  folder up that names its folder (at most 4 for one import, 6 in all, each whole): a port's
  adapters and contract test come with the port, and so does the file that declares the ports
  (`ports/mod.rs` with `pub mod clock;`), so a step can extend a port and its adapters, or add a
  port beside it. A fenced file is skipped, not named.

  A related file is never cut. The related files of one step share a budget of about 6,000
  tokens (some 600 lines): the imports the function uses come first, then the other imports,
  then the files near them. A file that does not fit in what is left is left out whole, and
  the situation names it (`Also related, but left out for room …`), so the writer knows it
  exists and does not rewrite it unseen. A smaller file after it may still fit;
- what you have been doing: the uncommitted changes in files of the same language, cut at 300
  lines, and the names of up to 20 new files;
- scry's open findings near the function, at most 5, as context the writer may act on or not
  (conjure reads scry's `.sorcerer/state/scry/findings.json` when it is version 2);
- your project's rules, from `.sorcerer/scry.toml` (its `context`, its `include`d files and its
  rules), cut at 3000 characters;
- with `--finding`, that finding leads;
- when the file, its test file or a related file holds a spellbook region, one line: *Lines inside a
  spellbook:region are generated from files: to add one, create the file, and saving adds its
  line.* The region's own comment is unchanged; the line keeps a writer from typing a module into
  the list, or from thinking it may not add one.

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
endings stays CRLF, and only the lines the writer changed are edited. An editor that keeps a new file with CRLF lines (VS Code on Windows) still counts as having landed the change, and Undo restores it without changing its line endings.

### A change keeps your spellbook rules

When the project has spellbook rules (`.sorcerer/spellbook/rules.toml`) and `spellbook` is
installed (beside `conjure`, or on PATH), conjure checks each answer against them before it lands,
with `spellbook check --texts - --json`: the new texts are checked as the files they would become,
and nothing is written. Only an `error` the change adds counts; a finding the code already had, a
`warn` and a `hint` do not.

The writer is told the rules before it writes. Conjure asks `spellbook rules FILE --json` for the
rules that look at the file you are in (one more, quick, call to spellbook for each change), and
the situation ends with them: each rule's id, severity, message and why, and the instruction to
add what the code lacks (a method on a port, an adapter) rather than break one. Most answers then
keep the rules the first time. The check after the answer stays, as the guarantee.

An answer that breaks a rule is not used. Conjure asks the writer once more, with the situation
and, after it, each broken rule, its line of code and its message, and the instruction to add
what the code lacks (a method on a port, an adapter) where it belongs, or to change nothing and
say what is missing. The command line then prints `asked again: the first answer broke
commands-use-ports (src/booking/commands/seat_walk_in.rs:36)`. When the second answer breaks a
rule too, nothing lands: `conjure: no change · held back: it breaks commands-use-ports at
seat_walk_in.rs:36 (Commands and queries talk to ports, not the file system (ports & adapters)),
asked twice`. Asking again is a second call: its cost is added to the change's, the ledger
records it as a `conjure-retry` (so `conjure cost` shows it in its own `retries` column and does
not count it as a second change), and it is not made when the first call spent the day's budget (the change is held back instead).

#### What a retry dropped

A second answer can keep the rule by deleting what the first did instead of moving it: the
first wrote `std::fs::write(…)` in a command, the second just leaves the line out, and the
walk-in is no longer saved. Conjure never holds a change back for this, but it says it.

After a retry that keeps the rules, conjure compares the two answers by the language's queries:
each call (named by its callee as written, `std::fs::write`, `self.clock.record`) and each
definition the first answer added and the second has nowhere in the change. A call the second
answer moved into another file of the change (an adapter) is not a drop, and neither is a
placeholder the first answer left alone. One drop is kept per line.

Each drop is put to Jev, when `JEV_API_KEY` is set, as one question (one call for all of them, at
most 5): was the code not needed (the rule asked for its removal), does other code in the second
answer do the same job (moved elsewhere), or was behaviour lost? When at least 0.7 of Jev's
answer is on the first two together, the drop is legitimate; at most 0.3, it looks unintended;
between, Jev is unsure. A Jev call costs a fraction of a cent and is counted in `conjure cost`'s
total (verb `decide`).

- A drop that looks unintended, an unsure one, and one Jev was not asked about (no key, the call
  failed, more than 5) are in the step message: `conjure: changed seat_walk_in.rs (1 line) ·
  seat the walk-in · asked again for commands-use-ports · dropped: std::fs::write in
  seat_walk_in.rs (looks unintended)` (`maybe unintended`, `not checked`). The command line prints
  `dropped: …` on its own line.
- A drop Jev calls legitimate is said quietly: on the change's mark (`dropped on purpose:
  std::fs::write in seat_walk_in.rs (moved elsewhere)`), on the command line, and in the change's
  record (`retry_drops` in `changes.jsonl`, every drop with its verdict).

The diff and Undo are as for any change. The drops that are said are also left for scry, in
`.sorcerer/state/conjure/drops.json` (the newest 20; Undo and Try harder take a change's back):
scry's next review of that file is told the dropped code, the rule and Jev's verdict, while the
file still lacks it, so it can raise a finding where behaviour went missing.

Each call to spellbook waits at most 10 seconds. When the check times out (spellbook hung or the
machine is busy), the change goes ahead unchecked and says so: its goal ends `(rules not checked:
the check timed out)`, and the command line prints `rules not checked: the check timed out`.

Without spellbook, or without rules, changes are not checked, and `conjure doctor`'s
`spellbook rules` row says so (`not checked: spellbook is not installed`). It is never a
failure: conjure works without spellbook.

Conjure itself writes nothing to your project, and only its own memory under
`.sorcerer/state/conjure/` (its records, and the drops it leaves for scry).

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
languages (and whether each has its test and import tables), scry's findings file, whether changes
are checked against spellbook's rules, the ledger,
the records and whether the editor's server needs the project trusted. `conjure trust` puts the
project on the same trust list as `scry trust`, for a project whose config runs code or moves
your key.
