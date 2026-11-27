# Apprentice

`crates/apprentice`: a coding agent whose whole interface is the editor. Package
`sorcerer-apprentice` (`publish = false`, `dist = false`: not on crates.io and not in a release
until the subscription question is asked), library `apprentice`, binary `apprentice`, and the
test program `apprentice-fake-claude`. The design is
`apprentice-prototype-design-2026-11-26.md` in the planning notes.

## The shape

```
 editor ──LSP──▶ apprentice lsp (a Product on sorcerer-lsp; adapters/lsp/)
                   │
                   ├─ run_task ── Agent port ──▶ claude -p (stream-json in and out)
                   │                               │ --mcp-config → apprentice mcp ─┐
                   │                               │ --settings   → apprentice hook ┤ loopback
                   └─ TaskRunner ◀── loopback port (127.0.0.1, token) ◀─────────────┘
```

A task is one `claude -p` child. Its helper processes (the MCP server that serves the medium
tools, and the hooks) are the child's children, not the server's, so they reach the server
through a loopback port: `127.0.0.1`, a port the system picks, one JSON line in and one out per
connection, and a token of the server's own in every request (`new_token`: 16 bytes from the
operating system's random source, through `getrandom`). Loopback TCP is one code path on
three systems; pipes and sockets would be two. Any local process can connect, so `Loopback`
bounds a peer (`LIMITS`): the request line within 5 s and at most 16 MiB, at most 64
connections answered at once; past that a connection is closed unanswered.

## Module map

**Domain** (`src/domain/`), no I/O:

- `config.rs`: `ApprenticeConfig` (`[apprentice]`), `apprentice_config`, `PRODUCT`.
- `task.rs`: `TaskState`, `TaskFile`, `TaskRecord` (one line of `tasks.jsonl`; the last line of
  an id is the task), `TaskRows`, `latest_of_each`, `task_id`.
- `agent_event.rs`: `AgentEvent` (`Started`, `ToolUse { name, input, parent }`, `Text`,
  `Used { message_id, model, usage }`, `Finished`) and `events_of_line`, the stream-json reader:
  an `assistant` message's `tool_use` and `text` blocks (`parent_tool_use_id` names a
  subagent's) and its `usage` (the stream repeats a message once per block, under one id), the
  `system`/`init` line's session id, the `result` line's `total_cost_usd`, `num_turns`,
  `is_error`.
- `token_cost.rs`: `TokenUsage`, `usage_of` (a message's `usage`, cache writes split 5m / 1h),
  `UsageSeen` (the calls by message id, the last usage of each; `cost` at a model's
  `[input, output]` price, cache reads at 0.1, writes at 1.25 or 2 times the input price;
  `None` when nothing was seen or a model has no price).
- `status.rs`: `activity_of` (`reading booking.py`, `subagent: …`; a `Bash` call the stream
  shows is `waiting until allowed: make`, since every command passes the permission tool, which
  says `running make` once allowed; Claude Code's `ToolSearch` is `getting ready`),
  `CommandVerdict` and `command_activity` (`waiting for you: ls`, `running ls`, `denied: ls`; a
  chain is named by `programs_of`: the first word of each part split at `;`, `&&`, `||`, `|`,
  `&`, past `NAME=value` and brackets, each once, at most 3, so `ls; cat x` is `ls, cat`),
  and `status_text`; the status line is derived, never written by the agent.
- `mediums.rs`: the six tools and the permission tool (`tool_definitions`: names, descriptions,
  JSON schemas), `ToolCall` and `tool_call_of`, `Ask`, `AskWay`, `MarkSeverity`, `is_a_medium`.
- `loopback_message.rs`: `LoopbackRequest { token, task, body }` with `RequestBody::Edits |
  Tool | Permission`, `LoopbackReply`, and the three environment variables of the way back.
- `edits_text.rs`: `FileEdit`, `diff_text` (unified, two lines of context),
  `edits_context_text` (the hook's text) and `unsaved_edits_text` (the first message's).
- `permissions.rs`: `AllowedCommands` (`permissions.json` version 2: exact command lines,
  `allows`, `with`), `CommandPrefixes` (version 1, first words, and `migrated`), `always_for`
  (the line *always* remembers, or `None` for a chain, a glob or a tree reader),
  `words_naming_paths`.
- `tree_readers.rs`: `TreeReaders` (`shipped`, `reads_tree`) over
  `library/tree-readers.toml`, the commands that read whole trees: each a program, a
  subcommand for one that has them, and `always` or the `options` that make it one. It is
  data because sorcerer's code names no tool. A word is matched by `program_name`: its file
  name, unquoted, lower case, less a Windows suffix (`.exe`, `.cmd`, `.bat`, `.com`), so
  `C:\tools\RG.EXE` is `rg`.
- `paths.rs`: `project_relative` (conjure's `lexical_relative`, copied), `is_never_written`
  over `library/never-written.toml` (data, like the tree readers: names any part of a path may
  not be, ignoring case: `.sorcerer`, `.git`, `.claude`, `.mcp.json`, `.envrc`).
- `budget.rs` (`spent_today`, `triage_spent_today`, `day_allows`, `triage_allows`),
  `estimate.rs` (`estimate_usd`: the median after five tasks), `log_text.rs` (`usd`,
  `title_of`, `offer_title`, `done_text`, `task_mark_text`, `task_line`), `prompt.rs`
  (`EDITOR_RULES`, `TaskPlace`, `task_prompt`).
- `region.rs`: `Region` (id from the file and the squashed text, `RegionPlace`: comment,
  prose, stub, hunk), `found_in` (where its lines are now, nearest its old place; `None` once
  they changed), `region_id`.
- `cutting.rs`: `Shape`, `ShapeUnit`, `persons_changed_lines` (over `similar`: the changed
  lines less a line a tool landed, a line already in the file, a change that is the same text
  once `layout_free` takes out whitespace, brackets, commas, semicolons and quote marks, a line
  of layout only; a deletion is the line after it unless every deleted line is still in the
  file), `regions_of` (comment
  blocks touching a change; prose paragraphs where no language covers the file; stubs; the rest
  as hunks of at most 40 lines with a letter in them).
- `triage.rs`: `Kind`, `Triaged`, `KIND_CUT` and `ADDRESSED_CUT` (0.70), `free_kind` (a stub
  is an instruction), `questions_for` (the four questions, sharing the region's `text` and
  `where`, so Jev takes them as one request), `verdict_of`.
- `region_record.rs`: `RegionRecord` (the region, its triage, its `Reply`, dismissed),
  `ReplyVerdict`, `RegionRecords` (`regions.json`, `Versioned`), `Label` (a line of
  `labels.jsonl`); `offers_a_task` (an instruction, or a claim that does not hold),
  `waits_for_a_reply`.
- `answer_prompt.rs`: `question_prompt`, `read_answer` (`LONG:`, the `path:line` citations).
- `scry_notes.rs`, `rules.rs`: scry's findings file and steering file, read by apprentice's own
  readers (copies of conjure's: products never import each other).

**Ports** (`src/ports/`): `agent.rs` (`Agent::start(&TaskRequest) -> Running`;
`Running::next_event(wait)`, `close_input`, `stop`; `TaskRequest`, `WayBack`, `NextEvent`),
`editor.rs` (`Editor`: `notify`, `mark`, `ghost`, `write_file -> Written`, `note`, `ask ->
Asked`, `edits_since_look`, `progress`, `finished`), `task_store.rs`, `journal.rs`,
`permission_store.rs`, `project_context.rs` (the unit at a line, scry's notes near it, the
rules), `syntax.rs` (`Syntax::shape`, `comment_prefix`), `region_store.rs` (`RegionStore`:
`get`, `of_file`, `put`, `label`), `clock.rs`.

**Use cases** (`src/usecases/`): `run_task/` (below), `triage_regions.rs` (`triage_change`,
`TriageDeps`, `Look`, `acted_on_in`), `answer_regions.rs` (`answer_saved`, `AnswerDeps`),
`label_region.rs` (`label_region`, `dismiss_region`), `offer_task.rs` (`offer_text`),
`end_task.rs` (`record_task_end`), `report_cost.rs` (a day a row, by verb), `show_log.rs`.

**Adapters** (`src/adapters/`): `claude_agent.rs` (`ClaudeAgent`), `loopback.rs` (`Loopback`,
`ask_loopback`), `helper_way.rs` (`HelperWay::from_env`, `ask`), `mcp_server.rs` (`serve_mcp`),
`hooks.rs` (`hook_output`), `task_runner.rs` (`TaskRunner`: the loopback open, the live tasks by
id), `jsonl_task_store.rs`, `markdown_journal.rs`, `json_permissions.rs`, `project_context.rs`
(`FilesContext`), `scry_files.rs`, `steering_files.rs`, `state_files.rs` (`StateFiles`: every
path under `.sorcerer/state/apprentice/`), `disk_editor.rs` (`DiskEditor`, the command line's
editor), `grammar_syntax.rs` (`GrammarSyntax`: comments from core's `Captures` and
`COMMENT`, units from `units_of`, stubs from `facts_of`'s `body_empty`), `json_region_store.rs`,
`system_clock.rs`, `wiring.rs` (`live_agents`: the agent builder, holding one core
`FencedLinksCache` per project root for the server's life; `task_deps`, `WireAgent`), and `lsp/` (the
server, below).

**Command line** (`src/cli/`): `run_cli.rs` (`main`, `main_with`, `Wiring`), `arguments.rs`,
`run.rs` (`apprentice FILE:LINE "…"`), `mcp.rs`, `hook.rs`, `cost.rs`, `log.rs`, `doctor.rs`,
`trust.rs`, `lsp.rs` (`apprentice lsp`), `console.rs`, `format.rs`.

## A task, end to end

`TaskRunner::open(deps)` opens the loopback port with a handler that finds the live task a
request names and calls `run_task::answer_request`. Then `TaskRunner::run(order)`:

1. **Start** (`start.rs`, `start_task`). `refuse_a_spent_day` (`spent_today` over the ledger
   against `budget_per_day`); the record, `running`, appended to `tasks.jsonl`; the
   instruction journaled; the first message (`task_prompt`: the instruction, where it was
   written, the unit around it from `ProjectContext::unit_at`, scry's open findings near it, the
   rules, the editor's unsaved edits, less a fenced file's: `sendable`); `Agent::start` with the
   way back, once the live task is findable by its helpers (`TaskRunner::start_with` inserts it in
   the `before_the_agent` step: an agent may call back before `start` returns). An instruction in a fenced file is refused (`TaskRefused::Fenced`). A start that
   fails is recorded `failed` and said (`TaskRefused::NotStarted`).
2. **Follow** (`follow.rs`, `follow_task`), on the calling thread: every 200 ms it checks the
   task's `Cancel` (a stop) and takes the next event. A session id is recorded; a tool call
   becomes the status line (`activity_of`, `status_text`, `Editor::progress`, through
   `show_activity`) and a journal line, and a medium's call marks the turn as spoken. A command
   the person was asked about is shown by its verdict (`LiveTask::take_command_verdict`):
   claude's stream may show the call after the answer. When it shows the call first, the answer
   that follows is not kept (`Verdicts` in `task_deps.rs` counts the calls shown before their
   answer), so a later run of the same command is never shown by an old verdict. A message's `Used` is kept on the
   `LiveTask`; the status line's amount is `LiveTask::cost_seen`. `Finished` closes the agent's input;
   when the turn used no medium its final text is posted (`notify` when it is one line of at
   most 200 characters, else `note`).
3. **Helpers' requests**, meanwhile, each on a loopback thread (`answer_request.rs`):
   - `Tool` → `answer_tool_call` (`tool_calls.rs`): the arguments read (`tool_call_of`; a
     missing one is an error result naming it), paths checked (`project_relative`, the fences,
     never a path `is_never_written` names), then the editor. `write_file` journals the file's text before
     once (`Journal::keep_before`), counts the file as the task's, and answers `written: …`;
     `Written::MovedOn` answers an error with the file's text now. `ask` sets the task `asking`
     while it waits.
   - `Permission` (and the `permission` tool) → `answer_permission` (`permissions.rs`): the
     writers and `AskUserQuestion` denied with a sentence; a command naming a fenced path denied
     (`fenced_path_named`); a command line allowed always (`AllowedCommands::allows`) passes;
     else the picker, with *Always* only when `always_for` gives a line, and the instruction's
     file and line as `Ask::at`, so an editor with no picker is asked there. The answer is the permission tool's JSON, `{"behavior":"allow","updatedInput":…}`
     or `{"behavior":"deny","message":…}`, with its `CommandVerdict`; the status line says
     `waiting for you: …` while the picker is up, then `running …` or `denied: …`
     (`say_where_the_command_stands`, which journals each as a `tool` entry), and the verdict is
     kept for the stream. Because every command goes through here, `saw` (`follow.rs`) journals
     no command itself: the stream usually shows the call before the question, and the journal
     would say `running ls` for a command still waiting. The status line it sets for such a call
     is `waiting until allowed: ls` (`activity_of`), never `running`.
   - `Edits` → `edits_for_agent` (`feed.rs`): `Editor::edits_since_look` (which moves the look
     to now) as `edits_context_text`, journaled; at `stop` only journaled.
4. **Finish** (`finish.rs`, `finish_task`): the state (`done`, `failed`, `stopped`), the cost
   (the reported one; a stop is charged `LiveTask::cost_seen` at `TaskDeps::price_of`, which
   the wiring sets to `sorcerer_models::adapters::anthropic::shipped_price`; a stop that saw no
   call, an early end or a result with no `total_cost_usd` is charged the whole
   `budget_per_task`), the
   record, one ledger errand (product `apprentice`, verb `task`), the done message
   (`done_text`) and `Editor::finished`.

`LiveTask` (`task_deps.rs`) is what the follow loop and the loopback threads share: the record
behind a mutex, whether the turn spoke, the calls' usage seen (`used`, `cost_seen`), the
person's verdict on each command asked about, the `Cancel`.

## The harness: `claude -p` headless

`ClaudeAgent::args` (`claude_agent.rs`), in order: `-p`, `--output-format stream-json`,
`--input-format stream-json`, `--verbose`, `--model`, `--max-budget-usd`, `--max-turns`,
`--setting-sources ""` (no project settings, hooks or `CLAUDE.md` reach the child; core's trust
gate stays in front), `--settings` (the two hooks, `PostToolUse` and `Stop`, each `"<apprentice>"
hook <event>` with `/` in the path on every system; then `permissions`: `deny` holds the five
tools and every fence as `Read(./…)` (a `dir/**` fence also as `Read(./dir)`, each rule once,
and `Read(./link)` with `Read(./link/**)` for each fenced link, from the project's
`FencedLinksCache` given by `with_links_cache`, else a fresh `Fences::fenced_links` walk),
which Claude Code applies to Read, Grep, Glob, `@`-mentions and the file commands it parses
(`cat .env`, `head`, `cp .env x`); `ask` is `ASKED_TOOLS`, `Bash`, `PowerShell` and `Monitor`,
so every command reaches the permission tool, even the ones Claude Code otherwise runs unasked
as read-only; `blockReadsOutsideWorkingDirectories` makes the file tools refuse paths outside
the project), `--mcp-config` (one stdio server, `apprentice mcp`; the way back is not in it,
since an argument is readable by any local user through `ps` or `/proc/<pid>/cmdline`: the server
inherits it from `claude`'s environment)
with `--strict-mcp-config`, `--allowedTools` (`ALLOWED_TOOLS`: the mediums, Read, Grep, Glob,
Agent, Task, TodoWrite, WebSearch, WebFetch; never a command tool, since a tool allowed there is
never asked about; naming Grep and Glob also brings them back on macOS and Linux, where Claude
Code otherwise searches through `Bash`), `--permission-prompt-tool mcp__apprentice__permission`,
`--append-system-prompt` (`EDITOR_RULES`). The environment drops
`CLAUDECODE`, sets `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` and the three `APPRENTICE_*` variables
(`way_back_env`). Every command the agent runs inherits them too, the token included: that is the
agent's own trust (it may call the tools anyway), and the hooks need them there. The
child starts with core's `platform::process::start` (a process group or a job object, so a stop
ends everything it started). The task goes on standard input as one line,
`{"type":"user","message":{"role":"user","content":…}}` (`user_message_line`); a reader thread
turns each output line into events (`events_of_line`) on a channel; the error output is read
away. No argument holds a newline (npm's `claude.cmd` refuses one) and the line stays under
cmd.exe's 8191 characters (a test pins both).

`apprentice mcp` (`mcp_server.rs`) is JSON-RPC 2.0 a line: `initialize` is answered at once
(the agent waits at most 30 s), `tools/list` from `tool_definitions`, `tools/call` as one
loopback `Tool` request answered as one text block with `isError`. `apprentice hook <event>`
(`hooks.rs`) asks for `Edits` and prints
`{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":…}}`, or nothing; it
always exits 0. The `Stop` hook prints nothing: a stop's only way to say more is to refuse the
stop, and a task that should go on is a new instruction.

**A server that has ended.** `claude` runs in a process group (a job object on Windows) of its
own, so killing `apprentice lsp` does not kill it. Its helpers notice instead: `HelperWay::ask`
(`helper_way.rs`) answers `ServerAnswer::Gone` when the loopback port refuses the connection
(nothing listens there any more). `ask_loopback` connects through the platform adapter's
`connect_local`, so the refusal comes at once on Windows too, where a plain connect retries for
about two seconds first (the harness test once took 6 s there: three helper calls at 2 s each).
`serve_mcp` then fails the call with `SERVER_GONE` and exits,
so every later `mcp__apprentice__*` call fails at once; `hook_output` prints
`{"continue":false,"stopReason":…}`, which ends the agent's work after the tool call it follows
(confirmed on `claude` 2.1.331 under `-p`: after a `Read` the hook stopped, no second call, the
result came at once). The agent's input is the dead server's pipe, so it is closed too, and
`claude` exits once its turn is over. Only a turn with no tool call at all could still spend, and
the spend cap bounds it. Any other failure to connect (a timeout, a reset) stays an error reply:
the server may only be busy. Tying the child's life to the server directly was not done: the
Windows job object `process-wrap` makes has no kill-on-close flag, and nothing on Unix ends a
process group when its parent dies without code per platform.

### Fences against the real `claude`

Measured with `claude` 2.1.331 on haiku, on a copy of the bistro demo with a canary in `.env`
and in a `not_evidence` directory (report: `apprentice-secrets-gap-report-2026-11-26.md` in the
notes). Without the `ask` rule Claude Code ran its read-only commands unasked, and `grep -r KEY
.`, `grep -rh`, `rg --hidden`, `git grep`, `git show HEAD:.env`, `git log -p` and `cat s*/keys.txt`
printed the canary; the deny rules alone stopped `cat .env`, `head`, `tail`, `base64 -i`, `sort`,
`cat < .env`, `cp .env …`, Read, Grep, Glob and `@.env`. A bare `Read` in `--allowedTools` also
let Read open any file outside the project. With the `ask` rule and the outside-read block,
every command reached the permission tool and nothing printed a canary.

`answer_permission` (`usecases/run_task/permissions.rs`) refuses a command that names a fenced
path before it asks or checks the always list (`fenced_path_named`, over
`domain::permissions::words_naming_paths`, which splits at spaces, quotes, operators, `:` and
`=`): a fenced file, or a directory all of whose contents are fenced. *Always* remembers the
exact command line and is never offered for a tree reader, a glob or a chain, so only a command
the person allowed once, or the exact line they allowed always, can read a fenced file without
naming it. `json_permissions.rs` migrates a version 1 file on first read
(`CommandPrefixes::migrated`: bare program names and tree readers dropped). Claude Code's
sandbox would stop that at the system level on macOS and Linux (measured: `grep -r` and `rg` got
"Operation not permitted" on the fenced paths), but it is not on: it has no native Windows
support, and with `.git` fenced and writes outside the project refused, `git status` and `uv run
pytest` (its cache in `~/.cache/uv`) failed under it.

`tests/harness.rs` pins this: the fake `claude` runs `cat`, `grep`, `git`, … unasked unless
`permissions.ask` names `Bash`, as the real one does, and
`every_command_reaches_the_permission_tool_and_every_fence_is_a_deny_rule` checks every command
reached the permission tool, the `ask`, block and deny rules, and that no flag widens
permissions. `a_command_naming_a_fenced_path_is_refused_even_when_allowed_always`
(`tests/run_task.rs`) pins the refusal; `a_search_allowed_always_does_not_let_a_recursive_search_run`
(harness) and `allowing_a_search_always_does_not_allow_a_recursive_one` and
`a_version_1_grant_of_a_bare_program_no_longer_lets_it_run` (`run_task.rs`) pin the exact lines.

**Seen working against the real `claude`** (2.1.331, haiku; the tests run the fake): the stdin
framing of `--input-format stream-json`, the permission tool's input (`tool_name`, `input`) and
its answer obeyed, the MCP tools, the cost and session in `result`; and, on 2026-11-26, the
`PostToolUse` hook given inline through `--settings` under `-p`: a line added to a file the task
had written reached the agent after its next tool call (the journal's *edits fed*), and the MCP
server found the way back in the environment it inherited, with no `env` in `--mcp-config`.

## The server

`adapters/lsp/apprentice_server.rs`: `serve(connection, ServerConfig)` plugs `ApprenticeServer`
into the host as a product that tracks text. `ServerConfig` (`server_config.rs`): `open`,
`cache`, `trust`, `clock`, `agent` (`WireAgent`), `decider` (`WireDecider`: Jev for the project,
recording under `apprentice`, or none), `researcher` (`WireResearcher`), `idle` (1.5 s; the tests
use 50 ms). The shared `Server` (`server_state.rs`) holds the client, the root, the `Jobs`, the
last looks, `.git/HEAD`, the runner, each running task's looks and writes and progress line,
the finished tasks still marked (`ShownTask`), the agent's marks and the pending questions.
`trusted_project` opens the project afresh at every look (config edits need no restart) and,
untrusted, says `NOT_TRUSTED` once and does nothing.

**The two clocks** (`watch.rs`). `opened` starts a file's look at its saved text (`look_from_open`): when the buffer is not the disk (the person typed while the editor was still starting the server, which VS Code does without waiting, or the editor restored an unsaved buffer) the look is the disk's and a `look_at` is submitted as if they had typed, so those edits are triaged too; a file not on the disk starts at its text now. `changed` submits `look_at` under `idle:<path>`; `saved` cancels
it, then runs `look_at` and `answer_on_save` under `save:<path>`. `look_at` takes the text now,
swaps it for the last look, and (unless `head_moved`, which resets every look) runs `triage`
(`regions.rs`, building `TriageDeps` over `GrammarSyntax`, `JsonRegionStore`, the ledger and the
decider) on the change, then `publish`. A task's own `write_file` moves the look
(`looked_at_own_edit`), so its edits are never triaged as the person's. Edits the other
servers land are told apart through the host's `tool_edits` note: `triage` reads
`lines_tools_landed` for the file into `Look::landed_by_tools`, and `persons_changed_lines`
leaves those lines out (with moved lines, reflows and blank lines, which is what
format-on-save makes). `answer_on_save` builds
`AnswerDeps` (the researcher over core's `FilesView` of the disk, which the save just wrote) and
writes a long answer as a note too.

**The marks** (`publisher.rs`): one publish per file joins the regions' marks
(`region_mark`: an answer or a verdict Information with its cost and time and the citations as
`relatedInformation`, else in the message; an instruction a Hint with the offer's price; an
offer inside a larger offering region, a comment inside a stub, is not drawn:
`inside_another_offer`), the
finished tasks' (`task_marks`, code `task`, `data.task`; the file whose relative path is the
record's `file` gets the count, the others `other file`), the agent's (`said_marks`) and the
questions' (`ask_marks`). Ranges end at the end of the last line (`lines_of`, UTF-16).

**The actions** (`commands.rs`): `code_actions` gives, per line, the found regions' actions
once (`regions_actions`: the offer of the offering region spanning the most lines while no task
runs, *Show code* and *Keep this answer* of the first answered region, one *Dismiss* and the
four labels carrying every region's id: `apprentice.dismiss [ids…]`, `apprentice.label [word,
ids…]`), the pending questions' options (`ask_actions`), a marked task's *Show diff*,
*Undo this task*, *Show journal*, and *Stop this task* while one runs. `execute` routes the ten
`apprentice.*` commands; the slow ones (*do this*, *Dismiss*, a label) go to a thread.

**A task from the editor** (`tasks.rs`). `do_region` makes a `TaskOrder` (the region's text, its
place now, the open files' unsaved edits) and `start_task`, holding `Server::runner` from the
check to the start (so two *do this* at once start one task), refuses a second task, builds a
`TaskRunner` over `task_deps` with an `LspEditor` (`editor_port.rs`, holding the server weakly),
starts it with `TaskRunner::start_with`, giving it its first look (every open file's text now, `open_texts`) before its agent starts, and follows it on a
thread. `open_texts` keeps only the files `Server::may_send_under` passes (inside the project,
not `.sorcerer/` or `.git/`, allowed by the project's `Fences`, opened afresh; none when they do
not open), so neither the first look, the unsaved edits nor the hook's diffs carry a fenced
buffer or a journal open in the editor; `triage` and `answer_on_save` skip a fenced file
(`Server::may_send`). `LspEditor`:

- `write_file` → `write_for_task`: refused when the editor cannot apply edits (or create a
  file); `MovedOn` when the file is not what the task last looked at
  (`moved_since_task_looked`), or when the buffer is not the disk and the task has neither
  written the file nor been sent its buffer (`unsaved_and_unseen`; the agent's `Read` shows the
  disk; the file goes into `told_unsaved` and the look moves to the buffer, so the next write
  lands); then one `workspace/applyEdit` built by the host's
  `landing::workspace_edit` at the buffer's version (checked with `moved_on`), waiting up to
  10 s for the answer and 3 s for the editor to report the text (`wait_until_reported`; Neovim
  answers first). Texts are compared with the host's `same_text`, so a final newline the
  editor adds (Neovim's `eol`) or its line endings are not the person's edit, here, in
  `edits_since_task_looked` and in `undo_of`. The edit goes out through
  `Client::apply_tool_edit`, noted as a tool's. The task's look and the server's look move to the new text, the text is kept
  for undo (`written`), and `show_working` draws `⟳ apprentice: writing…` at the first changed
  line.
- `edits_since_look` → `edits_since_task_looked`: each open file against the task's look, the
  look moved to now.
- `ask` → `ask_person` (`asks.rs`): on a line (a `PendingAsk`, its mark, a `say`, one code action
  per option, `apprentice.answer` sends the pick down a channel) or in the picker
  (`Client::ask`); an editor that cannot show the picker (an error reply, `Answer::Unsupported`)
  falls back to the line. A question of several lines is cut by `Question` (`asks.rs`): the
  picker and the `say` show its first line (the picker's question said whole first), the mark's
  message is the first line then the rest, for the hover. A line question's id is
  `<task>-<n>` from the `asks_put` counter, never reused.
- `mark` keeps a `SaidMark`; `ghost` and `note` send `sorcerer/ghost` and `sorcerer/note` to an
  editor that handles them (`Client::notify_sorcerer`), else a message and `show_document`.
- `progress` starts the task's `WorkDone` and reports on it; `finished` → `task_finished`: the
  progress line ends, `clear_working`, a `ShownTask` per file it changed (the before-text from
  the journal, the after-text from `written`, the first changed line) and its marks published,
  then the done message, once: it is the person's cue to look at the marks.

*Show diff* sends `sorcerer/showDiff { uri, before, title }` (else shows the journal's
before-file). *Undo this task* plans each file with the host's `undo_of` (from
`versioned_text_now`), sends the undoable ones as one `undo_edit` and, once applied, records
`undone` (`record_task_end`) and names the files left alone. `keep_on_save` settles a saved file
and records `kept` once every file is. At start, `interrupt_what_was_left_running` records any
task still `running` or `asking` as `interrupted`.

## The editor clients

The server sends `sorcerer/ghost`, `sorcerer/note` and `sorcerer/showDiff` only to a client
that lists them in `initializationOptions.handles` (`Client::notify_sorcerer`); every other
client gets the fallbacks above. Both clients also set `drawsWorking`, run the server's code
actions from keys by title, and leave marks, the picker, edits and progress to the editor.

**Neovim** (sorcerer.nvim, `lua/sorcerer/apprentice.lua`): `client_config` adds the handlers to
`working.lua`'s config; `attach` starts the server with `debounce_text_changes = 0` on every
file buffer under the project root; `ghost` draws `virt_lines` extmarks (cleared on
`TextChanged` of that line), `note` `vsplit`s, `show_diff` opens a tab with a scratch buffer of
the before-text and the live buffer, both `diffthis`. The keys are in `lua/sorcerer/keys.lua`.

**VS Code** (`editors/vscode/apprentice/`). `src/extension.ts` starts one `apprentice lsp` per
workspace folder (`startFor`, every file under the folder) with `initializationOptions: {
drawsWorking: true, handles: [...] }`. VS Code's client holds a whole-text `didChange` back up
to 250 ms (the server's sync is `Full`); the `didChange` middleware follows each change with the
notification `$/apprentice/sendEditsNow`, which the server ignores but which makes the client
send the pending change first, as Neovim's `debounce_text_changes = 0` does. The handlers:
`Ghosts` (`src/ghosts.ts`) draws each ghost as an `after` decoration on its line in the theme's
`editorGhostText.foreground`, the lines on hover, and moves or drops it on
`onDidChangeTextDocument` with the pure `afterChanges` (`src/ghost.ts`); `openBeside` shows a
note in `ViewColumn.Beside` with `preserveFocus`; `BeforeTexts` (`src/showDiff.ts`) is the
`apprentice-before` content provider and runs `vscode.diff` of a before-URI (the file's path,
a fresh query per diff) against the live file. `src/taskMarks.ts` (over the pure `taskLines`
in `src/marks.ts`) draws the gutter sparkle on diagnostics of source `apprentice`, code `task`.
`src/keys.ts` (`KEYS`, `actionsFor`: exact titles, or a prefix for the offer and the labels) and
`src/keyActions.ts` (`vscode.executeCodeActionProvider` at the cursor; the labels in a
QuickPick) make the `ctrl+m a/q/d/z/j/x/l` commands, `apprentice.keys.*`, named apart from the
server's `apprentice.*` commands, which the language client registers. `binary.ts`,
`download.ts`, `offer.ts`, `working.ts` and `inline.ts` are byte-identical copies of scry's and
conjure's (CI `diff`s them); with no binary the offer finds no release (a 404) and says the
`cargo install` line once per version. Permission questions are the language client's own
`window/showMessageRequest` handler, a notification with buttons; a closed one answers `null`,
which the server reads as `Answer::Closed`, a deny.

## Tests

`tests/run_task.rs`: the use cases over a `ScriptedAgent` (`tests/support/scripted_agent.rs`)
that reports events and calls back through the **real** loopback port, as its helpers would, and
a `RecordingEditor` (buffers in memory, `person_edits` to type meanwhile, answers from a
queue). `tests/harness.rs`: `ClaudeAgent` against `apprentice-fake-claude`
(`tests/support/fake_claude.rs`), which writes its arguments and first input line to files,
plays a script, really starts `apprentice mcp` from `--mcp-config` and really runs `apprentice
hook` from `--settings`, logging each result; like the real one it stops at a hook's
`continue: false` (`an_agent_whose_server_has_ended_stops_after_its_next_tool_call` points the
way back at a closed port). `tests/cli.rs`: `main_with` with a `Wiring` of
doubles. `tests/lsp.rs`: the server over `Connection::memory()` with a small editor
(`tests/support/lsp_editor.rs`: buffers, versions checked, every `workspace/applyEdit` applied
and reported, pickers answered from a queue), `StubJev` and `StubResearcher`
(`tests/support/model_doubles.rs`) and the vendored grammars: the design's walk (a claim
checked on save and offered, a task that writes, asks on a line, asks before a command, then
*Show diff* and *Undo this task*), keeping on save, a stub offered by rule, a question answered
and kept, labels and dismissal, an untrusted project, ghost and note. No test calls a model.

The VS Code extension (`editors/vscode/apprentice/`): `npm run test:unit` covers the pure
files (`keys`, `marks`, `ghost`, `diff`, the copies, and `package.test.ts`: the manifest, the
chords, the version, the publish matrix). `npm run test:e2e` (`test/e2e/run.ts`) makes a git
project with `cart.py`, its config naming `apprentice-fake-claude` as the `claude` runner,
trusts it in a config directory of the run's own, and launches VS Code with the real `apprentice
lsp`. `test/e2e/apprentice.test.ts` writes the fake's script (`FAKE_APPRENTICE_SCRIPT`) before
each case and answers the notifications with buttons from a queue: a stub typed unsaved is
offered by rule, `ctrl+m a` runs it, the first `write_file` is refused (unsaved and unseen) and
the second lands in the buffer, not on disk, with `⟳ apprentice: writing…`; the ghost, the
note beside, the agent's mark, the messages, a closed permission question denied and an
answered one allowed (from the fake's log, `FAKE_APPRENTICE_LOG`), the task's mark, *Show
diff* as a diff tab, *Show journal* and *Undo this task*; then a sleeping task stopped with
`ctrl+m q`. CI's `vscode` job runs it on ubuntu (under `xvfb-run`) and windows.
