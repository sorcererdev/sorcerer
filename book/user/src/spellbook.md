# Spellbook

Spellbook keeps a project's conventions as data in `.sorcerer/spellbook/`. Its first milestone
does one thing well: a **generator** writes every file of a convention (a command and its test,
say) after showing you what it will write, and leaves **holes** where the code needs a person,
an agent or a model. Each hole is code that parses and says what belongs there.

Spellbook is the third tool of the suite, after scry and conjure. It **costs nothing and calls no
model**: it needs no key and no `claude`. When conjure is installed, conjure fills holes in the
editor; spellbook itself never spends.

What is here and what is coming (the five parts of spellbook):

| part | state |
|---|---|
| generators with holes | here: `spellbook new`, `spellbook holes`, and `:Spellbook new` in Neovim |
| rules that check the code | here: `.sorcerer/spellbook/rules.toml`, `spellbook check`, and findings on save in the editor |
| fixes (a rule names the generator that repairs a finding), insert edits and `[[uses]]` | here: `spellbook fix` on the command line and, in VS Code and Neovim, the quick fix and the fix key (`ctrl+m f`, `<leader>mf`) |
| owned regions (`pub mod` lines a generator keeps), list inputs, `if`/`for` in templates | milestone 2 |
| packs of generators shared between projects | milestone 2 |
| suggestions (a convention noticed in your code) | milestone 3 |

## A generator, step by step

A generator is a directory: `.sorcerer/spellbook/generators/<name>/` holding a
`generator.toml` and its `templates/`. The name of the directory is what `spellbook new`
takes. This one writes a command of a Python service and its failing test:

```toml
# .sorcerer/spellbook/generators/command/generator.toml
name        = "Command"
summary     = "A command of a service, with its test"
language    = "python"
examples    = ["spellbook new command --set service=booking --set command=cancel_booking"]

[inputs.app]
type   = "enum"
one_of = "apps"
prompt = "The app (the folder under src/ with main.py)"

[inputs.service]
type   = "enum"
one_of = "services"
prompt = "The service it belongs to"

[inputs.command]
type   = "ident"
prompt = "The command, a verb phrase (cancel_booking, close_table)"

[query.apps]
glob  = "src/*/main.py"
value = "parent folder"

[query.services]
glob  = "src/{= app =}/*/models"
value = "parent folder"

[derive]
Command  = "{= command|pascal =}"
command_ = "{= command|snake =}"
dir      = "src/{= app =}/{= service|snake =}/commands"

[[edits]]
create    = "{= dir =}/__init__.py"
text      = ""
on_exists = "skip"

[[edits]]
create   = "{= dir =}/{= command_ =}.py"
template = "command.py.dt"

# A hole the inputs fill: nobody has to write it.
[holes.purpose]
source = "derived"
value  = "{= command|sentence =}."
```

And a piece of its template, `templates/command.py.dt`:

```text
"""{= ? purpose: text | what this command does =}"""

@dataclass(frozen=True)
class {= Command =}Input:
    {= ? field: ident | its first input =}: {= ? type: type | that input's type =}

class {= Command =}:
    def handle(self, input: {= Command =}Input) -> {= Command =}Output:
        {= ? handle: block | its first case, from the test file beside it =}
```

### The keys

| key | what it is |
|---|---|
| `name`, `summary`, `description`, `examples` | shown by `spellbook list` and `spellbook show` |
| `language` | the language the generator is for; it must be one of the project's languages |
| `[inputs.X]` | `type`: `ident` (a name in any case style: `cancel_table`, `CancelTable` and `cancel-table` are the same name), `enum` (its values are `one_of` a query over the project's files, or a fixed list, `choices = ["command", "query"]`, whose `default` must be one of them; not both) or `text`; `prompt`; `optional`; `default`. Inputs are asked in the order the file writes them |
| `[query.X]` | `glob`, a template over the inputs before the one that uses it, and `value`: `"parent folder"`, `"stem"` or `"path"` of each match. In the glob `*` stays inside one folder and `**` crosses folders; a glob may name folders as well as files |
| `[derive]` | names computed from the inputs, in order; each may use the ones before it |
| `[[edits]]` | `create` (the path, a template), then `template` (a file under `templates/`) or `text` (written as it is), and `on_exists`: `"error"` (the default: the plan stops, nothing is written) or `"skip"`. Or `insert`: lines put in a file that is there (below) |
| `[holes.X]` | `source = "derived"` and `value`: a hole filled from the inputs |
| `[[uses]]` | another generator planned into the same plan after this one's edits: `generator` (its id) and `with` (its inputs, templates over this generator's values). Its files, changes and holes join the plan, and it sees the files planned before it (`add-port` uses `port`, then `use-port`, which finds the port `port` just planned). A used generator that is missing, a `with` input it does not take, a chain deeper than four or a cycle is refused when the generator loads |
| `context` | olifant's key (files a filler reads); kept for a later milestone |

Keys that arrive later in milestone 2 (`type = "list"`, `ast` queries,
`{= if =}`/`{= for =}`) are refused with a sentence that says so. A slip gets the
word to write instead: `content` → `text`, `pascalCase(x)` → `{= x|pascal =}`, `{{ }}` → `{= =}`.

### Insert edits

An `insert` edit puts lines in a file that is there, where a phrase says:

```toml
[[edits]]
insert = "{= module =}"
at     = "end of parameters in {= Unit =}.__init__"
text   = "{= param =}: {= Port =}"
unless = { matches = "\\b{= param =}: {= Port =}\\b" }
```

| key | what it is |
|---|---|
| `insert` | the file to change, a template. Exactly one of `create`/`insert` per edit |
| `at` | where the text goes: a phrase, or a list of phrases tried in order (the first that resolves wins) |
| `text` / `template` | the lines to insert, rendered as a `create` body is (holes included), indented like what they go next to, in the file's own line ending |
| `unless = { matches = REGEX }` | the edit is left out (`=`) when any line of the file matches the regex (a template, filled first). This is what makes running a generator or a fix twice a no-op |
| `when = { found = PHRASE }` | the edit happens only when the phrase resolves in the file |
| `on_missing` | when the file is not there or no phrase resolves: `"error"` (the default: the plan stops, naming the edit and the phrase) or `"skip"` |

The phrases are `<position> <target>`: `start of`, `end of` (inside the target, before its first
or after its last item), `before`, `after` (outside it).

| target | `end of` means |
|---|---|
| `imports` | after the file's last top-level import (in a file with none, after its leading comments and docstring) |
| `function NAME`, `class NAME` | after the last statement of the body, indented like it. `NAME` may be a method, `Class.method` (`Class::method` in Rust) |
| `parameters in FUNCTION` | after the last parameter, with the list's own separator: on the line when the list is written on one line, on a line of its own when it is one per line |
| `call NAME`, `call NAME in FUNCTION` | after the call's last argument, by the same rule; an empty `()` takes the text alone |
| `marker TEXT`, `first line matching R`, `last line matching R` | `before` or `after` that line (any file, no grammar) |
| `file` | `start of file` or `end of file` |

A target found in several places (two `CloseTable(…)` calls) stops the plan and asks you to
narrow it with `in FUNCTION`. A phrase with no `{= =}` is checked when the generator loads; one
with a value is checked once filled. The structural targets read the queries every shipped
language has (`imports.scm`, `tags.scm`, `textobjects.scm`), so a language you add gets them by
having those queries.

A plan shows a changed file as `~ path (+3 −1)`, and `--plan` prints its diff. A plan whose every
insert is in place already and that writes nothing new says `nothing to do: every edit is in the
code already` and exits 3.

### Templates

- `{= name =}` is an input's or a derived name's value, as it was typed.
- `{= name|filter =}` writes it in a case style: `snake` (`cancel_table`), `pascal`
  (`CancelTable`), `camel` (`cancelTable`), `kebab` (`cancel-table`), `upper` (`CANCEL_TABLE`),
  `sentence` (`Cancel table`), `words` (`cancel table`). Filters apply left to right.
- `{= ? name: kind | instruction =}` is a hole. The same hole may appear in several places (and
  several files); it is one hole, with one kind and one instruction. The instruction is plain
  text: no quotes, backslashes, braces or new lines, since the placeholder puts it inside a string.

## The generators of the corpus

The repository ships two small projects with generators you can read and copy, in
`corpus/spellbook/`. `bistro` is Python (`command`, `query`, `port`, and the two its fix uses:
`use-port`, which inserts a port into a command and its test, and `add-port`, which uses `port`
then `use-port`); `bistro-rs` is Rust (`command`, `port`). Take bistro's `command` end to end:

```
$ spellbook new command --set command=cancel_table
command (2 files, 3 holes)
  = src/bistro/booking/commands/__init__.py            (exists, skipped)
  + src/bistro/booking/commands/cancel_table.py        (30 lines)
  + src/bistro/booking/commands/cancel_table__test.py  (12 lines)
```

`cancel_table.py` holds its input, a failure, a `handle` and the placeholders `todo_field`,
`object` and `raise NotImplementedError("hole handle: its first case, from the test file
beside it")`; `cancel_table__test.py` calls it with `todo_field="1"` and runs red until the holes are
filled. `spellbook holes` lists four places (`field` is in both files), and three once you have
replaced the `handle` line.

The Rust `command` writes `src/booking/commands/cancel_table.rs` the same way, its `handle` body a
`todo!("hole handle: …")`. It does not touch `commands/mod.rs`: add `pub mod cancel_table;` yourself.
Until you do, the new file is not compiled; once you do, it compiles (with an unused-variable
warning) and its test fails red, as the convention intends. Keeping a `pub mod` line for you is
what an owned region will do in milestone 2.

### Writing your first generator

1. Copy a generator that is close to what you want:
   `cp -r corpus/spellbook/bistro/spellbook/generators/command .sorcerer/spellbook/generators/job`.
2. Open `generator.toml` and change the `name`, `summary`, `description` and `examples`, the `dir`
   in `[derive]` and the `create` paths in `[[edits]]`.
3. Change a template in `templates/`: replace the code you do not want and keep the
   `{= ? name: kind | what belongs here =}` holes where a person decides.
4. Run `spellbook show job`: it lists the inputs with their choices, what each edit creates and every
   hole, with the derived ones marked. A mistake in a key or a template is named here, with the file.
5. Run `spellbook new job --set job=nightly_report --plan`: it prints the files and holes and writes
   nothing. Take off `--plan` when it reads right.

## Holes and who fills them

A hole that no `[holes.X]` fills becomes a **placeholder**: the code its kind gives in the
language of the file it lands in (the language's `[holes]` table, see
[Adding a language](adding-a-language.md)). Python's:

| kind | placeholder |
|---|---|
| `ident` | `todo_field` |
| `type` | `object` |
| `expr` | `None  # hole value: what it returns` (only where the hole ends its line) |
| `block` | `raise NotImplementedError("hole handle: its first case, …")` |

Rust's `type` is `()`, and its `expr` and `block` are `todo!("hole handle: …")`. Every
placeholder parses, so the generated file parses, and the test the generator wrote runs and
fails red until the holes are filled.

Who fills them: you, by hand; an agent such as Claude Code, which reads `spellbook new --json`
or `spellbook holes --json` and finds every hole with its file, line and instruction; or conjure
in the editor.

## `spellbook new`

```
$ spellbook new command --set command=cancel_table
command (2 files, 3 holes)
  = src/bistro/booking/commands/__init__.py            (exists, skipped)
  + src/bistro/booking/commands/cancel_table.py        (30 lines)
  + src/bistro/booking/commands/cancel_table__test.py  (12 lines)
holes:
  field   src/bistro/booking/commands/cancel_table.py:10  ident  its first input
  type    src/bistro/booking/commands/cancel_table.py:10  type   that input's type
  handle  src/bistro/booking/commands/cancel_table.py:30  block  its first case, from the test file beside it
(filled from the inputs: purpose "Cancel table.", failure "NotFound", value "None")
```

`app` and `service` were not asked: each query found one choice. An input that is not given and
that the project does not decide stops with its prompt and its choices, and exits 2:

```
$ spellbook new command --set command=cancel_table
spellbook: service: the service it belongs to (booking, kitchen)
  give it with --set service=VALUE
```

`--plan` prints the same and each file's text, and writes nothing. `--json` prints
`{ "plan", "applied", "written", "skipped", "holes" }` (a stop prints `{ "stop", "input",
"choices" }`).

Before anything is written, the whole generation is planned and checked: every path stays
inside the project (no `..` out, no absolute path) and off the fences (`.env` files,
`.sorcerer/`, `.git/`, `[model] not_evidence`); an existing file stops the plan unless its edit
says `skip`; and every new file a language covers is parsed, so a template mistake names the
template and the line instead of landing broken code. Writing then refuses again any file that
appeared since the plan, and any path a link leads out of the project.

A generator writes the files it creates and nothing else: a Rust module still needs its
`pub mod` line, which is yours to add until owned regions arrive (milestone 2).

## `spellbook holes`

```
$ spellbook holes
field   src/bistro/booking/commands/cancel_table.py:10        ident  its first input
type    src/bistro/booking/commands/cancel_table.py:10        type   that input's type
handle  src/bistro/booking/commands/cancel_table.py:30        block  its first case, from the test file beside it
field   src/bistro/booking/commands/cancel_table__test.py:11  ident  its first input
4 open holes
```

A hole stays open while its placeholder is in the file: a placeholder that says `hole <name>:`
is found by that text anywhere in the file, a short one (`todo_field`) on its line or within 20
lines of it. Fill it and it is gone. `spellbook holes FILE…` looks only at those files (`./path`, a path from where you stand or an absolute path inside the project all work; one outside it is refused);
`--json` prints the rows.

## The other commands

```
spellbook list                     the generators, a row each (a broken one is a row with its problem)
spellbook show <generator>         its inputs and their choices now, the files it creates, its holes, its examples
spellbook check [FILE…] [--json]   run the rules over the project (or the named files)
spellbook doctor                   the config, the generators, the rules, the languages and their hole kinds,
                                   whether conjure is installed, trust, holes.json, report.json, generations.jsonl
spellbook trust [--revoke]         let the editor's server run here although the project's config runs code
spellbook lsp                      the editor's language server (the editor starts it; see below)
```

`spellbook doctor` says "holes can be conjured in the editor" when `conjure` is on the PATH, and
"holes are filled by hand; install conjure to fill them with a model" when it is not.

## Rules and check

A rule is a convention that checks itself. Rules live in `.sorcerer/spellbook/rules.toml`, one
`[rules.<id>]` table each:

```toml
[rules.commands-use-ports]
files    = "src/{app}/{service}/commands/*.py"
exclude  = ["**/__init__.py", "**/*__test.py"]
match    = { any = [
  { kind = "import_statement", regex = "sqlite3" },
  { kind = "import_from_statement", regex = "^from sqlite3" },
] }
message  = "Commands talk to ports, not the database"
```

This rule names no severity, so it is an error.

| key | meaning |
|---|---|
| `files` | a path pattern: `*` is part of one folder or file name, `**` any number of folders (`**.py` is `**/*.py`), `{name}` a name that must be there (it binds one folder, or the part of a name between the text around it). Required |
| `exclude` | a list of path patterns no finding is made in |
| `match` | the short form, below (write this or `query`) |
| `query` | a [tree-sitter query](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/index.html) in the file's language; every `@finding` capture is a finding. A query with no `@finding` is refused when the rule first runs |
| `message` | the sentence a finding says. Required |
| `severity` | `hint`, `warn` or `error` (the default: a rule that names no severity is an error) |
| `since`, `why` | free text for people: when the convention began and why it holds |

| `fix`, `[[rules.ID.fixes]]` | the generators that repair a finding: see [Fixes](#fixes) |

`missing` (a rule that says what to add) arrives in milestone 2b: a rules file that has it says so
and stops.

The short form is `{ kind, regex, not = { regex }, any = [ … ] }`. `kind` is the syntax node's kind
in the language's grammar (`import_statement`, `call_expression`); `regex` must match the node's
text (the Rust `regex` syntax), `not` must not, and `any` is a list of forms of which any one is a
finding. Spellbook writes it out as a query, so the two forms run on one engine. A line gets at most
one finding for each rule.

`spellbook check` runs every rule over every file whose language the project knows (or just the
files you name) and prints a row for each finding:

```
$ spellbook check
✗ src/bistro/booking/commands/close_table.py:4  commands-use-ports — Commands and queries talk to ports, not the database  (fixes: Use a port the service has)
1 blocking finding
```

A row ends with `(fixes: … · …)` when the rule has fixes.

`!` is a warning, `·` a hint, `✗` an error. A rule is an error unless it says `severity = "warn"`
or `"hint"`. The exit code is 0 when the conventions hold (hints and warnings do not fail), 1 when a
finding is an error, and 2 when it could not run (the rules file has
a mistake, which the message names). `--json` prints `{ "checked", "findings": [ … ] }`: each finding
has `rule`, `file`, `line`, `severity`, `message` and a `fingerprint` (seven hex digits that stay the
same when the code is moved or re-indented), and, when its rule has fixes, `fixes`: `[{ "index",
"label", "generator", "inputs", "missing" }]`, the inputs the finding settles and the ones still to
give, so an agent can run the fix without reading the rules file (`report.json` does not keep
them). `[spellbook] exclude` in `config.toml` lists files no
rule sees.

To let one place break a rule, say why in a comment on the line or the line above:

```python
# spellbook:allow commands-use-ports — migration, tracked in issue 12
import sqlite3
```

Only a real comment counts: the same words inside a string are ignored.

The reason follows an em dash (` - ` and ` -- ` work too). An allow without a reason still silences
the finding, and leaves a warning, `allow-without-reason`, on the comment; an allow that names no rule
(a typo) silences nothing and is an `unknown-rule` warning.

In the editor, the server checks a file when you open it and when you save it, and shows each finding
as a diagnostic on its line (a hint, warning or error, source `spellbook`, the rule's id as its
code), beside the marks of the holes in the file. Edit `rules.toml` and the next save uses it. A
`rules.toml` that does not load is said once in the editor, and `spellbook doctor` shows the
problem.

The last check is kept in `.sorcerer/state/spellbook/report.json`.

## Fixes

A rule can name the generators that repair its findings, best first:

```toml
[[rules.commands-use-ports.fixes]]
generator = "use-port"
label     = "Use a port the service has"
with      = { app = "{= app =}", service = "{= service =}", kind = "{= kind =}", unit = "{= stem =}" }
replace   = [
  { enclosing = ["import_statement", "import_from_statement"], text = "" },
  { enclosing = ["expression_statement", "return_statement"], text = "{= ? port_call: block | the same as {= finding =} through self.{= param =} =}" },
]
```

| key | what it is |
|---|---|
| `fix = "ID"` | the short form: one fix, labelled with the generator's `name`, whose `with` passes each slot of `files` on as the input of that name |
| `generator` | a generator of the project |
| `label` | what `check` and the editor show; a template over the same names as `with` |
| `with` | the generator's inputs, each a template over the slots of `files` (`{= app =}`), `{= dir =}` (the file's folder), `{= stem =}` (its name without extension), `{= file =}` (its path) and `{= finding =}` (the found code, on one line). An input not in `with` is asked (an enum with one choice is taken) |
| `replace` | cases tried in order: the first whose `enclosing` node kind is around the finding (the found node itself counts) has that node's whole lines replaced by `text`. An empty `text` deletes them; otherwise it is a template over the generator's inputs and derived names and `finding` (the code replaced), and may hold holes. No case matching stops the plan, naming the node's kind. Without `replace`, the found code stays |

Write `fix` or `fixes`, not both. Each inline table sits on one line (TOML does not let an inline
table span lines). A fix that names no generator of the project, gives an input the generator does
not have, or uses a name in `with` that is neither a slot nor one of `dir`, `stem`, `file`,
`finding` makes the rules file broken: `check` exits 2 naming it and `doctor` shows it.

On bistro, with `import sqlite3` added to a generated command:

```
$ spellbook fix src/bistro/booking/commands/close_table.py:4
spellbook: port: the port it takes (one of the service's ports/) (booking_repository, id_generator, table_repository)
  give it with --set port=VALUE

$ spellbook fix src/bistro/booking/commands/close_table.py:4 --set port=table_repository --plan
Use a port the service has: use-port (2 files changed, 0 new, 0 holes)
  ~ src/bistro/booking/commands/close_table.py        (+3 −2)
  ~ src/bistro/booking/commands/close_table__test.py  (+2 −1)
--- src/bistro/booking/commands/close_table.py
@@ -3,5 +3,5 @@
 from dataclasses import dataclass
-import sqlite3
 
 from bistro.shared.result import Ok
+from bistro.booking.ports.table_repository.table_repository import TableRepository
 
@@ -26,4 +26,5 @@
 class CloseTable:
-    def __init__(self) -> None:
+    def __init__(self, table_repository: TableRepository) -> None:
         """Takes the ports it uses; main.py passes their adapters."""
+        self.table_repository = table_repository
 
--- src/bistro/booking/commands/close_table__test.py
@@ -2,2 +2,3 @@
 from bistro.shared.result import Ok
+from bistro.booking.ports.table_repository.in_memory_table_repository import InMemoryTableRepository
 
@@ -5,3 +6,3 @@
 def command() -> CloseTable:
-    return CloseTable()
+    return CloseTable(table_repository=InMemoryTableRepository())
 
nothing written (--plan)
```

The second fix, `--fix 2` (or `--fix "Add a port and use it"`), asks `port` as a new name and plans
the `port` generator's four new files and the same wiring in one plan, `+` rows above `~` rows.

Without `--plan` the two files are changed and `spellbook check` is clean; run it again and it says
`nothing to do: the fix is already applied` (exit 3). Had the finding been `conn =
sqlite3.connect(path)` (the `attribute` form of the match), that statement would become
`raise NotImplementedError("hole port_call: the same as conn = sqlite3.connect(path) through
self.table_repository")`, a hole like any other.

`spellbook fix FILE:LINE` takes the finding on that line (`--rule ID` picks when two rules fire on
it), `--fix N|LABEL` (the first fix when not given), `--set` for the inputs the finding does not
settle, `--plan` and `--json` (the plan). The exit code is 0 when it was applied, 1 when the plan
stopped (the finding is gone, no `replace` case fits, a phrase does not resolve) or a file was
refused, 2 when an input is to give, and 3 when there is nothing to do.

What keeps a fix safe:

- **Planned before written.** The finding is found again in the file as it is now; every new and
  changed file a language covers is parsed; a phrase that does not resolve stops the plan.
- **The plan is the whole change.** `--plan` shows new files whole and changed files as hunks.
- **Changed files land first, and only onto the text the plan saw.** A file edited since the plan
  is refused and nothing is written. New files are never written over a file that appeared.
- **Twice is a no-op.** Each insert has its `unless`, so the second run has nothing to do.
- **Nothing runs by itself.** A fix is an offer: never on save, never from `check`. A finding still
  has its other exit, `# spellbook:allow RULE — reason`.

A fix never calls a model, never edits a file it did not show, and never touches `main.py` unless a
generator's edit says so.

In an editor, the language server (`spellbook lsp`) offers each fix as a quick fix on the finding's
line, `spellbook: Use a port the service has` first (preferred) and `spellbook: Add a port and use
it` after. Applied from the editor, the changed files land as one edit you can undo in each file
(they are left unsaved, for you to look at and save), then the new files are written. If you typed
in the file since the plan, nothing is applied and you are told `close_table.py changed since the
plan; nothing was applied: fix again`. An editor that cannot apply edits gets the changed files
written to disk, and is told so once. See [Fixes in VS Code](#fixes-in-vs-code) and [Fixes in Neovim](#fixes-in-neovim).

## In Neovim

Install `spellbook` (`sorcerer-spellbook-installer`, the same line as scry's with `spellbook` for
`scry`, or `cargo install sorcerer-spellbook`) and [sorcerer.nvim](https://github.com/sorcererdev/sorcerer.nvim),
and turn it on, which the plugin keeps off until you ask:

```lua
require("sorcerer").setup({ spellbook = { enabled = true } })
```

The plugin binds `<leader>mn` to `:Spellbook new`, `<leader>mf` to `:Spellbook fix` and `<leader>mk` to `:Spellbook check` (and
conjure's `<leader>mc/u/t/w` when conjure is on; see [Conjure](conjure.md)). `keys = false`
binds none; a key you already mapped is skipped and listed by `:checkhealth sorcerer`.
`:Spellbook check` runs `spellbook check --json` in the project root and fills the quickfix list with the findings
(opened when there are any, else `spellbook: conventions hold`); findings on the file you save
already show as marks.

The marks have their own looks, apart from scry's: a hole is `◇` (`SorcererHole`, linking
`Special`) with its placeholder underlined (`SorcererHolePlaceholder`, `DiagnosticUnderlineHint`); a
rule finding is `≡` in its severity's colour (`SorcererRuleError`, `SorcererRuleWarn`,
`SorcererRuleHint`, linking `DiagnosticSign*`); a conjured change is `✦` (`SorcererConjured`).
`spellbook.signs = { hole = "◇", rule = "≡" }` sets the glyphs and `false` gives Neovim's own
display back (`conjure.sign` for the star).

| setting | default | what it does |
|---|---|---|
| `spellbook.enabled` | `false` | start `spellbook lsp` for the files you open, one server per project root |
| `spellbook.cmd` | `{ "spellbook", "lsp" }` | the program, when it is not on your PATH |
| `spellbook.conjure_holes` | `false` | after `:Spellbook new`, run conjure on the first hole (needs conjure on too) |
| `spellbook.hole_keys` | `{ next = "<Tab>", prev = "<S-Tab>" }` | after `:Spellbook new`, the keys between the opened file's holes (insert and select mode, that buffer only); `false` = no session |

`:Spellbook new [generator] [input=value…]` takes five steps, with Neovim's own pickers
(`vim.ui.select`, `vim.ui.input`, so a picker plugin such as telescope or snacks shows them):

1. **Pick the generator** (skipped when you named it): `Command — A command of a service, with
   its test`.
2. **Answer its inputs**, in order. An input you gave as `name=value` is not asked. An enum
   (`app`, `service`) offers what the project holds now (a `choices` list offers its values), and an enum with one choice takes it
   without asking; an `ident` or `text` input is typed. An optional input offers `(skip)`, or an
   empty answer.
3. **Read the plan** in a float: each file as `+ path (N lines)` or `= path (exists, skipped)`,
   how many holes there are to fill, then each new file's text coloured as its
   language (the tree-sitter parser you have for it; a language with none stays plain), each
   placeholder underlined (`SorcererHolePlaceholder`). Nothing is written yet. `<CR>` applies; `q` or `<Esc>` cancels. If the
   plan stops on an input (say a `service` that is not there), you are asked that input once
   more with the choices.
4. **Apply.** The server writes the new files, never over one that is there, opens the first
   file at its first hole, and puts a **hole mark** on each hole's placeholder (just the
   placeholder, not the line): an Information diagnostic from source `spellbook`, code `hole`,
   saying `handle (block): its first case, from the test file beside it`. The diagnostics list
   is the hole list, and `]d` walks it.
5. **Fill the holes, like snippet placeholders.** You land on the first placeholder selected
   (select mode): type and it is replaced. `<Tab>` moves to the next hole of that file, `<S-Tab>`
   back, and the message line says where you are: `hole 1 of 3 · field: its first input · type
   to replace · <Tab> next · <leader>mc conjure` (the conjure part when conjure is on; the last
   hole says `<Tab> done`). A hole grows with what you type, so `<S-Tab>` selects your whole
   answer again, and a placeholder you have not reached stays code that parses. `<Tab>` on the
   last hole ends the session, and so does closing the buffer: `spellbook: holes done`, or
   `spellbook: holes done · 1 more in command__test.py, marked (]d)` when a hole sits in another
   file (that one you fill by its mark). The keys are that buffer's own, in insert and select
   mode only, for the session's length: normal-mode `<Tab>` and your global maps (a completion
   plugin's `<Tab>`) are never touched, and a buffer-local map you had on them is put back.
   `spellbook.hole_keys` sets them; `false` means no session, and then the end line reads
   `N holes, marked; ]d walks them`. The marks go when you save the file with the placeholder
   replaced (the server looks at the file on disk when you open or save it, not at each
   keystroke). With conjure installed and on, a hole's line offers *Conjure here* like any line
   in a function (`<leader>mc`), and conjure's change, mark, Undo and Try harder apply as they do
   anywhere. With `conjure_holes = true` the plugin runs conjure on the first hole for you and
   starts no session.
   Spellbook never calls a model itself.

An untrusted project (one whose own config sets a provider `runner` or `url`, or a grammar of its
own) gets no marks, and says `spellbook: this project is not trusted; run "spellbook trust" in it`
once a session when you open or save a file; every command answers the same sentence. The command line trusts the directory you run it in, as scry's does: `spellbook check` in a cloned repository builds that repository's own grammar first.
`:checkhealth sorcerer` has a spellbook section when it is enabled.

### Fixes in Neovim

`vim.lsp.buf.code_action()` on a finding's line lists `spellbook: Use a port the service has`
(preferred) and `spellbook: Add a port and use it`. `<leader>mf` runs the first one without the
menu, and `:Spellbook fix [input=value…]` is the command form (the `input=value` words answer inputs
the finding does not settle). A modified buffer is refused: `spellbook: save close_table.py first; a
fix is planned against the file on disk`. Otherwise it asks only what the finding leaves open (with
`vim.ui.select` or `vim.ui.input`), then shows the plan in the same float as `:Spellbook new`,
titled `spellbook: fix commands-use-ports · Use a port the service has · 2 files changed · app
bistro, unit close_table (from the finding)`. A changed file is `~ path (+3 −1)` and its hunks
follow, `+` lines in `DiffAdd` and `-` lines in `DiffDelete`; a new file is `+ path` with its text,
as in `new`. `<CR>` applies. The changed files land through the server's own `workspace/applyEdit`
(Neovim's default handler): one `u` in each buffer takes the fix back, and the buffers stay
modified until you save them (`:wa`); the new files are written, and the end line says
`spellbook: fixed commands-use-ports in close_table.py · changed 2 files · wrote 4 files`, with
`· 2 holes, marked; ]d walks them` after a fix that leaves holes (a hole session starts as after
`new`). Typing in the file since the plan applies nothing and says so.

## In VS Code

The **Spellbook** extension (`sorcererdev.spellbook`) does what the Neovim plugin does. Install it
from the Marketplace (`code --install-extension sorcererdev.spellbook`; forks get it from Open
VSX) and the program with `sorcerer-spellbook-installer` or `cargo install sorcerer-spellbook`.
The extension finds `spellbook` as scry's and conjure's find theirs (the `spellbook.path` user
setting, then `PATH`, `~/.cargo/bin`, `~/.local/bin`); with none it tells you that install line.
It starts one `spellbook lsp` per workspace folder; the server's log is the Output channel
**Spellbook**.

`ctrl+m n` (or **Spellbook: New** in the Command Palette) runs the same steps as `:Spellbook new`
in the folder of the file you are in (with no file open and several folders, it asks which):

1. **Pick the generator** in a quick pick, its summary beside it.
2. **Answer its inputs**, in order: an enum in a quick pick (one choice taken without asking, an
   optional one offers `(skip)`), an `ident` or `text` input in an input box. `Esc` anywhere
   cancels, and nothing is said. An input taken without asking is named in the plan's title, so
   you see what was picked for you: `· app bistro, service booking (the only choices)`.
3. **Read the plan.** Each new file opens as a read-only preview, coloured as its language by
   VS Code itself, and over it a quick pick titled `spellbook: command · 2 files · 3 holes to
   fill` holds **Apply**, **Cancel** and the file list (`+ path`, `31 lines`; `= path`, `exists,
   skipped`). Moving onto a file with the arrows shows it; you can click into the preview to
   scroll, and the pick stays. **Apply** writes; **Cancel** or `Esc` writes nothing. Either way
   the previews close. If the plan stops on an input, you are asked it once more.
4. **Apply.** The server writes the files, opens the first at its first hole and puts the hole
   marks in Problems (Information, source `spellbook`, code `hole`). When no hole session follows
   (step 5), the status bar says `spellbook: wrote 2 files · 3 holes, marked (Problems)`, or
   `· ctrl+m c to conjure the first` when the Conjure extension is installed: a hole's line offers **Conjure here** like any line in
   a function. With `spellbook.conjureHoles` on, the extension runs conjure on the first hole for
   you, and no hole session starts.
5. **Fill the holes, like snippet placeholders.** The opened file's holes become VS Code's own
   snippet placeholders: the first is selected, typing replaces it, `Tab` moves to the next hole
   and `Shift+Tab` back, `Esc` leaves. These are VS Code's snippet keys, so a binding you gave
   them is kept. A hole grows with what you type. The status bar says where you are: `hole 1 of
   3 · field: its first input · type to replace · Tab next · ctrl+m c conjure` (the conjure part
   when the Conjure extension is here; the last hole says `Tab done`). Moving out of every hole
   (`Tab` on the last, a click elsewhere) ends the session: `spellbook: holes done`, or
   `spellbook: holes done · 1 more in cancel_table__test.py (Problems)` when another file has
   holes (fill that one by its marks). The file shows as changed as soon as the session starts,
   although its text is what was written; save when the holes are filled, which is also when
   their marks go. `spellbook.holeSession` off gives the end line instead
   (`3 holes, marked (Problems)`).

The command also takes arguments, so a keybinding can skip questions:
`{ "key": "ctrl+m b", "command": "spellbook.new", "args": { "generator": "command", "inputs":
{ "service": "booking" } } }`.

`ctrl+m k` (or **Spellbook: Check**) runs `spellbook check --json` in the folder's root and puts
the findings in Problems, each on its line with VS Code's error, warning or hint squiggle (source
`spellbook`, the rule's id as its code); Problems opens when there are any, else the status bar
says `spellbook: conventions hold`. Each run replaces the last. The files you have open show their
findings through the server (on open and save), so Check leaves them out and a finding is listed
once. A check that could not run (exit 2) says why: `spellbook: check could not run: …`.

The looks: a hole's mark draws a `◇` in the gutter, a blue tick in the overview ruler, and its
placeholder tinted as VS Code tints a snippet's tab stop (the theme's
`editor.snippetTabstopHighlightBackground`); the plan preview's placeholders have the same tint. A
rule finding is VS Code's own squiggle in its severity's colour. A conjured change is the green
`✦` the Conjure extension draws.

Both chords work while the cursor is in an editor (`when: editorTextFocus`): in the integrated
terminal `ctrl+m` is Enter. On macOS `ctrl` is the Control key. On Windows and Linux `ctrl+m` is
VS Code's own "Toggle Tab Key Moves Focus", which it no longer does as the first key of a chord;
to keep it, or for other keys, rebind in Keyboard Shortcuts or `keybindings.json`, for example
`{ "key": "ctrl+alt+n", "command": "spellbook.new", "when": "editorTextFocus" }`.

An untrusted project says the server's sentence (`run "spellbook trust" in it`) as a warning;
run `spellbook trust` in a terminal in the project. `[spellbook] enabled = false` reads the same
way. `spellbook doctor` is the command line's; the extension has no doctor of its own.

| setting | default | what it does |
|---|---|---|
| `spellbook.path` | `""` | the `spellbook` binary, from your user settings only (not in Restricted Mode) |
| `spellbook.conjureHoles` | `false` | after **Spellbook: New**, run conjure on the first hole (needs the Conjure extension) |
| `spellbook.holeSession` | `true` | after **Spellbook: New**, the opened file's holes as snippet placeholders; off: only marked |

### Fixes in VS Code

On a finding's line the lightbulb (or `ctrl+.`) offers `spellbook: Use a port the service has`
(preferred) and `spellbook: Add a port and use it`; `ctrl+m f` (**Spellbook: Fix**) runs the
preferred one without the menu. A buffer with unsaved changes is refused:
`spellbook: save close_table.py first; a fix is planned against the file on disk`. Otherwise it
asks only what the finding does not settle (a quick pick, `Esc` cancels), then shows the plan under
a title such as `spellbook: fix commands-use-ports · Use a port the service has · 2 files changed
· 0 holes to fill · app bistro, unit close_table (from the finding)`. A changed file is `~ path
(+3 −1)`; moving onto it opens VS Code's diff view (the file on disk against the plan, read only);
a new file is `+ path` and opens as in New. **Apply** changes the open files in the editor (one
`ctrl+z` per file takes the fix back; they stay unsaved until you save) and writes the new ones;
the status bar says `spellbook: fixed commands-use-ports in close_table.py · changed 2 files`, and
`· wrote 4 files · 2 holes, marked` when the fix leaves holes (a hole session starts as after New).

## The files it writes

Besides the files you ask for by name, spellbook writes only its own state under
`.sorcerer/state/spellbook/`:

| file | what it holds |
|---|---|
| `generations.jsonl` | one line per generation applied: its id, time, generator, inputs, files and holes, and for a fix `"fix": { "rule", "file", "line", "fingerprint", "index" }` and `"changed": [{ "path", "added", "removed" }]` (each line has `"version": 1`) |
| `holes.json` | the open holes, `{"version": 1, "data": {"holes": [...]}}`, refreshed by `new` and `holes` |
| `report.json` | the findings of the last check, `{"version": 1, "data": {"checked": …, "findings": [...]}}`: rewritten whole by `spellbook check`, a file at a time by the editor's server |

## Settings

`[spellbook]` in `.sorcerer/config.toml` has two keys, `enabled` and `exclude`; see
[Configuration](configuration.md). With `enabled = false` the command line (everything but
`trust` and `doctor`) says `spellbook is off in this project ([spellbook] enabled = false)` and
exits 2, and the editor server shows no marks and answers commands with that sentence.

## What is coming

The rest of milestone 2 brings the editors' side of fixes (the fix command, a diff preview, the keys),
`missing` rules, owned regions (the
`pub mod` line a generator keeps), list inputs and `if`/`for` in templates, and packs of generators
and rules shared between projects.
Milestone 3 brings suggestions: a convention noticed in your code, written as a draft generator or
rule, first by counting and then, if you want it, with Jev. Nothing in spellbook costs anything
until then, and the model parts stay optional.
