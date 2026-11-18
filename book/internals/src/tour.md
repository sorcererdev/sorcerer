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

## The modules

`crates/sorcerer-core/src/`:

| module | job |
|---|---|
| `config.rs` | `.sorcerer/config.toml`: core's tables strict, products' tables kept |
| `env.rs`, `cancel.rs` | the environment as a seam; the cancel flag |
| `project.rs` | find the root; defaults for provider and decider |
| `language.rs` | languages as shipped and project layers; `Languages::load` |
| `grammar.rs` | fetch, compile and load grammars at run time |
| `query.rs` | compile query files, refusing unsupported predicates |
| `unit.rs` | units from the queries; `unit_at` |
| `fingerprint.rs` | formatter-proof fingerprints |
| `compare.rs` | units before and after a change |
| `facts.rs` | parse errors, TODOs, empty bodies |
| `view.rs` | `CodebaseView` and `FilesView` |
| `store.rs` | versioned state under a lock |
| `ledger.rs`, `cost.rs` | every paid call; spend per day |
| `fences.rs` | what a model may never read |
| `fnv.rs` | the one hash |

`crates/sorcerer-models/src/`:

| module | job |
|---|---|
| `provider.rs` | the `Provider` trait, HTTP with retries and cancel, prices, `ScriptedProvider` |
| `provider/{anthropic,claude,openai}.rs` | the three providers |
| `providers.rs` | choose a provider from config |
| `research.rs`, `research/*` | researchers: `claude -p`, core's own tool loop and its tools, recorded |
| `decide.rs`, `decide/*` | the decider: Jev and recorded answers |
| `checker.rs`, `checker/prompts.rs` | the claim checker, ported from olifant |

Two demos show the core working on a real repository: `examples/units.rs` in core and
`examples/ask.rs` in models (see the README).
