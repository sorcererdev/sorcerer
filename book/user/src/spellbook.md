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
| rules that check the code | here: `.sorcerer/spellbook/rules.toml`, `spellbook check`, and warnings on save in the editor |
| owned regions (`pub mod` lines a generator keeps), insert edits, list inputs, `if`/`for` in templates | milestone 2 |
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
| `[inputs.X]` | `type`: `ident` (a name in any case style: `cancel_table`, `CancelTable` and `cancel-table` are the same name), `enum` (`one_of` a query) or `text`; `prompt`; `optional`; `default`. Inputs are asked in the order the file writes them |
| `[query.X]` | `glob`, a template over the inputs before the one that uses it, and `value`: `"parent folder"`, `"stem"` or `"path"` of each match. In the glob `*` stays inside one folder and `**` crosses folders; a glob may name folders as well as files |
| `[derive]` | names computed from the inputs, in order; each may use the ones before it |
| `[[edits]]` | `create` (the path, a template), then `template` (a file under `templates/`) or `text` (written as it is), and `on_exists`: `"error"` (the default: the plan stops, nothing is written) or `"skip"` |
| `[holes.X]` | `source = "derived"` and `value`: a hole filled from the inputs |
| `context` | olifant's key (files a filler reads); kept for a later milestone |

Keys that arrive in milestone 2 (`insert` and `at` edits, `[[uses]]`, `type = "list"`, `ast`
queries, `{= if =}`/`{= for =}`) are refused with a sentence that says so. A slip gets the
word to write instead: `content` → `text`, `pascalCase(x)` → `{= x|pascal =}`, `{{ }}` → `{= =}`.

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
`corpus/spellbook/`. `bistro` is Python (`command`, `query`, `port`); `bistro-rs` is Rust
(`command`, `port`). Take bistro's `command` end to end:

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
severity = "warn"
```

| key | meaning |
|---|---|
| `files` | a path pattern: `*` is part of one folder or file name, `**` any number of folders (`**.py` is `**/*.py`), `{name}` a name that must be there (it binds one folder, or the part of a name between the text around it). Required |
| `exclude` | a list of path patterns no finding is made in |
| `match` | the short form, below (write this or `query`) |
| `query` | a [tree-sitter query](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/index.html) in the file's language; every `@finding` capture is a finding. A query with no `@finding` is refused when the rule first runs |
| `message` | the sentence a finding says. Required |
| `severity` | `hint`, `warn` (the default) or `error` |
| `since`, `why` | free text for people: when the convention began and why it holds |

`missing`, `fix` and `fixes` are keys of milestone 2 (a rule that says what to add, and a
generator that repairs it): a rules file that has one says so and stops.

The short form is `{ kind, regex, not = { regex }, any = [ … ] }`. `kind` is the syntax node's kind
in the language's grammar (`import_statement`, `call_expression`); `regex` must match the node's
text (the Rust `regex` syntax), `not` must not, and `any` is a list of forms of which any one is a
finding. Spellbook writes it out as a query, so the two forms run on one engine. A line gets at most
one finding for each rule.

`spellbook check` runs every rule over every file whose language the project knows (or just the
files you name) and prints a row for each finding:

```
$ spellbook check
! src/bistro/booking/commands/close_table.py:2  commands-use-ports — Commands and queries talk to ports, not the database
conventions hold (1 finding, none blocking)
```

`!` is a warning, `·` a hint, `✗` an error. The exit code is 0 when the conventions hold (hints and
warnings do not fail), 1 when a finding is an error, and 2 when it could not run (the rules file has
a mistake, which the message names). `--json` prints `{ "checked", "findings": [ … ] }`: each finding
has `rule`, `file`, `line`, `severity`, `message` and a `fingerprint` (seven hex digits that stay the
same when the code is moved or re-indented). `[spellbook] exclude` in `config.toml` lists files no
rule sees.

To let one place break a rule, say why in a comment on the line or the line above:

```python
# spellbook:allow commands-use-ports — migration, tracked in issue 12
import sqlite3
```

The reason follows an em dash (` - ` and ` -- ` work too). An allow without a reason still silences
the finding, and leaves a warning, `allow-without-reason`, on the comment; an allow that names no rule
(a typo) silences nothing and is an `unknown-rule` warning.

In the editor, the server checks a file when you open it and when you save it, and shows each finding
as a diagnostic on its line (a hint, warning or error, source `spellbook`, the rule's id as its
code), beside the marks of the holes in the file. Edit `rules.toml` and the next save uses it. A
`rules.toml` that does not load is said once in the editor, and `spellbook doctor` shows the
problem.

The last check is kept in `.sorcerer/state/spellbook/report.json`.

## In Neovim

Install `spellbook` (no installer is published yet: `cargo install --git
https://github.com/sorcererdev/sorcerer sorcerer-spellbook`) and [sorcerer.nvim](https://github.com/sorcererdev/sorcerer.nvim),
and turn it on, which the plugin keeps off until you ask:

```lua
require("sorcerer").setup({ spellbook = { enabled = true } })
vim.keymap.set("n", "<leader>sn", "<cmd>Spellbook new<cr>")
```

| setting | default | what it does |
|---|---|---|
| `spellbook.enabled` | `false` | start `spellbook lsp` for the files you open, one server per project root |
| `spellbook.cmd` | `{ "spellbook", "lsp" }` | the program, when it is not on your PATH |
| `spellbook.conjure_holes` | `false` | after `:Spellbook new`, run conjure on the first hole (needs conjure on too) |

`:Spellbook new [generator] [input=value…]` takes five steps, with Neovim's own pickers
(`vim.ui.select`, `vim.ui.input`, so a picker plugin such as telescope or snacks shows them):

1. **Pick the generator** (skipped when you named it): `Command — A command of a service, with
   its test`.
2. **Answer its inputs**, in order. An input you gave as `name=value` is not asked. An enum
   (`app`, `service`) offers what the project holds now, and an enum with one choice takes it
   without asking; an `ident` or `text` input is typed. An optional input offers `(skip)`, or an
   empty answer.
3. **Read the plan** in a float: each file as `+ path (N lines)` or `= path (exists, skipped)`,
   how many holes there are to fill, then each new file's text with its hole lines lit
   (`DiagnosticInfo`). Nothing is written yet. `<CR>` applies; `q` or `<Esc>` cancels. If the
   plan stops on an input (say a `service` that is not there), you are asked that input once
   more with the choices.
4. **Apply.** The server writes the new files, never over one that is there, opens the first
   file at its first hole, and puts a **hole mark** on each hole's line: an Information
   diagnostic from source `spellbook`, code `hole`, saying `handle (block): its first case, from
   the test file beside it`. The diagnostics list is the hole list, and `]d` walks it.
5. **Fill the holes.** By hand, and the marks go when you save the file with the placeholder
   replaced (the server looks at the file on disk when you open or save it, not at each
   keystroke). With conjure installed and on, the end line reads `N holes · <leader>ca to
   conjure the first`: the hole's line already offers *Conjure here* like any line in a
   function, and conjure's change, mark, Undo and Try harder apply as they do anywhere. With
   `conjure_holes = true` the plugin runs conjure on the first hole for you, and nothing else.
   Spellbook never calls a model itself.

An untrusted project (one whose own config sets a provider `runner` or `url`, or a grammar of its
own) gets no marks, and says `spellbook: this project is not trusted; run "spellbook trust" in it`
once a session when you open or save a file; every command answers the same sentence. The command line trusts the directory you run it in, as scry's does: `spellbook check` in a cloned repository builds that repository's own grammar first. VS Code has no spellbook extension yet (milestone 2).
`:checkhealth sorcerer` has a spellbook section when it is enabled.

## The files it writes

Besides the files you ask for by name, spellbook writes only its own state under
`.sorcerer/state/spellbook/`:

| file | what it holds |
|---|---|
| `generations.jsonl` | one line per generation applied: its id, time, generator, inputs, files and holes (each line has `"version": 1`) |
| `holes.json` | the open holes, `{"version": 1, "data": {"holes": [...]}}`, refreshed by `new` and `holes` |
| `report.json` | the findings of the last check, `{"version": 1, "data": {"checked": …, "findings": [...]}}`: rewritten whole by `spellbook check`, a file at a time by the editor's server |

## Settings

`[spellbook]` in `.sorcerer/config.toml` has two keys, `enabled` and `exclude`; see
[Configuration](configuration.md). With `enabled = false` the command line (everything but
`trust` and `doctor`) says `spellbook is off in this project ([spellbook] enabled = false)` and
exits 2, and the editor server shows no marks and answers commands with that sentence.

## What is coming

Milestone 2 brings fixes (a rule that names the generator that repairs it), `missing` rules,
owned regions (the `pub mod` line a generator keeps), insert edits, list inputs and `if`/`for` in
templates, packs of generators and rules shared between projects, and a VS Code extension.
Milestone 3 brings suggestions: a convention noticed in your code, written as a draft generator or
rule, first by counting and then, if you want it, with Jev. Nothing in spellbook costs anything
until then, and the model parts stay optional.
