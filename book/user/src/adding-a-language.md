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

A grammar of your project's own is compiled on your machine, so you need a C compiler on
PATH: `cc`, `gcc` or `clang`, whichever is found first. Without one, `scry doctor` and the
first review say so and how to get one:

| platform | install |
|---|---|
| macOS | `xcode-select --install` |
| Linux | `apt install gcc` (or your distribution's `gcc`) |
| Windows | install MinGW-w64 (gcc) or LLVM (clang) |

The compiled grammar is kept in your cache: `~/.cache/sorcerer/grammars` on Linux and macOS
(`$XDG_CACHE_HOME/sorcerer/grammars` when that is set), `%LOCALAPPDATA%\sorcerer\grammars` on
Windows.

## What a language file holds

| key | meaning |
|---|---|
| `files` | globs over paths in the project: which files are this language's |
| `grammar` | `{ git = "…", rev = "…" }`, or `{ path = "…" }` for a grammar on disk (relative to the language file's directory) |
| `comment` | the line-comment marker |
| `unit_words` | how a prompt describes a unit of this language |
| `unit_separator` | joins a method to its type in a unit's name; `.` unless set (Rust ships `::`) |
| `[unit] name` | patterns that read a unit's name from its first line, when the queries do not |

Other tables (`[test]`, `[check]` and so on) belong to individual tools and are described
in their chapters.

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
error naming it, so remove such patterns from a copied file.
