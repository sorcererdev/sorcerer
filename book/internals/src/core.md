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

## Trust

`domain/trust.rs` decides; `adapters/trust_list.rs` remembers. Three things a project's own
files can set run code or move the user's API key: a provider's `runner` (a program sorcerer
starts), a provider's `url` (where the code and the key are sent) and a grammar the
project's `.sorcerer/languages/<name>/language.toml` names (C source compiled with the C
compiler and loaded into the process). `needs_trust(project)` lists each one as a `TrustReason { key,
value }`, providers by name (runner, then url), then languages by name; a language counts
when `origins["grammar"] == "project"`, so a shipped grammar or a project file that only
changes `files` needs nothing. A git grammar needs trust with or without a pinned `rev`.

`TrustList` is the user's list of trusted project roots, `trusted.toml` in `sorcerer/` under
the user's config directory (`TrustList::user`, through the platform adapter's
`user_config_dir`: `$XDG_CONFIG_HOME`, else `~/.config`, on Unix; `%APPDATA%` on Windows;
`None` when the environment names none; `TrustList::at(dir)` for tests). It reads `projects = [ … ]`, sorted. Every
question reads the file afresh; `trust` and `revoke` write it whole beside itself and rename
it over. A missing file is an empty list; a broken one trusts nothing and refuses to be
written. The key is the root `Project::open` picks, which is canonical, so a symlink and its
target are one project. `Project::root_above(dir)` gives that root without reading any
config, for a server deciding whether a file belongs to its project.

Only a server that starts by itself (an editor's) consults the list. The command line never
asks: typing a command in a project is the consent.

## Languages

`domain/language.rs`. A language is layers of TOML merged in order, then query files:

1. **shipped**: `crates/sorcerer-core/library/languages/<name>/{language.toml, queries/*.scm}`, embedded in the
   binary with `include_dir!` (`SHIPPED`);
2. **project**: `.sorcerer/languages/<name>/`, either file optional.

`Languages::assemble(root, named_in_config)` reads every layer and `merge` combines them (`LaidTogether`, then `CoreKeys` reads each core key): a top-level
key in a later layer replaces the earlier one whole, and query files merge by name.
`Language::origins` records which layer set each key (`files`, `grammar`, …) and each query
file (`queries/tags.scm`), for `doctor`. A project-only language (no shipped layer) must
have `files` and `grammar`.

Core reads `files`, `grammar`, `comment`, `unit_words`, `unit_separator` and `[unit] name`
(the `CORE_KEYS`). Every other table stays in `Language::rest` for the product that owns it;
the product reads it with the same strictness as its config table.
The shipped languages carry conjure's `[test]` and `[imports]` tables there and an
`imports.scm` beside `tags.scm`; core compiles the query like any other, and it adds no unit
because it captures only `@import.path`.

`Languages::for_path(rel)` gives the first language, in name order, whose `files` globs
match a project-relative path.

## Grammars

`adapters/grammar.rs`. No grammar is compiled into a binary.
`grammar::load(cache, name, source, origin)` returns a `tree_sitter::Language` and a
`GrammarObtained` (`domain/language.rs`) that says how its library got into the cache:
`Cached`, `Downloaded`, or `Built { compiler }` (the compiler's name, such as `cc`).
`Languages::load` passes the language's `grammar_origin()`: `GrammarOrigin::Shipped` when the
`grammar` key still comes from the shipped layer, `Project` when the project's layer or
`override_grammar` set it. `Loaded.obtained` keeps the answer for doctor.

1. `library_path` names the compiled library `<cache>/<name>-<key>.<ext>`, the extension
   from the platform adapter's `shared_library_extension()` (`dylib`, `so`, `dll`). The key is
   FNV-1a 64 (`domain/fnv.rs`) over the source's identity (the git rev, or a local directory's
   path with `parser.c`'s and `scanner.c`'s sizes and times), the tree-sitter runtime's
   ABI, the OS and the arch. A changed key is a new file. A file already there is `Cached`.
2. **Downloaded** (`Grammar::put_in_cache`, `release_to_download_from`, `download_prebuilt`).
   `load` calls `load_with_compiler` with `c_compiler(&SystemEnv)`; a `Grammar` struct carries
   the cache, the name, the source and that compiler through the steps. A `Shipped` grammar
   with a `{ git, rev }` source asks the cache's release for the file of that exact name: the
   name already says the platform. `GrammarCache::user` sets `releases` to `RELEASES`,
   `https://github.com/sorcererdev/sorcerer/releases/download/v<CARGO_PKG_VERSION>`;
   `GrammarCache::at` sets none, so tests never download unless they call `with_releases`
   with a local `mockito` server. `download_prebuilt` makes two GETs on one `reqwest` blocking
   client (`release_client`: 10 s to connect, 60 s in all, at most 10 redirects, and a
   redirect policy that asks `follows_redirect(from, to)`: a hop from https to anything but
   https is refused, so a redirect cannot drop to http; the function is pure and unit-tested,
   because the mockito tests run on plain http): `<releases>/<file>`, refused past 20 MB
   (`LIBRARY_CAP`, read through `take`, so a lying `Content-Length` cannot grow it), and
   `<file>.sha256`, refused past 4 KB. `sum_matches` compares the library's SHA-256
   (`sha256_hex`) with the first word of the sum file. Only then does `put_in_place` write the
   bytes beside the library (`aside`) and rename them in. A process killed
   between the two leaves a `*.tmp-*` file; `load_with_compiler` calls
   `sweep_once_per_process`, which the first time a process uses a cache runs
   `remove_stale_temporary_files` over its directory: files with `.tmp-` in the name and older
   than a day (`STALE_AFTER`; a younger one may be another process's download) are removed,
   errors ignored. Every failure (no release, a 404,
   offline, a cut-off or oversized body, an HTML page, a bad sum) is a `DownloadFailed` with a
   few words of why (`request_failed_text`: `no release for 0.1.0` for a 404, `the download
   timed out`, `its SHA-256 did not match the release's`), and `put_in_cache` falls through
   to step 3 with it. A project's own grammar, an override and a local
   directory never download. `put_in_cache_and_open` then opens the library; a `Downloaded`
   one that will not open (built for a newer system, or wrong in the release) is deleted and
   built here instead (`build_here`).
3. **Built** (`build_here`, `build_if_absent`). A library another process put in place
   meanwhile is `Cached`. With no compiler after a failed download it stops before fetching
   anything, with `Error::NoGrammar`: `couldn't download the python grammar (no release for
   0.1.0); install a C compiler: xcode-select --install`. Otherwise it fetches a git source into
   `<cache>/src/<name>-<rev7>/` (`fetch`: `check_fetchable` allows only `https`, `ssh`, `git`
   and `file` URLs, since git's `ext::` transport would run a command, then
   `fetch_commit_into` runs `git fetch --depth 1` of the pinned rev) and compiles `parser.c`
   and `scanner.c` with the compiler it is given (`compile_where_the_compiler_can_read_paths`, then `compile_library`: when the sources' or the output's path is not ASCII, as for a Windows user named outside Windows-1252 whose MinGW `ld` would get `?` for each letter, it copies the sources to a folder from `ascii_scratch_parent` in `platform/paths.rs`, compiles there and copies the library into place). `build_here` asks the
   platform adapter for it: `c_compiler(env)` is the first of `cc`, `gcc` and `clang` on
   PATH, with `-shared -fPIC -O2` on Unix and `-shared -O2` on Windows.
4. `open_library` loads it with `libloading`, calls `tree_sitter_<name>`
   (`load_language_symbol`), checks the ABI
   against the runtime's range, and leaks the library handle so the language outlives
   every parser.

Three kinds of file reach `libloading`: one this machine compiled; one whose SHA-256 matched
the `.sha256` beside it on the release; and one already in the user's cache under its name,
which is trusted by name and not checked again (whatever can write the user's cache can put a
library there). The sum comes from the same server as the library, so it guards against a
broken or cut-off download, not against the release itself; trust in the release is trust in
GitHub's https. On Windows a downloaded DLL that opens but fails the ABI check stays mapped and
cannot be deleted, so its `GrammarAbi` error stands; the ABI is in the file's name, so only a
mis-built release can cause that.

The cache is the user's (`GrammarCache::user`: `sorcerer/grammars` under the adapter's
`user_cache_dir`, `~/.cache/sorcerer/grammars` on Linux and macOS,
`%LOCALAPPDATA%\sorcerer\grammars` on Windows), shared by
every project. When the environment names no user cache directory, `GrammarCache::dir` is
`Error::NoCacheDir`, `there is no place for the grammar cache: {NO_CACHE_DIR}` (`neither
XDG_CACHE_HOME nor HOME is set`; on Windows `LOCALAPPDATA is not set`), and every load fails
with it. There is no fallback under the current directory: `scry lsp` runs in the project, so
a repository could carry a library there. Every write goes to a temporary name and is renamed into place, so a
reader never sees half a library and two processes building at once race harmlessly.
`load` remembers each library once per process, failures included, except a failed fetch
or file error: being offline at first start must not cost units until a restart. A second
`load` of a library this process already opened says `Cached`. The memo's lock is held while
a library is put in the cache, so a first download (at most two requests of 60 s each) or a
compile holds up every other grammar load in the process until it is done.

A missing compiler (`build_if_absent` given `None`, and no library in the cache yet) is
`Error::NoCompiler`, `no C compiler (cc, gcc or clang) found; install one: {hint}`, with the
platform adapter's `compiler_hint()`: `xcode-select --install` on macOS, `apt install gcc`
on Linux, `install MinGW-w64 (gcc)` on Windows. Never a silent fallback:
every reader of units would be wrong in a different way. With a release for this version
and a network, a shipped language never gets that far.

The release's files come from `examples/build-grammars.rs`
(`cargo run -p sorcerer-core --example build-grammars -- <out dir>`): for each shipped
language it calls `build_if_absent` with `GrammarCache::at(<out dir>)`, so each file has the
name `library_path` will ask for, writes `<file>.sha256` (`<hex>  <file name>`) beside it, and
removes the fetched sources. The release job runs it on each platform's runner
([Releasing](releasing.md)).

Tests never fetch: `tests/grammars/{python,rust}` hold the vendored sources at the
shipped pins, and `Languages::override_grammar` points a language at them. The download
tests in `tests/grammar.rs` serve a library built from those sources, and a local
repository stands in for GitHub when they fall back to fetching.

## Units

`domain/unit.rs` and `domain/query.rs`. `Languages::load(name, cache)` gives a `Loaded`: the language,
its grammar, and its query files compiled by `query::compile`, which refuses any predicate
or directive the `tree-sitter` crate would silently ignore (only `#eq?`, `#match?`,
`#any-of?` and their `#not-` forms are evaluated).

`units_of(loaded, text)` parses the text (`Loaded::parse`, which `facts_of` uses too) and runs
every query file over it once (`Captures::of`, below; `Captured::from` reads the matches); then `one_per_node`, `widened_span` and
`named_units` apply the rules below, in that order. The rules:

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
5. **Its body** is what `textobjects.scm` captures as `@function.inner` or `@class.inner`
   inside it and no smaller definition holds (`Captures::body_of`): Python's block, Rust's
   items between the braces. A definition with nothing in its body (`fn f() {}`) has none.

A Rust impl is a unit named by its type (`queries/impls.scm`), so methods of a trait impl
are still `Type::method`. `unit_at(units, line)` gives the innermost unit holding a line. When the parser has lost a unit, `unit_or_broken_region_at`
(`domain/broken_region.rs`) falls back to the innermost ERROR node holding the line, as a `Unit`
of kind `BrokenRegion`; scry does not use it, conjure does (through `CodebaseView`).
Units are nested: a changed method also changes its class.

## Captures: the only way core looks at a tree

`domain/captures.rs`. The engine never names a grammar's node kinds or fields, and never a
language (`scripts/no-language-names` and the `no-*-in-the-engine` rules of
`.sorcerer/spellbook/rules.toml` refuse it). Everything it needs from a tree is a capture the
language's queries write. `Captures::of(loaded, tree, text)` runs every query file once and keeps
each match; `nodes(names)`, `is(name, node)`, `named(capture, text)` (with the match's `@name`)
read them, and three helpers answer the questions products ask:

| helper | answers | from |
|---|---|---|
| `body_of(definition)` | the definition's own body | `@function.inner`/`@class.inner` inside it that no smaller `@function.outer`/`@class.outer` holds |
| `body_items(definition)` | the body's items, comments out | the same, a block whose children are `@statement.outer` opened into them, `@comment.outer` dropped |
| `own(name, definition)` | a capture that belongs to the definition (its `@parameters`) | the same ownership rule |
| `list_items(list)` | a bracketed list's items | `@parameter.inner` whose parent is the list |

The table of capture names, and which query file writes each, is the module's doc comment;
`book/user` lists them for a person adding a language. A language without a capture gets
nothing from the feature that reads it (no comments, no anchors), never a guess.

## Fingerprints

`domain/fingerprint.rs`. `fingerprint(text, span)` names a unit's text with formatting removed:
seven hex digits. The span's lines are trimmed (which also
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
read from captures: comments are `@comment.outer`, a placeholder is what the language's
`placeholders.scm` captures as `@placeholder` (Python's `pass` and `...`, Rust's `todo!()` and
`unimplemented!()`), and the body is the unit's definition's `Captures::body_items`. No language
or node kind is named. Scry v1 does not use facts; conjure does.

`parse_errors_of(loaded, text)` is the same parse-error reading over the whole file: the lines
(from 1) of every error or missing node, each once, in order, and an empty list when the parser
gives up. Conjure checks a changed file with it ("parses", or "does not parse (line N)").

## Templates and name forms

`domain/templates.rs` owns the `{= … =}` template grammar the products share.
`pieces(template)` splits a template into `Piece::Text`, `Piece::Value { name, filters }`
(`{= name|pascal =}`, filters left to right) and `Piece::Hole { name, kind, instruction }`
(`{= ? handle: block | its first case =}`), refusing with a sentence a `{=` never closed,
`{{ }}` (another template language), `{= if =}`/`{= for =}` (milestone 2), an empty name and a
hole with no `: kind`. An error quotes the template whole when it is short (a path) and by its first
line and `…` when it is longer (`excerpt`), so a mistake in a file of template is not buried in the file.
`fill(template, values)` fills the values and applies the filters; a
hole is an error there ("a hole has no value here"), because only spellbook knows what goes in
one. Conjure fills its language files' path templates with it; spellbook renders generators
with `pieces`.

`domain/generated_regions.rs`: `REGION_OPENS` (`spellbook:region`, the words that open a region
spellbook keeps; spellbook's `region.rs` takes its `OPENS` from it) and `generated_regions_line(texts)`,
`GENERATED_REGIONS_LINE` when any text holds a region. Conjure's situation and lackey's
first message and `write_file` answer add the line, so a writer adds a module by creating its
file rather than typing a line into the region.

`domain/name_forms.rs`: `words_of(name)` splits a name written in any case style
(`cancel_table`, `CancelTable`, `cancel-table`, `cancelTable`, `Cancel table`) into the same
lowercase words, and `apply_filter(filter, value)` writes them again as `snake`, `pascal`,
`camel`, `kebab`, `upper`, `sentence` or `words` (`FILTERS`).

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
tree-sitter through `Languages::load` for units and facts. `FilesView::cache` gives its grammar
cache to a view laid over it (conjure's `EditorView` parses the editor's unsaved text with it).

To see units on a real file, run the demo from inside a project:

```text
cargo run -p sorcerer-core --example units -- src/orders.py
```

It prints each unit's lines, fingerprint, kind and name, then what changed against `HEAD`.

## Stores

`adapters/json_store.rs` (the data, `Versioned` and `Snapshot`, are `domain/versioned.rs`). A product's state is a `Store<T>`: one JSON file under
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

`domain/cost.rs`. `spent_since(errands, product, since, offset)` sums a product's spend from a
local day on; every cap (a pass's, a day's) reads it. An errand with no known cost counts zero, and a call
that timed out after starting is recorded with its cap as its cost, since it was probably
paid for.

## Fences

`domain/fences.rs` matches; `adapters/fences.rs` resolves. `Fences::for_project(project)` lists what a researcher may never read or send
to a model: `.env*` anywhere, `.sorcerer/` (other findings, the ledger) but its notes, `.git/`,
anything outside the root, and the user's `[model] not_evidence` globs. `allows(path)` resolves the
path first (symlinks and `..`, through the nearest existing ancestor for a file not yet
written) and then matches with `allows_relative`, ignoring case. A path that cannot be resolved for certain (a
`..` past a missing directory, a dangling symlink) is fenced.

**The notes carve-out.** `NOTES_DIR` (`.sorcerer/notes`) is the one place under `.sorcerer/`
that is not fenced. `Fences` keeps `.sorcerer/**` (`SORCERER_DIR_FENCE`) in a glob set of its
own, and `allows_relative` lets a path through it only when `is_in_notes` holds: the first two
names are exactly `.sorcerer` and `notes` and there is no `.` or `..` in it. The fences ignore
case to fence more; the exception is exact so it never unfences more (on macOS and Windows the
resolved path carries the on-disk spelling, so a note opened as `.Sorcerer/Notes/x.md` still
passes once resolved; on Linux `.sorcerer/Notes/` is another directory and stays fenced). It is judged
on the resolved path, so a link in the notes onto `.sorcerer/state/` (or `.sorcerer/notes` itself
a link) resolves to a fenced path and is fenced, and `fenced_links` names it, since `link_walk`
walks the notes. The other fences (`.env*`, `not_evidence`) still hold inside the notes.

The researcher's tools check every path they read, and every citation is checked before
it is shown. `patterns()` gives the list as configured (and tells one walk's fences from
another's); `described()` is what a prompt says, `.sorcerer/** (except .sorcerer/notes/)`; and
`deny_patterns()` is the list for `claude`'s deny rules, which take no exception, so
`.sorcerer/**` becomes `.sorcerer/<entry>` and `.sorcerer/<entry>/**` for each entry of
`.sorcerer/` but the one named exactly `notes`, read afresh for each `claude` started, plus
the ones sorcerer always keeps there (`KEPT_IN_SORCERER_DIR`: `state`, `config.toml`,
`spellbook`, `scry.toml`, `scry.md`, `languages`). The measurement below was made on one
`claude` version and must be made again on an upgrade. Measured with `claude` 2.1.331 on haiku: under such
rules a Grep of `.sorcerer` with a canary in `.sorcerer/state/` and `.sorcerer/config.toml`
printed only the note's line, and Read and Grep of `.sorcerer/state` were refused. An entry made
in `.sorcerer/` while a `claude` runs is not denied until the next start (no task writes there); a
`not_evidence` entry with no glob characters (`docs/plans`) fences the directory and
everything under it. `fenced_links()` lists the symbolic links in the project that lead out
of the root or onto a fenced path, for a researcher that cannot check each path itself.
