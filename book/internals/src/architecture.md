# Architecture

Every crate is laid out the same way, in layers. A layer is a directory under the crate's
`src/`, and a dependency only ever points inward:

| layer | holds | may use |
|---|---|---|
| `domain/` | data and the rules over it; no I/O (no files, processes, environment, network or clock) | nothing else of the crate |
| `ports/` | the traits the crate talks through, and the data they pass | `domain` |
| `usecases/` | the service layer, the API: one module a use case, its steps private | `domain`, `ports` |
| `adapters/` | the ports implemented over the disk, git, a model, the clock; the wiring | anything but `cli` |
| `cli/` | an inbound adapter: parse the arguments, call one use case, print | anything |

**The vocabulary.** `usecases/` is the service layer. Each use case is a **command** (it changes
state and returns its outcome) or a **query** (it only reads and returns data). Nothing sits
above the use cases: **inbound adapters** call them. The cli, a web controller or the editor's
server are inbound adapters, and each cli subcommand is one small piece of the cli. So "command"
never names a cli subcommand or a layer.

A crate root (`lib.rs`, `main.rs`, `error.rs`, `cancel.rs`) sits outside the layers. Across
crates the same rule holds: a product's `domain/`, `ports/` and `usecases/` use another
crate's domain and ports (core's `Snapshot`, `CodebaseView`, `Ledger`; models' `Decider`), never
its `adapters/`. What needs a real adapter (scry's `doctor`, which probes the provider, Jev's
key and the grammars) lives in `cli/` or `adapters/`.

A domain type whose construction needs the disk keeps that half as an inherent `impl` in an
adapter file: `struct Project` is `domain/project.rs`, `Project::open` is
`adapters/project.rs`. Rust allows an `impl` block anywhere in the crate, so callers keep the
method names and the domain file stays pure.

## The crates

**sorcerer-core** has no feature of its own, so no `usecases/` or `cli/`. The languages it
ships are data in `crates/sorcerer-core/library/languages/<name>/`, embedded with
`include_dir!`; they live inside the crate so the published crate carries them.

- `domain/`: `config.rs` (the parsed `.sorcerer/config.toml`), `project.rs` (`Project`),
  `language.rs` (the merged language layers, `Languages`), `query.rs` (compiled queries),
  `unit.rs` (units from the queries), `fingerprint.rs`, `compare.rs` (units before and
  after), `facts.rs`, `versioned.rs` (`Versioned`, `Snapshot`: a product's state file as data),
  `errand.rs` (one paid call), `cost.rs` (`spent_since`), `fences.rs` (the
  patterns and `allows_relative`), `trust.rs` (`needs_trust`), `fnv.rs`, `paths.rs`
  (`forward_slashes`), `templates.rs` (the `{= … =}` grammar: `pieces`, `fill`) and
  `name_forms.rs` (`words_of`, `apply_filter`: the filters a template applies).
- `ports/`: `view.rs` (`CodebaseView`), `ledger.rs` (`Ledger`), `env.rs` (`Env`).
- `adapters/`: `project.rs` (`Project::open`, `state_dir`), `grammar.rs` (grammars downloaded
  prebuilt or fetched and built, then loaded; `Languages::assemble`, `Languages::load`), `files_view.rs` (`FilesView`),
  `json_store.rs` (`Store`), `jsonl_ledger.rs` (`JsonlLedger`),
  `fences.rs` (`Fences::for_project`, `allows`, `relative`, `fenced_links`), `env.rs`
  (`SystemEnv`, `MapEnv`), `trust_list.rs` (`TrustList`), and `platform/`, the platform
  adapter (below).

**sorcerer-models**

- `domain/`: `model.rs` (a model call as data, `price`, `errand`), `research.rs` (`Tool`,
  `Research`, `Researched`), `decide.rs` (`Question`, `Answer`, `Weighed`), `claim.rs` (a
  claim, its citations, the verdict, the cascade's constants), `claim_prompt.rs` (the
  checker's prompt, its parser, the guard questions).
- `ports/`: `provider.rs` (`Provider`), `researcher.rs` (`Researcher`), `decider.rs`
  (`Decider`).
- `usecases/`: `check_claim.rs` (the claim checker's cascade).
- `adapters/`: `anthropic.rs`, `claude.rs`, `openai.rs`, `http.rs`, `providers.rs`,
  `claude_code.rs`, `tool_loop.rs`, `tools.rs`, `jev.rs`, and the doubles
  `scripted_provider.rs`, `recorded_researcher.rs`, `recorded_decider.rs`.

**scry** (the package `sorcerer-scry`, whose library and program are both `scry`)

- `usecases/`: `review_change/` (`run_review.rs` with `review_change`, `request_and_report.rs` with its
  request, ports and report, `review_pass.rs` with the `Pass`; the steps `scope.rs`, `ask.rs`,
  `judge.rs`, `remember.rs`, `verify.rs`),
  `list_findings.rs`, `explain_finding.rs`, `dismiss_finding.rs`, `report_cost.rs`.
- `domain/`: `config.rs`, `issue_type.rs`, `finding.rs`, `findings.rs`, `steering.rs`,
  `scope.rs`, `review_prompt.rs`, `judging.rs`, `budget.rs`, `verification.rs`,
  `log_text.rs`.
- `ports/`: `reviewer.rs`, `claim_checker.rs`, `finding_store.rs`, `settings.rs`, `clock.rs`,
  `run_log.rs`.
- `adapters/`: `json_finding_store.rs`, `scry_toml.rs`, `research_reviewer.rs`,
  `markdown_run_log.rs`, `system_clock.rs`, `wiring.rs`.
- `cli/`: `arguments.rs` (the clap types), `run_cli.rs` (`main`, `main_with`), `console.rs`, one file a cli subcommand (`review.rs`,
  `list.rs`, `why.rs`, `dismiss.rs`, `cost.rs`, `doctor.rs`, `log.rs`), `format.rs`.

[Scry](scry.md) says what each file holds.

**sorcerer-conjure** (library and program `conjure`) has the same five layers:
`usecases/` (`conjure_change/`, `report_cost.rs`, `show_log.rs`), `domain/` (`config`,
`language_tables`, `change_prompt`, `situation`, `scry_notes`, `rules`, `spellbook_findings`,
`reply`, `edits`, `record`, `budget`, `log_text`), `ports/` (`writer`, `change_store`, `clock`,
`project_context`, `rule_checker`), `adapters/` (`provider_writer`, `research_writer`, `imports`,
`scry_files`, `steering_files`, `spellbook_checker`, `project_context`, `jsonl_change_store`, `system_clock`, `wiring`) and `cli/`.
Its path templates are filled with core's `templates::fill`.
[Conjure](conjure.md) says what each file holds.

**sorcerer-spellbook** (library and program `spellbook`) has the same five layers and depends on
`sorcerer-core` and `sorcerer-lsp` only (no models: it never spends). `domain/` (`config`,
`generator`, `generator_checks`, `generator_problems`, `holes_table`, `inputs`, `template`,
`holes`, `plan`, `record`, `open_hole_places`, `allow`, `finding`, `path_pattern`, `report`,
`rule`, `rule_query`), `ports/` (`generators`, `grammars`, `workspace`, `generation_store`,
`holes_store`, `clock`, `rule_runner`, `report_store`), `usecases/` (`list_generators`,
`choices_for`, `plan_generation`, `apply_plan`, `open_holes`, `check_files`,
`check_project`, `check_texts`), `adapters/` (`generator_dir`, `cached_grammars`, `disk_workspace`,
`jsonl_generation_store`, `json_holes`, `json_report`, `rules_toml`, `query_rules`, `checking`,
`system_clock`, `wiring`, and `lsp/`, the editor server) and `cli/`. Its use cases may not load a grammar (that is core's adapter work), so
they parse through the `Grammars` port. [Spellbook](spellbook.md) says what each file holds.

**sorcerer-apprentice** (library and program `apprentice`; a prototype, not published) has the
same five layers and depends on core, models and `sorcerer-lsp`. `domain/` (`config`, `task`,
`agent_event`, `status`, `mediums`, `loopback_message`, `edits_text`, `permissions`, `paths`,
`budget`, `estimate`, `log_text`, `prompt`, `region`, `cutting`, `triage`, `region_record`,
`answer_prompt`, `scry_notes`, `rules`), `ports/` (`agent`, `editor`, `task_store`, `journal`,
`permission_store`, `project_context`, `syntax`, `region_store`, `clock`), `usecases/`
(`run_task/`, `triage_regions`, `answer_regions`, `label_region`, `offer_task`, `end_task`,
`report_cost`, `show_log`), `adapters/` (`claude_agent`, `loopback`, `helper_way`,
`mcp_server`, `hooks`, `task_runner`, the stores, `project_context`, `grammar_syntax`,
`disk_editor`, `wiring`, and `lsp/`, the editor server) and `cli/`. [Apprentice](apprentice.md) says what each file holds.

**sorcerer-lsp** is a flat crate: `src/{lib,serve,client,jobs,uri}.rs`, with no layer
directories. All of it is adapter code (a connection, threads, the protocol's types), and
nothing in it is a product's domain, so splitting it into layers would leave every layer but
one empty; `scripts/layers` checks only layer directories and passes it as it is. A product's
server lives in that product's `adapters/` and calls its use cases.
[The language-server host](lsp.md) says what it owns.

## Adding a feature

1. Write the use case: a module under `usecases/` taking a request and the ports it needs,
   returning data (never printing). Its rules go in `domain/` as functions over data.
2. A use case that needs something from outside (a file, a model, the clock) asks a port. Use
   one that exists, or add a trait under `ports/` that says what the use case needs in its own
   words.
3. Implement the port in `adapters/`, and build it in the wiring.
4. Add the cli subcommand: a file under `cli/` that parses, calls the use case and prints.

An editor's language server is one more inbound adapter beside `cli/`: it calls the same use cases.

**A `mod.rs` holds only declarations.** Its lines are `mod`, `pub mod`, `use` and `pub use`
(plus comments and attributes): no function, type, constant, impl or test. What a directory does
lives in files named for the job (`review_change/run_review.rs`, `adapters/lsp/publisher.rs`), so
the tree and fuzzy-find show where the work is, and a `mod.rs` re-exports (`pub use`) what the
rest of the crate reaches by the directory's path. That includes `tests/support/mod.rs`, which
re-exports the fixture files beside it. `scripts/mod-rs-declarations` checks the rule on every
`mod.rs` (the eval fixtures under `corpus/` are skipped) and `scripts/mod-rs-declarations-test`
checks the check.

## Testing each layer

- **Domain**: plain unit tests, no doubles (`#[cfg(test)]` in the file, or `tests/`).
- **A use case**: with the doubles in `tests/support/` (`StubReviewer`, `StubDecider`,
  `FixedClock`) and closures for the checker and the run log (`ClaimChecker` and `RunLog` are
  implemented for closures); the store, ledger and settings over a temporary project
  (`Ports::of`).
- **An adapter**: against the real thing it wraps, a temporary directory, a git repository,
  a scripted provider; never a network or a model.
- **The command line**: `main_with` with a `Wiring` of doubles, asserting on stdout, stderr and
  the exit code.

## The platform adapter

Everything that differs between Windows, macOS and Linux is in one directory,
`crates/sorcerer-core/src/adapters/platform/`, the only code allowed to ask which operating
system it runs on. The models crate and scry reach it as `sorcerer_core::adapters::platform`
from their adapters and cli (and from tests). Each file replaces what used to be written per
platform where it was needed:

- `paths.rs`: `canonical(path)` (`dunce::canonicalize`: `C:\…` on Windows, never `\\?\C:\…`,
  which a compiler or git does not accept), used by `Project::open`, `Fences`, the grammar
  key and scry's server; `link_to(target, link)` makes a symlink for tests (`symlink_dir` or
  `symlink_file` on Windows).
- `dirs.rs`: `user_cache_dir(env)` (`$XDG_CACHE_HOME`, else `$HOME/.cache`; `%LOCALAPPDATA%`
  on Windows) and `user_config_dir(env)` (`$XDG_CONFIG_HOME`, else `$HOME/.config`;
  `%APPDATA%`), which `GrammarCache::user` and `TrustList::user` call instead of reading
  `HOME`; `NO_CONFIG_DIR`, the words for "there is none"; `user_dir_vars(home)`, the variables
  that put every user directory under one folder, for tests (`MapEnv::with_user_dirs(home)`
  is built on it).
- `compiler.rs`: `c_compiler(env)`, the first of `cc`, `gcc`, `clang` on PATH as a
  `CCompiler { program, flags }`; `shared_library_extension()` (`dylib`, `so`, `dll`);
  `compiler_hint()`, what to install on this platform.
- `local_connect.rs`: `connect_local(port, within)` connects to `127.0.0.1:port`. A port
  nothing listens on is refused at once everywhere: Windows by default retries a refused
  connect for about two seconds, so there the socket is made with `socket2` and told no SYN
  retransmissions (`WSAIoctl` with `SIO_TCP_INITIAL_RTO`). Apprentice's helpers reach their
  server with it (`ask_loopback`), and read a refusal as the server having ended.
- `process.rs`: `process::start(command)` starts a program in a group of its own (a process
  group on Unix, a job object on Windows, through the `process-wrap` crate) and returns a
  `Running`; `Running::stop()` ends the program and everything it started, then waits. A
  program already seen to end (by `try_wait` or an earlier `stop`) is left alone, so a stop
  never signals a reused process group id nor waits again on a finished job.
  `is_running(pid)` is for tests.

A project-relative path that becomes text (a prompt, the store, a run log, a research tool's
answer, a recording key, a `claude` deny rule, and what `scry list`, `why` and `review` print)
goes through `forward_slashes(path)` (`sorcerer_core::domain::paths`), which writes it with
`/` on every platform, so the same change gives the same text on Windows; on Linux and macOS
it is the path's own text. Paths enter `/`-separated where they are made: git names files
that way, and scry's server turns an editor's path into the project-relative one with `/`
(`project_file` in `adapters/lsp/publisher.rs`), so the editor route and the terminal route store
the same text. An absolute path (a root, the cache, the trust list, a file to open) keeps the
platform's form.

Finding a program on PATH needs nothing of its own: `SystemEnv::which` uses the `which`
crate, which tries `PATHEXT`'s extensions on Windows, so `claude` finds `claude.cmd`.

`scripts/platform` keeps it so (beside `scripts/layers`, below): in every `.rs` file under
`crates/` (sources, tests, examples and test modules alike) outside that directory it
refuses `cfg(unix)`, `cfg(windows)`, `cfg(target_os …)` (also inside `not(…)` or `any(…)`),
the same in `cfg!(…)` and `cfg_attr(…)`, `std::os::`, `os::unix` and `os::windows` (as in
`use std::{fs, os::unix::…}`), `.canonicalize(`, `"HOME"`, `process_group` and
`Command::new("kill"`, skipping comment lines. Each refusal is printed as
`file:line: platform code outside crates/sorcerer-core/src/adapters/platform/: <form>`, and
the script exits 1. `scripts/platform-test` plants every form and checks each is named, that
clean code passes, and that the same forms pass only in sorcerer-core's own `platform/`
directory. CI runs both, and tests the workspace on ubuntu, macOS and Windows.

`.gitattributes` checks every text file out with LF on every platform, and leaves the eval's
recordings (`corpus/scry/*/{reviews,jev,bare}/`, `checks.json`, `corpus/conjure/*/replies/`,
`corpus/apprentice/*/{jev,scenarios}/`) and `*.patch` files
byte for byte, so a recording key made on macOS matches on Windows.

## The rule, checked

`scripts/layers` runs in CI. For each crate's layer directories it refuses a path to a layer
that is not inside (`domain` may not use `ports`, `usecases`, `adapters` or `cli`; `ports` may
not use `usecases`, `adapters` or `cli`; `usecases` may not use `adapters` or `cli`;
`adapters` may not use `cli`), however the path is written: `crate::<layer>` in a `use` or an
expression, an item of a `use crate::{…}` group on one line or several, or a `super::` chain
that climbs out of the file's module into another layer. `domain`, `ports` and `usecases` may
not name `sorcerer_core::adapters` or `sorcerer_models::adapters`. In a `domain/` file it also
refuses `std::fs`, `std::process`, `std::env`, `std::net` (also inside a `use std::{…}`
group), `std::time::SystemTime`, `Instant::now`, `Utc::now`, `Local::now`, `std::io::stdin`,
`reqwest` and `canonicalize`. It reads each file's code up to its `#[cfg(test)] mod tests` (a
test may set up files) and skips comment lines. Each refusal is printed as
`file:line: <layer> may not use <what>`, and the script exits 1. `scripts/layers-test`
plants each kind of violation in a scratch crate and checks every one is refused; CI runs it
too.

## The repository checks itself with spellbook

The repository keeps the same conventions as spellbook data, in `.sorcerer/spellbook/`:

- `rules.toml` holds the half of the scry rules (`.sorcerer/scry.toml`) that a syntax match
  names exactly: `domain-uses-no-other-layer`, `domain-does-no-io`, `ports-depend-on-domain-only`,
  `usecases-depend-inward`, `adapters-do-not-use-cli` (the paths `scripts/layers` refuses) and
  `mod-rs-declarations-only` (what `scripts/mod-rs-declarations` refuses). They are errors, as
  the scripts are. Two things only the scripts do: follow a `super::` chain into another layer,
  and skip a file's `#[cfg(test)] mod tests`; a test module that sets up files says so with a
  `// spellbook:allow domain-does-no-io — …` comment. The prose rules stay with scry.
- `generators/` writes the shapes the crates repeat: `port` (a trait in `ports/` and its adapter
  in `adapters/`), `usecase` (a request and the use case's public function, a command or a query by its `kind`)
  and `cli-subcommand` (an inbound adapter: a cli subcommand and its `…_text`). Each writes new
  files only; the `mod` line in the parent `mod.rs` is the region's (below), and a subcommand's
  place in `arguments.rs` and `run_cli.rs` is added by hand.
- `regions.toml` keeps the module lists. 25 of the 33 `mod.rs` files under `crates/`
  hold owned regions (30 in all), one per shape of line: `domain-modules`, `ports-modules`,
  `adapters-modules`, `usecases-modules` (`pub mod NAME;` per file), `adapters-folders`,
  `usecases-folders` (`pub mod NAME;` per folder with a `mod.rs`), `platform-modules`, and the
  private `mod NAME;` lists `cli-modules`, `lsp-modules`, `usecase-parts`. A new module is a new
  file plus `spellbook sync` (or `spellbook new`, which includes it); never type the line, and
  delete a file and sync drops its line. Four lists stay by hand because no glob describes them:
  `conjure/src/adapters/lsp/mod.rs` (`pub(crate) mod` mixed with `mod`),
  `scry/src/usecases/mod.rs` and `scry/src/usecases/review_change/mod.rs` (public and private
  mixed), and every `tests/support/mod.rs` (`#[allow]` re-exports). A mod.rs with files and
  folders has two regions, so two blocks; `sync --init` of the second puts it just after the first.

CI runs `spellbook check` and `spellbook sync --check` on ubuntu beside the scripts; `spellbook doctor` shows the rules and
generators, and the project needs no trust.
