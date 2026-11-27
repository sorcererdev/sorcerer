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
- `cutting.rs`: `Shape`, `ShapeUnit`, `changed_lines` (over `similar`), `regions_of` (comment
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
`system_clock.rs`, `wiring.rs` (`live_agent`, `task_deps`, `WireAgent`), and `lsp/` (the
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
   way back. An instruction in a fenced file is refused (`TaskRefused::Fenced`). A start that
   fails is recorded `failed` and said (`TaskRefused::NotStarted`).
2. **Follow** (`follow.rs`, `follow_task`), on the calling thread: every 200 ms it checks the
   task's `Cancel` (a stop) and takes the next event. A session id is recorded; a tool call
   becomes the status line (`activity_of`, `status_text`, `Editor::progress`) and a journal
   line, and a medium's call marks the turn as spoken. `Finished` closes the agent's input;
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
     or `{"behavior":"deny","message":…}`.
   - `Edits` → `edits_for_agent` (`feed.rs`): `Editor::edits_since_look` (which moves the look
     to now) as `edits_context_text`, journaled; at `stop` only journaled.
4. **Finish** (`finish.rs`, `finish_task`): the state (`done`, `failed`, `stopped`), the cost
   (the reported one; a stop, an early end or a result with no `total_cost_usd` is charged
   the whole `budget_per_task`), the
   record, one ledger errand (product `apprentice`, verb `task`), the done message
   (`done_text`) and `Editor::finished`.

`LiveTask` (`task_deps.rs`) is what the follow loop and the loopback threads share: the record
behind a mutex, whether the turn spoke, the `Cancel`.

## The harness: `claude -p` headless

`ClaudeAgent::args` (`claude_agent.rs`), in order: `-p`, `--output-format stream-json`,
`--input-format stream-json`, `--verbose`, `--model`, `--max-budget-usd`, `--max-turns`,
`--setting-sources ""` (no project settings, hooks or `CLAUDE.md` reach the child; core's trust
gate stays in front), `--settings` (the two hooks, `PostToolUse` and `Stop`, each `"<apprentice>"
hook <event>` with `/` in the path on every system; then `permissions`: `deny` holds the five
tools and every fence as `Read(./…)` (a `dir/**` fence also as `Read(./dir)`, each rule once),
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

**The two clocks** (`watch.rs`). `changed` submits `look_at` under `idle:<path>`; `saved` cancels
it, then runs `look_at` and `answer_on_save` under `save:<path>`. `look_at` takes the text now,
swaps it for the last look, and (unless `head_moved`, which resets every look) runs `triage`
(`regions.rs`, building `TriageDeps` over `GrammarSyntax`, `JsonRegionStore`, the ledger and the
decider) on the change, then `publish`. A task's own `write_file` moves the look
(`looked_at_own_edit`), so its edits are never triaged as the person's. `answer_on_save` builds
`AnswerDeps` (the researcher over core's `FilesView` of the disk, which the save just wrote) and
writes a long answer as a note too.

**The marks** (`publisher.rs`): one publish per file joins the regions' marks
(`region_mark`: an answer or a verdict Information with its cost and time and the citations as
`relatedInformation`, else in the message; an instruction a Hint with the offer's price), the
finished tasks' (`task_marks`, code `task`, `data.task`), the agent's (`said_marks`) and the
questions' (`ask_marks`). Ranges end at the end of the last line (`lines_of`, UTF-16).

**The actions** (`commands.rs`): `code_actions` gives, per line, each found region's actions
(`region_actions`: the offer while no task runs, *Show code*, *Keep this answer*, *Dismiss*, the
four labels), the pending questions' options (`ask_actions`), a marked task's *Show diff*,
*Undo this task*, *Show journal*, and *Stop this task* while one runs. `execute` routes the ten
`apprentice.*` commands; the slow ones (*do this*, *Dismiss*, a label) go to a thread.

**A task from the editor** (`tasks.rs`). `do_region` makes a `TaskOrder` (the region's text, its
place now, the open files' unsaved edits) and `start_task`, holding `Server::runner` from the
check to the start (so two *do this* at once start one task), refuses a second task, builds a
`TaskRunner` over `task_deps` with an `LspEditor` (`editor_port.rs`, holding the server weakly),
starts it, gives it its first look (every open file's text now, `open_texts`) and follows it on a
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
  answers first). The task's look and the server's look move to the new text, the text is kept
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
