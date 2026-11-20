# A tour

Sorcerer is a Cargo workspace. The libraries are:

| crate | holds | depends on |
|---|---|---|
| `sorcerer-core` | project, config, languages, grammars, units, facts, store, ledger, fences, cost | nothing of ours |
| `sorcerer-models` | providers, researchers, the decider (Jev), the claim checker | `sorcerer-core` |

Products (scry first) depend on both and never on each other. A product that needs
another's results reads a versioned file under `.sorcerer/state/`.

Core is synchronous and makes no model calls. Everything that spends money lives in
`sorcerer-models`, behind a trait with a recorded double, so no test calls a model.

Every crate is in layers (`domain/`, `ports/`, `usecases/`, `adapters/`, and `cli/` in a
product), each depending only on the layers inside it; [Architecture](architecture.md) says
what each holds, how a feature is added and tested, and how `scripts/layers` checks the rule.

## The modules

`crates/sorcerer-core/src/` is in three layers (see [Architecture](architecture.md)):
`domain/` is data and rules with no I/O, `ports/` the traits products talk through, and
`adapters/` the disk, git and the network behind them. `cancel.rs` and `error.rs` sit at the
root.

| module | job |
|---|---|
| `domain/config.rs` | `.sorcerer/config.toml` parsed: core's tables strict, products' tables kept |
| `domain/project.rs` | the project: root, config, languages, state directory |
| `domain/language.rs` | languages as shipped and project layers, merged |
| `domain/query.rs` | compile query files, refusing unsupported predicates |
| `domain/unit.rs` | units from the queries; `unit_at` |
| `domain/fingerprint.rs` | formatter-proof fingerprints |
| `domain/compare.rs` | units before and after a change |
| `domain/facts.rs` | parse errors, TODOs, empty bodies |
| `domain/errand.rs`, `domain/cost.rs` | one paid call; spend per day |
| `domain/fences.rs` | what a model may never read, matched on a resolved path |
| `domain/trust.rs` | `needs_trust`: what in a project's own files must be trusted |
| `domain/fnv.rs` | the one hash |
| `domain/versioned.rs` | `Versioned`, `Snapshot`: a product's state file as data |
| `ports/view.rs` | `CodebaseView`: the questions a product asks about a codebase |
| `ports/ledger.rs` | `Ledger`: append and read errands |
| `ports/env.rs` | `Env`: variables and executables on PATH |
| `adapters/project.rs` | `Project::open`: find the root, read the config and languages; `Project::root_above` |
| `adapters/grammar.rs` | fetch, compile and load grammars at run time; `Languages::assemble`, `Languages::load` |
| `adapters/files_view.rs` | `FilesView`: the working tree, git and tree-sitter |
| `adapters/json_store.rs` | versioned state under a lock |
| `adapters/jsonl_ledger.rs` | `JsonlLedger`: the ledger as `ledger.jsonl` |
| `adapters/fences.rs` | `Fences::for_project`, `allows`, `relative`, `fenced_links`: paths resolved first |
| `adapters/env.rs` | `SystemEnv`, `MapEnv` |
| `adapters/trust_list.rs` | `TrustList`: the user's trusted project roots |

`crates/sorcerer-models/src/` is in four layers: `domain/`, `ports/`, `usecases/` and
`adapters/` ([Architecture](architecture.md)).

| module | job |
|---|---|
| `domain/model.rs` | a model call as data: messages, completion, reply, usage, prices, its errand |
| `domain/research.rs`, `domain/decide.rs` | a research errand; a typed question and its answer |
| `domain/claim.rs`, `domain/claim_prompt.rs` | a claim, its citations and verdict; the checker's prompt and guards |
| `ports/provider.rs`, `ports/researcher.rs`, `ports/decider.rs` | the three seams |
| `usecases/check_claim.rs` | the claim checker's cascade |
| `adapters/{anthropic,claude,openai}.rs`, `adapters/http.rs` | the three providers and their HTTP |
| `adapters/providers.rs` | choose a provider (and the decider) from config and the environment |
| `adapters/claude_code.rs`, `adapters/tool_loop.rs`, `adapters/tools.rs` | researchers: `claude -p`, sorcerer's own tool loop and its tools |
| `adapters/jev.rs` | Jev |
| `adapters/{scripted_provider,recorded_researcher,recorded_decider}.rs` | the doubles tests use |

`crates/scry/src/` adds a fifth layer, `cli/`, beside `usecases/` (the API: `review_change`
and the small commands' use cases), `domain/`, `ports/` and `adapters/` (the store file, the
run log, the settings, the reviewer over a researcher, the wiring). Its module map is in
[Scry](scry.md).

Two demos show the core working on a real repository: `examples/units.rs` in core and
`examples/ask.rs` in models (see the README).
