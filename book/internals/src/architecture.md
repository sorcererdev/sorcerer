# Architecture

Every crate is laid out the same way, in layers. A layer is a directory under the crate's
`src/`, and a dependency only ever points inward:

| layer | holds | may use |
|---|---|---|
| `domain/` | data and the rules over it; no I/O (no files, processes, environment, network or clock) | nothing else of the crate |
| `ports/` | the traits the crate talks through, and the data they pass | `domain` |
| `usecases/` | the API: one module a feature, its steps private | `domain`, `ports` |
| `adapters/` | the ports implemented over the disk, git, a model, the clock; the wiring | anything but `cli` |
| `cli/` | parse the arguments, call one use case, print | anything |

A crate root (`lib.rs`, `main.rs`, `error.rs`, `cancel.rs`) sits outside the layers. A crate
uses another crate's public items freely: scry's use cases call core's `Fences::for_project`
and read core's `Snapshot`; the rule is about the layers inside one crate.

A domain type whose construction needs the disk keeps that half as an inherent `impl` in an
adapter file: `struct Project` is `domain/project.rs`, `Project::open` is
`adapters/project.rs`. Rust allows an `impl` block anywhere in the crate, so callers keep the
method names and the domain file stays pure.

## The crates

**sorcerer-core** has no feature of its own, so no `usecases/` or `cli/`.

- `domain/`: `config.rs` (the parsed `.sorcerer/config.toml`), `project.rs` (`Project`),
  `language.rs` (the merged language layers, `Languages`), `query.rs` (compiled queries),
  `unit.rs` (units from the queries), `fingerprint.rs`, `compare.rs` (units before and
  after), `facts.rs`, `errand.rs` (one paid call), `cost.rs` (`spent_in`), `fences.rs` (the
  patterns and `allows_relative`), `fnv.rs`.
- `ports/`: `view.rs` (`CodebaseView`), `ledger.rs` (`Ledger`), `env.rs` (`Env`).
- `adapters/`: `project.rs` (`Project::open`, `state_dir`), `grammar.rs` (grammars fetched,
  built and loaded; `Languages::assemble`, `Languages::load`), `files_view.rs` (`FilesView`),
  `json_store.rs` (`Store`, `Versioned`, `Snapshot`), `jsonl_ledger.rs` (`JsonlLedger`),
  `fences.rs` (`Fences::for_project`, `allows`, `relative`, `fenced_links`), `env.rs`
  (`SystemEnv`, `MapEnv`).

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

**scry**

- `usecases/`: `review_change/` (`mod.rs` with `review_change` and its request, ports and
  report; the steps `scope.rs`, `ask.rs`, `judge.rs`, `remember.rs`, `verify.rs`),
  `list_findings.rs`, `explain_finding.rs`, `dismiss_finding.rs`, `report_cost.rs`,
  `check_setup.rs`.
- `domain/`: `config.rs`, `issue_type.rs`, `finding.rs`, `findings.rs`, `steering.rs`,
  `scope.rs`, `review_prompt.rs`, `judging.rs`, `budget.rs`, `verification.rs`,
  `log_text.rs`.
- `ports/`: `reviewer.rs`, `claim_checker.rs`, `finding_store.rs`, `settings.rs`, `clock.rs`,
  `run_log.rs`.
- `adapters/`: `json_finding_store.rs`, `scry_toml.rs`, `research_reviewer.rs`,
  `markdown_run_log.rs`, `system_clock.rs`, `wiring.rs`.
- `cli/`: `mod.rs` (the clap types, `main`, `main_with`), one file a command (`review.rs`,
  `list.rs`, `why.rs`, `dismiss.rs`, `cost.rs`, `doctor.rs`, `log.rs`), `format.rs`.

[Scry](scry.md) says what each file holds.

## Adding a feature

1. Write the use case: a module under `usecases/` taking a request and the ports it needs,
   returning data (never printing). Its rules go in `domain/` as functions over data.
2. A use case that needs something from outside (a file, a model, the clock) asks a port. Use
   one that exists, or add a trait under `ports/` that says what the use case needs in its own
   words.
3. Implement the port in `adapters/`, and build it in the wiring.
4. Add the command: a file under `cli/` that parses, calls the use case and prints.

An editor's language server is one more caller beside `cli/`: it calls the same use cases.

## Testing each layer

- **Domain**: plain unit tests, no doubles (`#[cfg(test)]` in the file, or `tests/`).
- **A use case**: with the doubles in `tests/support/` (`StubReviewer`, `StubDecider`,
  `FixedClock`) and closures for the checker and the run log (`ClaimChecker` and `RunLog` are
  implemented for closures); the store, ledger and settings over a temporary project
  (`Ports::of`).
- **An adapter**: against the real thing it wraps, a temporary directory, a git repository,
  a scripted provider; never a network or a model.
- **The command line**: `main_with` with a `Wired` of doubles, asserting on stdout, stderr and
  the exit code.

## The rule, checked

`scripts/layers` runs in CI. For each crate's layer directories it refuses a `use crate::…`
of a layer that is not inside (`domain` may not use `ports`, `usecases`, `adapters` or `cli`;
`ports` may not use `usecases`, `adapters` or `cli`; `usecases` may not use `adapters` or
`cli`; `adapters` may not use `cli`), and in a `domain/` file it refuses `std::fs`,
`std::process`, `std::env`, `std::net`, `std::time::SystemTime`, `Utc::now`, `Local::now`,
`reqwest` and `canonicalize`. It reads each file's code up to its `#[cfg(test)]` module (a
test may set up files) and skips comment lines. Each refusal is printed as
`file:line: <layer> may not use crate::<layer>`, and the script exits 1.
