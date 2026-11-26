# Adding a language

Sorcerer ships Python and Rust. Any language with a tree-sitter grammar can be added with
two files and no code: the engine knows no language, it reads them.

Say your project has Go. Write `.sorcerer/languages/go/language.toml`:

```toml
# Which files are Go.
files = ["**/*.go"]
# The grammar: fetched at this commit, compiled once into your cache, and loaded when a
# tool needs it. You need a C compiler (see below).
grammar = { git = "https://github.com/tree-sitter/tree-sitter-go", rev = "<a commit>" }
comment = "//"
```

Then copy the grammar's own `queries/tags.scm` into `.sorcerer/languages/go/queries/`.
That is enough: tools find the *units* of a Go file (its functions, methods and types),
which is what scry reads and reports on.

The grammars of the languages sorcerer ships come prebuilt: the first time scry needs one, it
downloads the file for your platform from the release of your scry's version on GitHub,
checks its SHA-256 against the release's checksum, and keeps it in your cache. If that cannot
work (offline, behind a proxy, or a scry built from source whose version has no release), it
compiles the grammar instead, as below. With no C compiler either, scry says why the download
failed and what to install:
`couldn't download the python grammar (no release for 0.1.0); install a C compiler: xcode-select --install`.

A grammar of your project's own is always compiled on your machine, so you need a C compiler
on PATH: `cc`, `gcc` or `clang`, whichever is found first. Without one, `scry doctor` and the
first review say so and how to get one:

| platform | install |
|---|---|
| macOS | `xcode-select --install` |
| Linux | `apt install gcc` (or your distribution's `gcc`) |
| Windows | install MinGW-w64 (gcc) |

A user name or folder with letters outside Windows-1252 (such as `日本`) is fine: the grammar is built in a plain-ASCII temporary folder and moved into your cache.

The downloaded or compiled grammar is kept in your cache: `~/.cache/sorcerer/grammars` on Linux and macOS
(`$XDG_CACHE_HOME/sorcerer/grammars` when that is set), `%LOCALAPPDATA%\sorcerer\grammars` on
Windows. With none of those variables set, scry has nowhere to keep grammars and says which
to set (`there is no place for the grammar cache: neither XDG_CACHE_HOME nor HOME is set`);
it never keeps them under the project.

## What a language file holds

| key | meaning |
|---|---|
| `files` | globs over paths in the project: which files are this language's |
| `grammar` | `{ git = "…", rev = "…" }`, or `{ path = "…" }` for a grammar on disk (relative to the language file's directory) |
| `comment` | the line-comment marker: allow comments and owned regions' markers are written with it |
| `unit_words` | how a prompt describes a unit of this language |
| `unit_separator` | joins a method to its type in a unit's name; `.` unless set (Rust ships `::`) |
| `[unit] name` | patterns that read a unit's name from its first line, when the queries do not |

Two tables are read by [conjure](conjure.md): `[test]` and `[imports]`, and one by
[spellbook](spellbook.md): `[holes]` (below). Other tables belong to individual tools and are
described in their chapters.

## What conjure reads from a language

Conjure needs two facts about a language, and takes both from the language file:

```toml
[test]
# Where a file's tests live. `{= dir =}` is the file's folder and `{= stem =}` its name without
# the extension. Python ships "{= dir =}/{= stem =}__test.py"; Rust's tests are in the file
# itself, so it ships "{= dir =}/{= stem =}.rs". Leave it out and conjure says no test file is
# known for the language.
tests_for = "{= dir =}/{= stem =}_test.go"

[imports]
# Where an import's file may be: `{= path =}` is the import's words joined by `/`, so
# `bistro.booking.models` is `bistro/booking/models`. The first template that names a file that
# exists wins; the import is then tried once more without its last word, which may be a name
# inside a module rather than a module. Python ships four (`{= path =}.py`, `{= path =}/__init__.py`
# and the same under `src/`).
resolve = ["{= path =}.go"]
# Leading words that name no folder: Rust's `use crate::a::b` is `src/a/b.rs`.
drop_first = []
```

Imports are found by a query file, `queries/imports.scm`, that captures each imported path as
`@import.path` and nothing else (no `@name`, no `@definition.*`, so it adds no unit). Python's:

```scheme
(import_from_statement module_name: (dotted_name) @import.path)
```

A key that is not in the tables is an error naming the language and the table. A language with
no `imports.scm` has no related files, and conjure still works.

## What spellbook reads from a language

Spellbook writes a placeholder where a generator leaves a hole, and takes the placeholder of
each kind from the language file:

```toml
[holes]
# One template per kind, filled with the hole's {= name =} and {= instruction =}. Python ships
# these four; Rust ships ident = "todo_{= name =}", type = "()", todo!("hole …") for expr and
# block, and stmt = "todo!(\"hole …\");" for a statement among others.
ident = "todo_{= name =}"
type  = "object"
expr  = "None  # hole {= name =}: {= instruction =}"
block = "raise NotImplementedError(\"hole {= name =}: {= instruction =}\")"
```

Every placeholder must parse where its kind belongs (`ident` and `type` in a declaration,
`block` as a body, `expr` as a value, `stmt` as one statement among others: what a fix's
`replace` text writes where a statement was), because spellbook parses every file it plans before it
writes it. `expr` and `block` should carry `hole {= name =}:` in the code: that is how
`spellbook holes` finds them again, anywhere in the file. A generator that uses a kind the
language does not have is refused with the kinds it has; a value that is not text is an error
naming the language and the kind.

A project's own `[holes]` (in `.sorcerer/languages/<name>/language.toml`) replaces the shipped
table whole, as any table does, so it restates every kind its generators use.

### What anchors and fixes read

An `insert` edit's phrase (`end of parameters in CloseTable.__init__`) and a fix find their place
through the language's query captures, never by the language's name or its grammar's node names.
A language gets each target by having the captures below; one it lacks makes that target say
what is missing instead of guessing.

| capture | query file | what it marks | used by |
|---|---|---|---|
| `@import.path` | `imports.scm` | what an import names | `imports` |
| `@definition.function`, `@definition.method`, `@definition.class` (and any `@definition.<kind>` around them), with `@name` | `tags.scm` | definitions by name | `function NAME`, `class NAME`, `Class.method` |
| `@reference.call` with `@name` | `tags.scm` (Rust adds `calls.scm`) | calls; a struct literal counts as a call of its type | `call NAME` |
| `@function.inner`, `@class.inner`, `@statement.outer` | `textobjects.scm` | a body, and a block's statements | `start of`/`end of` a function or class |
| `@parameter.inner` | `textobjects.scm` | one item of a list | `parameters in`, `call` |
| `@comment.outer` | `textobjects.scm` | a comment (left out of bodies and lists; allow comments are read from it) | all |
| `@parameters`, `@arguments` | `lists.scm` | a definition's parameter list, a call's argument list | `parameters in`, `call` |
| `@docstring` | `docstrings.scm` | a string documenting the file (Python) | `imports` in a file without any |
| `@placeholder` | `placeholders.scm` | a statement that does nothing yet (`pass`, `todo!()`) | core's facts: an empty body |

And one key, for what the code cannot show:

```toml
# The separator a list of one item, or of none, takes when an item goes in (a longer list shows
# its own, between its last two items).
[syntax]
list_separator = ","
```

The shipped `lists.scm`, `docstrings.scm`, `placeholders.scm` and Rust's `calls.scm` are
sorcerer's own and short; start from them for a new language.

## Changing a shipped language

The same directory changes a shipped language. A key you set replaces the shipped key
whole; a query file with the same name as a shipped one replaces it, and a new name adds
one. For example, to make `.pyi` stubs Python too:

```toml
# .sorcerer/languages/python/language.toml
files = ["**/*.py", "**/*.pyi"]
```

## Queries

Queries are tree-sitter's: units come from `@definition.*` captures with their `@name`,
widened by the `@function.outer` and `@class.outer` captures of a `textobjects.scm` when
the language has one. Predicates `#eq?`, `#not-eq?`, `#match?`, `#not-match?`, `#any-of?`
and `#not-any-of?` are supported; a pattern using any other predicate or directive is an
error naming it, so remove such patterns from a copied file. `imports.scm` is compiled the same
way and read by conjure (related files) and spellbook (`end of imports`); the other captures
spellbook reads are in [What anchors and fixes read](#what-anchors-and-fixes-read).

Sorcerer's code reaches every language this way, through the captures and the language file,
and never names one, a grammar's node kinds or fields, or a tool (formatter, linter, test runner,
package manager). Sorcerer's own `scripts/no-language-names` and the `no-*-in-the-engine` rules
in its `.sorcerer/spellbook/rules.toml` check that on every change, so a language you add needs
no code.
