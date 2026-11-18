# The core

`crates/sorcerer-core`: one module a concern.

## Config

`config.rs`. One file, `.sorcerer/config.toml`, parsed once by `Config::parse`. Core owns
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

The environment is a seam (`env.rs`): `Env` answers variables and executables on PATH.
`SystemEnv` is the real one, `MapEnv` a fixed one for tests. `Cancel` (`cancel.rs`) is
the flag every long call checks.

## Project

`project.rs`. `Project::open(dir)` walks up from `dir`. The first directory holding
`.sorcerer/` is the root, and its `config.toml` is read if present. With no `.sorcerer/`
anywhere above, the git root is the project and the config is `Config::default()`: that is
the out-of-the-box case, and every product must work in it. With neither, the error says
so in one sentence.

`Project::state_dir()` creates `.sorcerer/state/` on first use and writes a `.gitignore`
of `*` into it, so nothing a tool remembers is ever committed.

`default_provider` and `default_decider` choose what runs a model when the config does not
say, and why, for `doctor` to print: `claude` when it is on PATH, else `anthropic` when
`ANTHROPIC_API_KEY` is set; Jev when `JEV_API_KEY` is set.

## Languages

`language.rs`. A language is layers of TOML merged in order, then query files:

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

`grammar.rs`, ported from olifant's `Language/Grammar.hs`. No grammar is compiled into a
binary. `grammar::load(cache, name, source)` returns a `tree_sitter::Language`:

1. `library_path` names the compiled library `<cache>/<name>-<key>.<dylib|so>`. The key is
   FNV-1a 64 (`fnv.rs`) over the source's identity (the git rev, or a local directory's
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
`load` remembers each library once per process, failures included.

A missing compiler is `Error::NoCompiler` with an install hint, never a silent fallback:
every reader of units would be wrong in a different way.

Tests never fetch: `tests/grammars/{python,rust}` hold the vendored sources at the
shipped pins, and `Languages::override_grammar` points a language at them.

## Units

`unit.rs` and `query.rs`. `Languages::load(name, cache)` gives a `Loaded`: the language,
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
