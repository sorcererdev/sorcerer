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
