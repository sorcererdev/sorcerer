# sorcerer-models

Everything in Sorcerer that can spend money: model providers, researchers, the Jev decider and the claim checker.

## Who uses it

scry, conjure and apprentice. Products name models by alias (`sonnet`, `haiku`, `strong`, `cheap`) and never a vendor's id.

## Main parts

- `ports/`: `Provider` (one model turn), `Researcher`, `Decider`.
- `adapters/`: Anthropic, OpenAI-style APIs, `claude -p`, the tool loop, Jev, and test doubles (`ScriptedProvider`).
- `usecases/check_claim.rs`: a claim, checked against evidence in a cascade of models.
- Every call writes a row to the cost ledger in `sorcerer-core`.

## Example

Ask Jev a question and run a research errand (spends real money, cents):

```sh
cargo run -p sorcerer-models --example ask
```

Book: [Configuration](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/configuration.md) (using it), [Models](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/models.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
