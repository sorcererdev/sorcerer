# Apprentice

`crates/apprentice`: a coding agent whose whole interface is the editor. Package
`sorcerer-apprentice` (`publish = false`, `dist = false`: not on crates.io and not in a release
until the subscription question is asked), library `apprentice`, binary `apprentice`, and the
test program `apprentice-fake-claude`. The design is
`apprentice-prototype-design-2026-11-26.md` in the planning notes.

## The shape

```
 editor ──LSP──▶ apprentice lsp (a Product on sorcerer-lsp)          [the next part]
                   │
                   ├─ run_task ── Agent port ──▶ claude -p (stream-json in and out)
                   │                               │ --mcp-config → apprentice mcp ─┐
                   │                               │ --settings   → apprentice hook ┤ loopback
                   └─ TaskRunner ◀── loopback port (127.0.0.1, token) ◀─────────────┘
```

A task is one `claude -p` child. Its helper processes (the MCP server that serves the medium
tools, and the hooks) are the child's children, not the server's, so they reach the server
through a loopback port: `127.0.0.1`, a port the system picks, one JSON line in and one out per
connection, and a token of the server's own in every request. Loopback TCP is one code path on
three systems; pipes and sockets would be two.

## Module map

**Domain** (`src/domain/`), no I/O:

- `config.rs`: `ApprenticeConfig` (`[apprentice]`), `apprentice_config`, `PRODUCT`.
- `task.rs`: `TaskState`, `TaskFile`, `TaskRecord` (one line of `tasks.jsonl`; the last line of
  an id is the task), `TaskRows`, `latest_of_each`, `task_id`.
- `agent_event.rs`: `AgentEvent` (`Started`, `ToolUse { name, input, parent }`, `Text`,
  `Finished`) and `events_of_line`, the stream-json reader: an `assistant` message's `tool_use`
  and `text` blocks (`parent_tool_use_id` names a subagent's), the `system`/`init` line's
  session id, the `result` line's `total_cost_usd`, `num_turns`, `is_error`.
- `status.rs`: `activity_of` (`reading booking.py`, `running make`, `subagent: …`) and
  `status_text`; the status line is derived, never written by the agent.
- `mediums.rs`: the six tools and the permission tool (`tool_definitions`: names, descriptions,
  JSON schemas), `ToolCall` and `tool_call_of`, `Ask`, `AskWay`, `MarkSeverity`, `is_a_medium`.
- `loopback_message.rs`: `LoopbackRequest { token, task, body }` with `RequestBody::Edits |
  Tool | Permission`, `LoopbackReply`, and the three environment variables of the way back.
- `edits_text.rs`: `FileEdit`, `diff_text` (unified, two lines of context),
  `edits_context_text` (the hook's text) and `unsaved_edits_text` (the first message's).
- `permissions.rs`: `CommandPrefixes` (`permissions.json`, `Versioned`), `allows`, `prefix_of`.
- `paths.rs`: `project_relative` (conjure's `lexical_relative`, copied), `is_sorcerers_own`.
- `budget.rs` (`spent_today`, `triage_spent_today`, `day_allows`, `triage_allows`),
  `estimate.rs` (`estimate_usd`: the median after five tasks), `log_text.rs` (`usd`,
  `title_of`, `offer_title`, `done_text`, `task_mark_text`, `task_line`), `prompt.rs`
  (`EDITOR_RULES`, `TaskPlace`, `task_prompt`).
- `scry_notes.rs`, `rules.rs`: scry's findings file and steering file, read by apprentice's own
  readers (copies of conjure's: products never import each other).

**Ports** (`src/ports/`): `agent.rs` (`Agent::start(&TaskRequest) -> Running`;
`Running::next_event(wait)`, `close_input`, `stop`; `TaskRequest`, `WayBack`, `NextEvent`),
`editor.rs` (`Editor`: `notify`, `mark`, `ghost`, `write_file -> Written`, `note`, `ask ->
Asked`, `edits_since_look`, `progress`, `finished`), `task_store.rs`, `journal.rs`,
`permission_store.rs`, `project_context.rs` (the unit at a line, scry's notes near it, the
rules), `clock.rs`.

**Use cases** (`src/usecases/`): `run_task/` (below), `report_cost.rs` (a day a row, by verb),
`show_log.rs`.

**Adapters** (`src/adapters/`): `claude_agent.rs` (`ClaudeAgent`), `loopback.rs` (`Loopback`,
`ask_loopback`), `helper_way.rs` (`HelperWay::from_env`, `ask`), `mcp_server.rs` (`serve_mcp`),
`hooks.rs` (`hook_output`), `task_runner.rs` (`TaskRunner`: the loopback open, the live tasks by
id), `jsonl_task_store.rs`, `markdown_journal.rs`, `json_permissions.rs`, `project_context.rs`
(`FilesContext`), `scry_files.rs`, `steering_files.rs`, `state_files.rs` (`StateFiles`: every
path under `.sorcerer/state/apprentice/`), `disk_editor.rs` (`DiskEditor`, the command line's
editor), `system_clock.rs`, `wiring.rs` (`live_agent`, `task_deps`, `WireAgent`).

**Command line** (`src/cli/`): `run_cli.rs` (`main`, `main_with`, `Wiring`), `arguments.rs`,
`run.rs` (`apprentice FILE:LINE "…"`), `mcp.rs`, `hook.rs`, `cost.rs`, `log.rs`, `doctor.rs`,
`trust.rs`, `console.rs`, `format.rs`.

## A task, end to end

`TaskRunner::open(deps)` opens the loopback port with a handler that finds the live task a
request names and calls `run_task::answer_request`. Then `TaskRunner::run(order)`:

1. **Start** (`start.rs`, `start_task`). `refuse_a_spent_day` (`spent_today` over the ledger
   against `budget_per_day`); the record, `running`, appended to `tasks.jsonl`; the
   instruction journaled; the first message (`task_prompt`: the instruction, where it was
   written, the unit around it from `ProjectContext::unit_at`, scry's open findings near it, the
   rules, the editor's unsaved edits); `Agent::start` with the way back. A start that fails is
   recorded `failed` and said (`TaskRefused::NotStarted`).
2. **Follow** (`follow.rs`, `follow_task`), on the calling thread: every 200 ms it checks the
   task's `Cancel` (a stop) and takes the next event. A session id is recorded; a tool call
   becomes the status line (`activity_of`, `status_text`, `Editor::progress`) and a journal
   line, and a medium's call marks the turn as spoken. `Finished` closes the agent's input;
   when the turn used no medium its final text is posted (`notify` when it is one line of at
   most 200 characters, else `note`).
3. **Helpers' requests**, meanwhile, each on a loopback thread (`answer_request.rs`):
   - `Tool` → `answer_tool_call` (`tool_calls.rs`): the arguments read (`tool_call_of`; a
     missing one is an error result naming it), paths checked (`project_relative`, the fences,
     never `.sorcerer/` or `.git/`), then the editor. `write_file` journals the file's text before
     once (`Journal::keep_before`), counts the file as the task's, and answers `written: …`;
     `Written::MovedOn` answers an error with the file's text now. `ask` sets the task `asking`
     while it waits.
   - `Permission` (and the `permission` tool) → `answer_permission` (`permissions.rs`): the
     writers and `AskUserQuestion` denied with a sentence; a command allowed always passes; else
     the picker. The answer is the permission tool's JSON, `{"behavior":"allow","updatedInput":…}`
     or `{"behavior":"deny","message":…}`.
   - `Edits` → `edits_for_agent` (`feed.rs`): `Editor::edits_since_look` (which moves the look
     to now) as `edits_context_text`, journaled; at `stop` only journaled.
4. **Finish** (`finish.rs`, `finish_task`): the state (`done`, `failed`, `stopped`), the cost
   (the reported one; a stop or an early end is charged the whole `budget_per_task`), the
   record, one ledger errand (product `apprentice`, verb `task`), the done message
   (`done_text`) and `Editor::finished`.

`LiveTask` (`task_deps.rs`) is what the follow loop and the loopback threads share: the record
behind a mutex, whether the turn spoke, the `Cancel`.

## The harness: `claude -p` headless

`ClaudeAgent::args` (`claude_agent.rs`), in order: `-p`, `--output-format stream-json`,
`--input-format stream-json`, `--verbose`, `--model`, `--max-budget-usd`, `--max-turns`,
`--setting-sources ""` (no project settings, hooks or `CLAUDE.md` reach the child; core's trust
gate stays in front), `--settings` (the two hooks, `PostToolUse` and `Stop`, each `"<apprentice>"
hook <event>` with `/` in the path on every system; the deny rules: the five tools and every
fence as `Read(./…)`, which fences Grep and Glob too), `--mcp-config` (one stdio server, `apprentice
mcp`, with the way back in its `env`) with `--strict-mcp-config`, `--allowedTools` (the
mediums, Read, Grep, Glob, Agent, Task, TodoWrite, WebSearch, WebFetch; **not Bash**, which the
permission tool gates: a tool in `--allowedTools` is never asked about), `--permission-prompt-tool
mcp__apprentice__permission`, `--append-system-prompt` (`EDITOR_RULES`). The environment drops
`CLAUDECODE`, sets `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` and the three `APPRENTICE_*` variables. The
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

**To confirm against the real `claude`** (the tests run the fake): hooks given inline through
`--settings` under `-p`; the stdin framing of `--input-format stream-json`; the input the
permission tool receives (`tool_name`, `input`); whether a `-p` run asks the permission tool
for `Bash` when `Bash` is not allowed. Each is a check in `tests/harness.rs` against the fake;
one task by hand against the real one is the user's.

## Tests

`tests/run_task.rs`: the use cases over a `ScriptedAgent` (`tests/support/scripted_agent.rs`)
that reports events and calls back through the **real** loopback port, as its helpers would, and
a `RecordingEditor` (buffers in memory, `person_edits` to type meanwhile, answers from a
queue). `tests/harness.rs`: `ClaudeAgent` against `apprentice-fake-claude`
(`tests/support/fake_claude.rs`), which writes its arguments and first input line to files,
plays a script, really starts `apprentice mcp` from `--mcp-config` and really runs `apprentice
hook` from `--settings`, logging each result. `tests/cli.rs`: `main_with` with a `Wiring` of
doubles. No test calls a model.
