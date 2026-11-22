# Models

`crates/sorcerer-models`: everything that can spend money, in four layers.

- `domain/`: the data, no I/O. `model.rs` (a call: `ModelId`, `Message`, `Completion`,
  `Reply`, `Usage`, `price`), `research.rs` (`Tool`, `Research`, `Researched`),
  `decide.rs` (`Question`, `Shape`, `UnsureAnswer`, `Answer`, `Confidence`, `Weighed`), `claim.rs` (`Claim`,
  `Citation`, `Verdict`, `CitationCheck`, `Escalation`, `Checked`, the cascade's constants `FIRST_MODEL`, `STRONG_MODEL`, `GUARD_THRESHOLD`) and `claim_prompt.rs` (the
  checker's prompt, its parser and the guard questions).
- `ports/`: the seams. `provider.rs` (`Provider`), `researcher.rs` (`Researcher`),
  `decider.rs` (`Decider`).
- `usecases/check_claim.rs`: the claim check (`CheckClaimDeps`, `check_claim`).
- `adapters/`: `anthropic.rs`, `claude.rs`, `openai.rs`, `http.rs` (`post_json`),
  `providers.rs`, `claude_code.rs`, `tool_loop.rs`, `tools.rs`, `researchers.rs`
  (`researcher_for`), `jev.rs`, and the doubles
  `scripted_provider.rs`, `recorded_researcher.rs`, `recorded_decider.rs`.

## Providers

`ports/provider.rs`. A `Provider` runs a model: `complete(Completion, &Cancel) -> Reply` is one
turn, with optional tools. A `Completion` holds the model id, the system prompt, the
messages (text, tool calls, tool results), the tools offered, a token cap and a timeout. A
`Reply` holds the text, any tool calls, the usage, the cost when known, why it stopped,
and how long it took.

Products name models by alias (`sonnet`, `haiku`, `strong`, `cheap`) and the provider's
`resolve` turns an alias into the id it sends, so no product names a vendor's model id.

Each adapter builds the ledger row (`Errand`) for its own calls. `price(prices, model, usage)` turns usage into dollars from a table of
`[input, output]` dollars per million tokens.

`ScriptedProvider` (`adapters/scripted_provider.rs`) is the test double: its replies in order, each request recorded in
`seen`.

### anthropic

`adapters/anthropic.rs`. Anthropic's Messages API: `POST {url}/v1/messages` with the key
from `ANTHROPIC_API_KEY` only, tools native. `ALIASES` maps
`sonnet`, `haiku`, `opus`, `strong` and `cheap` to model ids, and `PRICES` holds dollars
per million tokens from Anthropic's pricing page (dated in the source). Both can be
overridden in `[model.providers.anthropic]`.

Every HTTP provider sends through `post_json` in `adapters/http.rs`, which returns a
`JsonReply` (the JSON and the seconds taken). `send_until_not_busy` retries a 429 or 529
twice (after one and two seconds); `send` builds the request (`build_request`), runs it on
its own thread (`send_on_its_own_thread`) and checks for a cancel every tenth of a second
while it waits (`await_answer`). Each adapter's `complete` reads the reply with small
helpers named for what they take out (`text_of`, `tool_calls_of`, `stop_reason`).

### claude

`adapters/claude.rs`. Claude Code on a subscription, with no API key: `claude -p
--output-format json` as a subprocess, the prompt on stdin. The flags: no
tools (`--tools ""`), no session saved, no settings files, and the child gets
`CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` and no `CLAUDECODE` (so a run started from inside Claude
Code is not refused as nested). The reply's `result` is the text, `total_cost_usd` the cost
and `duration_ms` the time; `is_error` is an error with the result's text.

`ClaudeRun` (the runner, its arguments, folder, prompt, timeout and extra environment) is
shared with the claude researcher. `ClaudeRun::run` reads as its steps: `start` the child
through the platform adapter's `process::start`, in a group of its own (a process group on
Unix, a job object on Windows, where `claude.cmd` runs `cmd.exe`, which runs `node`),
`Printing::start` (the prompt in, the output read on threads of their own), `wait_for_exit`
(which calls `Running::stop` on timeout or cancel, ending the whole group or job, so nothing
`claude` started is left running), then `reply_from` what it printed. The tests run a fake
`claude` written in Rust (`tests/support/fake_claude.rs`, the `fake-claude` binary), so they
run on Windows too; `a_timeout_stops_the_runner_and_what_it_started` has the fake start a
child of its own and checks both are gone after the timeout. Aliases pass straight through: `claude` understands `sonnet` and `haiku` itself.

### openai-compatible

`adapters/openai.rs`. Any server speaking OpenAI's chat completions (Ollama, LM Studio,
vLLM, OpenAI and most others): `POST {url}/chat/completions`, tools in the function shape,
tool results as `tool` messages. There are no shipped aliases; the table's `aliases` say
which model an alias means on that server, and an unmapped alias is an error naming the
key to set. A model name with `/` or `:` (`qwen3:32b`) passes through.

### Choosing a provider

`adapters/providers.rs`. `provider_named(project, name, env)` builds the provider of a
`[model.providers.<name>]` table by its `kind` (which defaults to the name).
`default_provider` and `default_decider` choose what runs a model when the config does not
say, and why (a `ProviderChoice`, for `doctor` to print): `[model] provider` if set, else
`claude` when it is on PATH, else `anthropic` when `ANTHROPIC_API_KEY` is set; Jev when
`JEV_API_KEY` is set. `default_provider_for(project, env)` uses
`default_provider` to pick the name and builds the provider.

## Researchers

`ports/researcher.rs` and `domain/research.rs`. A researcher is a model that may look things up in the project (read a file,
grep, glob) under fences and answer with citations. Scry's review call and the claim checker
both need one. `Researcher::research(Research, &Cancel) -> Researched` takes the product
and verb (for the ledger), the prompt, a model alias, the tools allowed, a spend cap and a
timeout. Every implementation appends its errand to the ledger, success or failure; a
timeout is charged its cap.

There are two ways to get one:

- **`ClaudeCodeResearcher`** (`adapters/claude_code.rs`): `claude -p` in the project root
  with `--tools Read,Grep,Glob`. The fences become `claude`'s deny rules
  (`Read(./**/.env*)` with its root form `Read(./.env*)`, which fence Grep and Glob too),
  one pair of deny rules for each symbolic link `Fences::fenced_links` finds, and a line in
  the system prompt (`prompts/fence.md`). Web tools are denied. Measured on 2026-11-17:
  `claude` reads through a symbolic link unless the link's path is denied, and reads an
  absolute path outside the project when asked. Denying all absolute paths (`Read(//**)`)
  also denies the project's own files, so that last case is guarded by the prompt alone.
- **`LoopResearcher`** (`adapters/tool_loop.rs`): core's own tool loop over any provider
  that supports tools.

A product does not choose between them itself: `researcher_for(project, env, view)`
(`adapters/researchers.rs`) takes the project's default provider (`default_provider_for`) and
builds `ClaudeCodeResearcher` when its kind is `claude` (a provider is of the kind its name says
unless `kind` says otherwise), otherwise `LoopResearcher` over the provider, `view`, the
project's fences and its ledger. Scry's live wiring and its eval's record mode call it; it lives
here, not in scry, because products never import each other and conjure needs it too.

`RecordedResearcher` (`adapters/recorded_researcher.rs`) answers from `<dir>/<key>.json`, where the
key is FNV-1a 64 of the model and the prompt. A missing recording is an error, unless a live
researcher is given to record it.

### LoopResearcher

`adapters/tool_loop.rs` and `adapters/tools.rs`. Core's own tool loop, which makes any
provider with tools (Anthropic's API, an OpenAI-compatible server) a researcher. It sends
the prompt; while the model asks for tools it runs them and sends the results back; it
stops when the model answers, at `MAX_TURNS` (12), or when the spend reaches the cap. One
errand covers the whole research, its usage and cost summed over the turns.

`research` builds a `ResearchRun` and calls `take_turns`, which resolves the model and
calls `turn` once per model call, recursively. A turn carries `SoFar` (the conversation,
the `Spent` tokens and dollars, the files read) and ends in `Ended` with a `HowItEnded`
(`Answered`, `TurnCapReached`, `SpendCapReached`) or in `Failed` with what was spent by
then. `after_running_tools` adds the model's turn and the tools' results to the
conversation. `charged_for_failure` charges a timeout the whole cap.

The three tools run over the `CodebaseView`, under fences:

| tool | gives the model | limits |
|---|---|---|
| `read` | the file's lines, numbered, optionally a range | 400 lines |
| `grep` | `path:line: text` for each hit | 100 hits, 200 characters a line |
| `glob` | matching paths | 200 paths |

`tools::run` returns a `ToolOutput`: the text the model sees and the file read, if any.
`first_of` keeps the first lines up to a cap and adds one line saying how many were left
out. A fenced `read` returns exactly `refused: <path> is fenced (not evidence)` and never opens
the file. Fenced `grep` hits and `glob` paths are dropped, and `grep` says how many.

## The decider

`ports/decider.rs` and `domain/decide.rs`. A `Decider` answers typed questions with calibrated probabilities and never
writes text: a `Question` has a kind (its name in the batch), instructions, a `Shape`
(`Choice`, `Noul` with its yes and no sides, or `Score`), named context texts, and `unsure` (an `UnsureAnswer`): whether
a choice may be answered "none of these fits". `ask` sends a batch as one call with one shared state and
returns a `Weighed` per question: the `Answer` and, for a choice, the distribution.

`adapters/jev.rs` is Jev. `jev_body` builds the request from `shared_state` (context names
shared across the batch must agree) and `questions_by_kind`, each from `question_body`;
`jev_answers` reads each answer with `chosen_option`, `yes_or_no` or `level`; the model is pinned (`jev-1.14.0`),
and 429 and 529 are retried twice. A reply that does not fit a question's shape, or a call
that fails, makes the whole batch `Unsure`: a decider never guesses, and a pass that gets
`Unsure` stops. Each call is a ledger row with verb `decide`.

`adapters/recorded_decider.rs` answers from `<dir>/<question_key>.json`, where the key is FNV-1a 64
of the question's own Jev body. Given a live `Jev`, it records what is missing.

## The claim checker

`usecases/check_claim.rs`, `domain/claim.rs` and `domain/claim_prompt.rs`: a cascade whose
prompt and guard texts are the ones a prose corpus measured. `claim_prompt(claim, languages)` names a unit the way the language of the
claim's files (`claim.about`) does: `unit_sentence` groups the files by their wording (`files_by_unit_words`), looking each file up with
`Languages::for_path` and uses its `unit_words` (a claim across languages gets each one's,
after the files it covers), else `NEUTRAL_UNIT_WORDS`; and it asks for the line beside a unit,
since a citation can stand on its line. The prompt is built from its sections in order (`task_lines`, `hint_line`, `how_to_check_lines`, `answer_shape_lines`, `unit_sentence`). `parse_reply` reads the reply: the answer object (`answer_object`: the last fenced JSON block, else the first bare object), then `verdict_in`, `why_in` and `citations_in`; `citation_in` reads a path written `orders.py:42` or `orders.py:cancel` as a line or a unit (`path_and_suffix`). `guard_questions` asks the three guards over one shared context (`guard_context`). `tests/golden/claim-prompt.txt` is the prompt the
corpus measured, in the neutral wording (its claim points at no file), with one rule added since (2026-11-20, scry's eval case b03): the verdict is about the sentence's main claim, so a wrong side remark (what a test covers) no longer refutes a true finding.
`CheckClaimDeps::languages` is `None` in `defaults`; scry's `claim_checker` sets the project's.

`check_claim(claim, deps, cancel)` checks a statement about code. Its body is the cascade: `refuse_web_claims`, then `ClaimCheck::by_model` (research, parse, `verdict_with_citations_looked_for`, giving a `ModelVerdict`) for the first model, `guards_trust` for Jev's three guards, and `by_model` again for the strong model:

1. **With a decider**, the cheap model (`haiku`) checks first through the researcher. Jev
   then asks three yes/no guards in one call: does the reason speak to the claim
   (`why-fits`), is the cited code what the claim is about (`evidence-fits`), and does the
   verdict follow from the reason (`verdict-follows`). The guards see up to three
   citations with twelve lines of code each. If any guard is under the threshold (0.60,
   where the prose corpus met every passing score), or unsure, or the first verdict is
   `cannot_tell`, or the first check failed, the strong model (`sonnet`) checks again and
   its verdict stands (`Escalation::ToTheStrongModel`).
2. **Without a decider**, the strong model checks alone.
3. **Citations are confirmed** (`ClaimCheck::confirm`, reading through `CitedFile::open`): inside the root, not fenced and readable,
   then the named unit's span; when the unit does not resolve (or none is named), the cited
   line if it is inside the file, and a citation that stood on its line although its unit
   was not found is `CitationCheck::ConfirmedOnTheLineOnly`; with neither unit nor line, the whole file. A unit
   not found with no usable line, a fenced or outside path, an unreadable file or a line past
   the end does not confirm, and makes the verdict `cannot_tell` ("cited code not found").
   `unit_named` reads `.` and `::` alike, since a model may join a method either way whatever
   the prompt said, and takes a bare name that ends exactly one qualified name. Each
   confirmed citation carries the fingerprint of its lines, so a later change to the cited
   code shows. `guard_evidence` (with `evidence_start`, `evidence_label`, `twelve_lines_from`) agrees: it shows the guards the cited line when it is inside
   the file, else the unit's start.

`Checked` carries the confirmed `citations` (empty when any failed) and `cited`: every
`RawCitation` of the reply the verdict came from, each a `Cited` with its `CitationCheck` (`Confirmed`, `ConfirmedOnTheLineOnly` or
`NotConfirmed`), so a `cannot_tell` can say which citation did not confirm and a kept verdict
which stood on a line because its unit name did not resolve. When the check escalated, `cited` is the
strong model's; the first model's citations are not kept.

Scry's verify step runs it on `bug` and `assumption` findings.
