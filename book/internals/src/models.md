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

### claude

`provider/claude.rs`. Claude Code on a subscription, with no API key: `claude -p
--output-format json` as a subprocess, the prompt on stdin. The flags are olifant's: no
tools (`--tools ""`), no session saved, no settings files, and the child gets
`CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` and no `CLAUDECODE` (so a run started from inside Claude
Code is not refused as nested). The reply's `result` is the text, `total_cost_usd` the cost
and `duration_ms` the time; `is_error` is an error with the result's text.

`run_claude` is shared with the claude researcher. It runs the child in its own process
group and kills the group on timeout or cancel, so nothing `claude` started is left
running. Aliases pass straight through: `claude` understands `sonnet` and `haiku` itself.

### openai-compatible

`provider/openai.rs`. Any server speaking OpenAI's chat completions (Ollama, LM Studio,
vLLM, OpenAI and most others): `POST {url}/chat/completions`, tools in the function shape,
tool results as `tool` messages. There are no shipped aliases; the table's `aliases` say
which model an alias means on that server, and an unmapped alias is an error naming the
key to set. A model name with `/` or `:` (`qwen3:32b`) passes through.

### Choosing a provider

`providers.rs`. `provider_named(project, name, env)` builds the provider of a
`[model.providers.<name>]` table by its `kind` (which defaults to the name).
`default_provider_for(project, env)` uses core's `default_provider` to pick the name and
says why.
