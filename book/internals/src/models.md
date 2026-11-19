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

## Researchers

`research.rs`. A researcher is a model that may look things up in the project (read a file,
grep, glob) under fences and answer with citations. Scry's find call and the claim checker
both need one. `Researcher::research(Research, &Cancel) -> Researched` takes the product
and verb (for the ledger), the prompt, a model alias, the tools allowed, a spend cap and a
timeout. Every implementation appends its errand to the ledger, success or failure; a
timeout is charged its cap.

There are two ways to get one:

- **`ClaudeCodeResearcher`** (`research/claude_code.rs`): `claude -p` in the project root
  with `--tools Read,Grep,Glob`. The fences become `claude`'s deny rules
  (`Read(./**/.env*)` with its root form `Read(./.env*)`, which fence Grep and Glob too),
  one pair of deny rules for each symbolic link `Fences::fenced_links` finds, and a line in
  the system prompt (`prompts/fence.md`). Web tools are denied. Measured on 2026-11-17:
  `claude` reads through a symbolic link unless the link's path is denied, and reads an
  absolute path outside the project when asked. Denying all absolute paths (`Read(//**)`)
  also denies the project's own files, so that last case is guarded by the prompt alone.
- **`LoopResearcher`** (`research/tool_loop.rs`): core's own tool loop over any provider
  that supports tools.

`RecordedResearcher` (`research/recorded.rs`) answers from `<dir>/<key>.json`, where the
key is FNV-1a 64 of the model and the prompt. A missing recording is an error, unless a live
researcher is given to record it.

### LoopResearcher

`research/tool_loop.rs` and `research/tools.rs`. Core's own tool loop, which makes any
provider with tools (Anthropic's API, an OpenAI-compatible server) a researcher. It sends
the prompt; while the model asks for tools it runs them and sends the results back; it
stops when the model answers, at `MAX_TURNS` (12), or when the spend reaches the cap. One
errand covers the whole research, its usage and cost summed over the turns.

The three tools run over the `CodebaseView`, under fences:

| tool | gives the model | limits |
|---|---|---|
| `read` | the file's lines, numbered, optionally a range | 400 lines |
| `grep` | `path:line: text` for each hit | 100 hits, 200 characters a line |
| `glob` | matching paths | 200 paths |

A fenced `read` returns exactly `refused: <path> is fenced (not evidence)` and never opens
the file. Fenced `grep` hits and `glob` paths are dropped, and `grep` says how many.

## The decider

`decide.rs`. A `Decider` answers typed questions with calibrated probabilities and never
writes text: a `Question` has a kind (its name in the batch), instructions, a `Shape`
(`Choice`, `Noul` with its yes and no sides, or `Score`), named context texts, and whether
"none of these fits" is allowed. `ask` sends a batch as one call with one shared state and
returns a `Weighed` per question: the `Answer` and, for a choice, the distribution.

`decide/jev.rs` is Jev, ported from olifant's `Decide.hs`. `jev_body` builds the request
(context names shared across the batch must agree), the model is pinned (`jev-1.14.0`),
and 429 and 529 are retried twice. A reply that does not fit a question's shape, or a call
that fails, makes the whole batch `Unsure`: a decider never guesses, and a pass that gets
`Unsure` stops. Each call is a ledger row with verb `decide`.

`decide/recorded.rs` answers from `<dir>/<question_key>.json`, where the key is FNV-1a 64
of the question's own Jev body. Given a live `Jev`, it records what is missing.

## The claim checker

`checker.rs` and `checker/prompts.rs`, ported from olifant's plans cascade
(`Server/Cascade.hs`, `Server/Checker.hs`) with the prompt and guard texts word for word but
for two departures. `claim_prompt(claim, languages)` names a unit the way the language of the
claim's files (`claim.about`) does: `unit_sentence` looks each file up with
`Languages::for_path` and uses its `unit_words` (a claim across languages gets each one's,
after the files it covers), else `NEUTRAL_UNIT_WORDS`; and it asks for the line beside a unit,
since a citation can stand on its line. `tests/golden/claim-prompt.txt` was printed by olifant
itself and differs only there (it renders the neutral wording: its claim points at no file).
`CheckerDeps::languages` is `None` in `defaults`; scry's `checker_over` sets the project's.

`check(claim, deps, cancel)` checks a statement about code:

1. **With a decider**, the cheap model (`haiku`) checks first through the researcher. Jev
   then asks three yes/no guards in one call: does the reason speak to the claim
   (`why-fits`), is the cited code what the claim is about (`evidence-fits`), and does the
   verdict follow from the reason (`verdict-follows`). The guards see up to three
   citations with twelve lines of code each. If any guard is under the threshold (0.60,
   where olifant's corpus met every bar), or unsure, or the first verdict is
   `cannot_tell`, or the first check failed, the strong model (`sonnet`) checks again and
   its verdict stands (`escalated`).
2. **Without a decider**, the strong model checks alone.
3. **Citations are confirmed** (`confirm_one`): inside the root, not fenced and readable,
   then the named unit's span; when the unit does not resolve (or none is named), the cited
   line if it is inside the file, and a citation that stood on its line although its unit
   was not found is marked `unit_missed`; with neither unit nor line, the whole file. A unit
   not found with no usable line, a fenced or outside path, an unreadable file or a line past
   the end does not confirm, and makes the verdict `cannot_tell` ("cited code not found").
   `unit_named` reads `.` and `::` alike, since a model may join a method either way whatever
   the prompt said, and takes a bare name that ends exactly one qualified name. Each
   confirmed citation carries the fingerprint of its lines, so a later change to the cited
   code shows. `guard_evidence` agrees: it shows the guards the cited line when it is inside
   the file, else the unit's start.

`Checked` carries the confirmed `citations` (empty when any failed) and `cited`: every
`RawCitation` of the reply the verdict came from, each a `Cited` with `confirmed` and
`unit_missed`, so a `cannot_tell` can say which citation did not confirm and a kept verdict
which stood on a line because its unit name did not resolve. When the check escalated, `cited` is the
strong model's; the first model's citations are not kept.

Scry's verify step runs it on `bug` and `assumption` findings.
