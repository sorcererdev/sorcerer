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

### anthropic

`provider/anthropic.rs`. Anthropic's Messages API: `POST {url}/v1/messages` with the key
from `ANTHROPIC_API_KEY` (or the table's `api_key_env`), tools native. `ALIASES` maps
`sonnet`, `haiku`, `opus`, `strong` and `cheap` to model ids, and `PRICES` holds dollars
per million tokens from Anthropic's pricing page (dated in the source). Both can be
overridden in `[model.providers.anthropic]`.

Every HTTP provider sends through `post_json` in `provider.rs`: a 429 or 529 is retried
twice (after one and two seconds), and the request runs on its own thread so a cancel
returns within a tenth of a second.
