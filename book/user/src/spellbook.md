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
| generators with holes | here: `spellbook new`, `spellbook holes` |
| rules that check the code | coming next in this milestone (`spellbook check`) |
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
  text: no quotes, backslashes or new lines, since the placeholder puts it inside a string.

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
lines of it. Fill it and it is gone. `spellbook holes FILE…` looks only at those files;
`--json` prints the rows.

## The other commands

```
spellbook list                     the generators, a row each (a broken one is a row with its problem)
spellbook show <generator>         its inputs and their choices now, the files it creates, its holes, its examples
spellbook doctor                   the config, the generators, the languages and their hole kinds,
                                   whether conjure is installed, trust, holes.json, generations.jsonl
spellbook trust [--revoke]         let the editor's server run here although the project's config runs code
```

`spellbook doctor` says "holes can be conjured in the editor" when `conjure` is on the PATH, and
"holes are filled by hand; install conjure to fill them with a model" when it is not.

## The files it writes

Besides the files you ask for by name, spellbook writes only its own state under
`.sorcerer/state/spellbook/`:

| file | what it holds |
|---|---|
| `generations.jsonl` | one line per generation applied: its id, time, generator, inputs, files and holes (each line has `"version": 1`) |
| `holes.json` | the open holes, `{"version": 1, "data": {"holes": [...]}}`, refreshed by `new` and `holes` |

## Settings

`[spellbook]` in `.sorcerer/config.toml` has two keys, `enabled` and `exclude`; see
[Configuration](configuration.md).
