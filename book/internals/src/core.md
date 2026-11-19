# The core

`crates/sorcerer-core`: one module a concern, in three layers. `domain/` holds data and
rules and does no I/O; `ports/` holds the traits products talk through (`CodebaseView`,
`Ledger`, `Env`); `adapters/` implements them over the disk, git and the network, and holds
the I/O half of a domain type as an inherent `impl` (`Project::open` in
`adapters/project.rs`, `Fences::for_project` in `adapters/fences.rs`, `Languages::load` in
`adapters/grammar.rs`), so callers keep the method names.

## Config

`domain/config.rs`. One file, `.sorcerer/config.toml`, parsed once by `Config::parse`. Core owns
three tables and reads them strictly (an unknown key is an error naming it):

- `[model]` (`ModelConfig`): the provider to use, the user's `not_evidence` globs, and one
  `[model.providers.<name>]` table per provider (`ProviderConfig`: kind, url, key variable,
  runner, aliases, prices).
- `[decide]` (`DecideConfig`): the decider's model and key variable.
- `[languages]`: `names`, languages the project uses beyond the shipped ones.

Every other table stays in `Config::tables` for the product that owns it. A product reads
its table with `config.table::<ScryConfig>("scry")`; its struct carries
`#[serde(deny_unknown_fields)]`, so a typo inside `[scry]` is an `Error::UnknownKey`. A
top-level table no tool knows (`KNOWN_TABLES`) is probably a typo too, but another tool
might own it, so it is a warning in `Config::warnings`, not an error.

The environment is a seam (`ports/env.rs`): `Env` answers variables and executables on
PATH. `SystemEnv` is the real one, `MapEnv` a fixed one for tests (`adapters/env.rs`). `Cancel` (`cancel.rs`) is
the flag every long call checks.

## Project

`domain/project.rs` holds `Project`; `adapters/project.rs` opens one. `Project::open(dir)` walks up from `dir`. The first directory holding
`.sorcerer/` is the root, and its `config.toml` is read if present. With no `.sorcerer/`
anywhere above, the git root is the project and the config is `Config::default()`: that is
the out-of-the-box case, and every product must work in it. With neither, the error says
so in one sentence.

`Project::state_dir()` creates `.sorcerer/state/` on first use and writes a `.gitignore`
of `*` into it, so nothing a tool remembers is ever committed.

## Languages

`domain/language.rs`. A language is layers of TOML merged in order, then query files:

1. **shipped**: `library/languages/<name>/{language.toml, queries/*.scm}`, embedded in the
   binary with `include_dir!` (`SHIPPED`);
2. **project**: `.sorcerer/languages/<name>/`, either file optional.

`Languages::assemble(root, extra)` reads every layer and `merge` combines them: a top-level
key in a later layer replaces the earlier one whole, and query files merge by name.
`Language::origins` records which layer set each key (`files`, `grammar`, …) and each query
file (`queries/tags.scm`), for `doctor`. A project-only language (no shipped layer) must
have `files` and `grammar`.

Core reads `files`, `grammar`, `comment`, `unit_words`, `unit_separator` and `[unit] name`
(the `CORE_KEYS`). Every other table stays in `Language::rest` for the product that owns it;
the product reads it with the same strictness as its config table.

`Languages::for_path(rel)` gives the first language, in name order, whose `files` globs
match a project-relative path.

## Grammars

`adapters/grammar.rs`, ported from olifant's `Language/Grammar.hs`. No grammar is compiled into a
binary. `grammar::load(cache, name, source)` returns a `tree_sitter::Language`:

1. `library_path` names the compiled library `<cache>/<name>-<key>.<dylib|so>`. The key is
   FNV-1a 64 (`domain/fnv.rs`) over the source's identity (the git rev, or a local directory's
   path with `parser.c`'s and `scanner.c`'s sizes and times), the tree-sitter runtime's
   ABI, the OS and the arch. A changed key is a new file.
2. `build_if_absent` fetches a git source into `<cache>/src/<name>-<rev7>/` (`git fetch
   --depth 1` of the pinned rev; only `https`, `ssh`, `git` and `file` URLs, since git's
   `ext::` transport would run a command) and compiles `parser.c` and `scanner.c` with
   `cc -shared -fPIC -O2`.
3. `open_library` loads it with `libloading`, calls `tree_sitter_<name>`, checks the ABI
   against the runtime's range, and leaks the library handle so the language outlives
   every parser.

The cache is the user's (`GrammarCache::user`: `~/.cache/sorcerer/grammars`), shared by
every project. Every write goes to a temporary name and is renamed into place, so a
reader never sees half a library and two processes building at once race harmlessly.
`load` remembers each library once per process, failures included, except a failed fetch
or file error: being offline at first start must not cost units until a restart.

A missing compiler is `Error::NoCompiler` with an install hint, never a silent fallback:
every reader of units would be wrong in a different way.

Tests never fetch: `tests/grammars/{python,rust}` hold the vendored sources at the
shipped pins, and `Languages::override_grammar` points a language at them.

## Units

`domain/unit.rs` and `domain/query.rs`. `Languages::load(name, cache)` gives a `Loaded`: the language,
its grammar, and its query files compiled by `query::compile`, which refuses any predicate
or directive the `tree-sitter` crate would silently ignore (only `#eq?`, `#match?`,
`#any-of?` and their `#not-` forms are evaluated).

`units_of(loaded, text)` parses the text and runs every query file over it. The rules are
olifant's:

1. **A definition** is a capture named `definition.<kind>`, for a kind in `UNIT_KINDS`
   (function, method, class, interface, module, type, impl, instance), or `unit.<anything>`
   other than `unit.name`, in a match that also captures `@name`. Its bare name is the
   `@name` texts in source order. A node captured by two patterns is one definition.
2. **Its span** is widened to the earliest-starting `@function.outer` or `@class.outer`
   capture that contains it, ends where it ends, and holds no other definition except ones
   inside it. That is how a Python decorator joins its function.
3. **Its name** is qualified one level by containment: the bare name of the innermost
   other unit around it, then the language's `unit_separator` (`Order.cancel`,
   `Order::new`).
4. **Its kind** comes from the capture's suffix. A `function` inside a class, impl or type
   is a `Method`.

A Rust impl is a unit named by its type (`queries/impls.scm`), so methods of a trait impl
are still `Type::method`. `unit_at(units, line)` gives the innermost unit holding a line.
Units are nested: a changed method also changes its class.

## Fingerprints

`domain/fingerprint.rs`. `fingerprint(text, span)` names a unit's text with formatting removed:
seven hex digits, olifant's function exactly. The span's lines are trimmed (which also
drops a `\r`), blank lines are dropped, and `reflowed` joins lines a formatter split (after
an opening bracket, before a closing one, after a trailing comma). Then FNV-1a 64, first
seven hex digits.

So re-indenting, CRLF line endings, blank lines and a formatter's line splitting keep it;
any token changes it, and so does a comment. Every verdict sorcerer records is "true when
the unit hashed to this". `Fingerprint` serialises as its seven-character string.

`domain/compare.rs`: `compare_units(before, after)` pairs units by name and by which of that name
they are (the second `Order` impl with the second), and says `Same`, `Changed`, `Added` or
`Removed`. Scry's scope is the `Added` and `Changed` units of a file against `HEAD`.

## Facts

`domain/facts.rs`. `facts_of(loaded, text, unit)` tells what the code says about a unit without a
model: the lines of parse errors inside it, the lines of comments saying `TODO` or `FIXME`,
and whether its body is empty (absent, or only comments and placeholders). All of it is
decided by node kinds tree-sitter grammars share: a kind containing `comment`, the
placeholders in `EMPTY_BODY_KINDS` (`pass_statement`, `ellipsis`), and a `macro_invocation`
of `todo` or `unimplemented`. No language is named. Scry v1 does not use facts; conjure
does.

## The codebase view

`ports/view.rs`. Products and the researcher's tools never touch the file system directly: they
ask a `CodebaseView` for a file's `text`, its `text_at(rev)`, its `units` (now, or
`units_at(rev)`), a unit's `facts`, a `grep`, a `glob`, whether a path `exists`, or the files `changed_since(rev)`
(a rename is listed as both paths, `--no-renames`, so the old path reads as deleted). The trait exists because a
separate sorcerer tool will serve a codebase as a graph over an API; when a product needs
it, that service is one more implementation of this trait.

Two more questions are git's: `diff(path, rev)` is the file's change against REV in the
working tree (`git diff --no-color --no-ext-diff`, empty for an unchanged or untracked
file), and `merge_base(rev)` is the commit where HEAD and REV last agree, so a product can
review a branch against where it left its base. An unknown REV is an `Error::Git` naming it.

`FilesView` (`adapters/files_view.rs`) is the one implementation today: the working tree (walked with the `ignore`
crate, so `.gitignore` holds), git subprocesses for `text_at` and `changed_since`, and
tree-sitter through `Languages::load` for units and facts.

To see units on a real file, run the demo from inside a project:

```text
cargo run -p sorcerer-core --example units -- src/orders.py
```

It prints each unit's lines, fingerprint, kind and name, then what changed against `HEAD`.

## Stores

`adapters/json_store.rs`. A product's state is a `Store<T>`: one JSON file under
`.sorcerer/state/<product>/`, shaped `{"version": N, "data": …}`, where `T: Versioned`
names its `VERSION`.

- `read()` gives `Absent`, `Current(T)`, or `Unreadable(why)` (corrupt JSON, data that
  does not fit, or another version).
- `update(f)` takes an exclusive OS lock on `<file>.lock` (std's `File::lock`, released if
  the process dies), reads (absent is `T::default()`), applies `f`, writes to a temporary
  file, syncs it, and renames it over the store. So the editor's server and a command-line
  run can update one store at once and lose nothing.

Products open a store with `Store::in_project(project, "scry/findings.json")`, which
creates the state directory and its `.gitignore` first.

An unreadable store is never overwritten: `update` returns `Error::StoreUnreadable` and
the product pauses for the project, saying so in `doctor`. Starting over would bring back
every finding the user dismissed.

## The ledger and cost

Every paid call of every product is an `Errand` (`domain/errand.rs`) (when, product, verb, model,
provider, cost, tokens, seconds, whether it succeeded, a note) appended as one JSON line to
`.sorcerer/state/ledger.jsonl` through the `Ledger` port (`ports/ledger.rs`), whose
implementation is `JsonlLedger` (`adapters/jsonl_ledger.rs`). Each line is one write to a file opened
for appending, so two processes never interleave lines. `Ledger::read` skips a line that
does not parse and counts it.

`domain/cost.rs`. `spent_in(errands, product, since, offset)` sums a product's spend from a
local day on; every cap (a pass's, a day's) reads it. An errand with no known cost counts zero, and a call
that timed out after starting is recorded with its cap as its cost, since it was probably
paid for.

## Fences

`domain/fences.rs` matches; `adapters/fences.rs` resolves. `Fences::for_project(project)` lists what a researcher may never read or send
to a model: `.env*` anywhere, `.sorcerer/` (other findings, the ledger), `.git/`, anything
outside the root, and the user's `[model] not_evidence` globs. `allows(path)` resolves the
path first (symlinks and `..`, through the nearest existing ancestor for a file not yet
written) and then matches with `allows_relative`, ignoring case. A path that cannot be resolved for certain (a
`..` past a missing directory, a dangling symlink) is fenced.

The researcher's tools check every path they read, and every citation is checked before
it is shown. `patterns()` gives the list for prompts and for `claude`'s deny rules; a
`not_evidence` entry with no glob characters (`docs/plans`) fences the directory and
everything under it. `fenced_links()` lists the symbolic links in the project that lead out
of the root or onto a fenced path, for a researcher that cannot check each path itself.
