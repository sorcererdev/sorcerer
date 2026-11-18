# Models

`crates/sorcerer-models`: everything that can spend money.

## Providers

`provider.rs`. A `Provider` runs a model: `complete(Completion, &Cancel) -> Reply` is one
turn, with optional tools. A `Completion` holds the model id, the system prompt, the
messages (text, tool calls, tool results), the tools offered, a token cap and a timeout. A
`Reply` holds the text, any tool calls, the usage, the cost when known, why it stopped,
and how long it took.

Products name models by alias (`sonnet`, `haiku`, `strong`, `cheap`) and the provider's
`resolve` turns an alias into the id it sends, so no product names a vendor's model id.

`errand(product, verb, provider, model, &result)` makes the ledger row for a call, success
or failure. `price(prices, model, usage)` turns usage into dollars from a table of
`[input, output]` dollars per million tokens.

`ScriptedProvider` is the test double: its replies in order, each request recorded in
`seen`.
