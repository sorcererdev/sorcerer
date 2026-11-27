# The language-server host

`crates/sorcerer-lsp` is everything about being a language server that is not about one
product. Scry's server (`scry lsp`) is the first product on it; conjure's server is next, and
uses what the host added for it (the open documents' text, `apply_edit`); spellbook's server (`spellbook lsp`) plugs
in the same way (`tracks_text` false, execute commands, code actions, `publish`, `show_document`,
`apply_edit`), and added one thing: a request answered later, through a `Reply`, so `spellbook.apply`
can wait for the editor to land a fix. A save is a notification with nothing to answer, yet
spellbook's `Product::saved` may still send `workspace/applyEdit` (its owned regions synced on save):
`Client::apply_edit` takes a callback and nothing waits, `Client::is_open` tells it whether a
region file is the editor's to change or the disk's, and `Client::text_of` (spellbook tracks text
for this) gives the buffer's text and the version the edit is tied to (`crates/spellbook/src/adapters/lsp/sync_on_save.rs`).
apprentice's server (`apprentice lsp`) tracks text, lands each of a task's writes through the
host's landing code, and was the reason that code moved here from conjure and `notify_sorcerer`
was added. No scry type appears in the crate.

It is synchronous like the rest of the workspace: `lsp-server` gives a connection as two
channels, and `lsp-types` the message types (its `Uri`, which `uri.rs` turns into paths
through the `url` crate). There is no async runtime.

## The files

| file | holds |
|---|---|
| `src/serve.rs` | `Product`, the trait a product implements; `serve`, the handshake and the main loop; `unknown_method` |
| `src/client.rs` | `Client`, what a product says to the editor; `WorkDone`, a progress line; `Answer`, the reply to a question; `Applied`, the reply to an edit |
| `src/jobs.rs` | `Jobs`, work to run after a delay, one waiting job per key |
| `src/reply.rs` | `Reply`, the answer to one request, which a product may send after the call that got it returned |
| `src/uri.rs` | `path_of` and `uri_of`: only `file:` URIs are files |
| `src/line_edits.rs` | `LineEdit` and `line_edits` (a new text as the fewest whole-line edits, over the `similar` crate), `same_text`, `in_base_line_endings`, `keep_unterminated_last_line`, `first_changed_line`, `lines_changed` |
| `src/landing.rs` | landing a change: `Snapshot` and `OpenDocument` (the open documents when it was asked for), `FileChange`, `NewFiles`, `workspace_edit`, `moved_on`, `versioned_text_now`, `Undoing` and `undo_edit` |
| `src/undo_plan.rs` | taking one landed file back: `TakenBack`, `FileDeletion`, `Undo`, `FileUndo`, `undo_of` |
| `src/tool_edits.rs` | the lines Sorcerer's servers land, noted for a server that watches the person's typing: `note_tool_edit`, `lines_tools_landed`, `NOTE_FILE` (`tool-edits.jsonl` in the state directory), `NOTED_FOR` (ten minutes) |

## What the host owns

- **The handshake** (`serve::handshake`). It reads the editor's abilities (`serve::abilities_of`:
  `window.workDoneProgress`, `textDocument.publishDiagnostics.relatedInformation`,
  `workspace.applyEdit`, `workspace.workspaceEdit.documentChanges` and whether
  `workspace.workspaceEdit.resourceOperations` holds `create` and `delete`; an editor that
  announces any resource operation is taken to accept `documentChanges`, since resource
  operations exist only there and Neovim 0.12 announces them and not `documentChanges`) and the workspace
  root (the first workspace folder, else `rootUri`), and announces the product's capabilities
  with the text sync replaced by its own (`serve::open_close_and_save`): open and close, save
  without the text, and no `change`, so nothing reaches a product while the person types. A
  product whose `Product::tracks_text` says yes gets `change: Full` instead.
- **The open documents' text, for a product that tracks it.** `didOpen` keeps the document's
  version and text in the client's state, each `didChange` replaces them with the new version
  and the whole text (the last content change, under full sync), and `didClose` drops them.
  `Client::text_of(path)` reads them; it is `None` for a file not open, and always for a product
  that does not track text. The host keeps the text before `Product::changed` runs, so the
  product sees it already. A change notification never reaches a product that does not track
  text.
- **The open set.** `didOpen` and `didClose` update it before the product hears of them, so
  `Client::is_open` and `Client::open_paths` already agree when `Product::opened` runs.
  Notifications about documents that are not files (`untitled:`, `fugitive://`) are dropped.
- **Requests.** Each goes to `Product::request_later(method, params, reply)`, whose default
  answers at once with `Product::request` by method name; a product answers a method it
  does not serve with `unknown_method` (`-32601`). A product that must wait for the editor
  before it can answer (spellbook's apply, which waits for `workspace/applyEdit`'s reply)
  overrides `request_later`, keeps the `Reply` (it is `Send`) and calls `Reply::send` from the
  callback; the main loop goes on meanwhile. A `Reply` is sent once; one dropped unanswered
  answers with an error (`-32603`), so the editor never waits forever. `shutdown` calls `Product::shutdown` and is
  answered with null; `exit` ends `serve`. So does the editor hanging up (stdin EOF): `serve` then calls
  `Product::shutdown` itself, so a running pass stops. Either way `Client::forget_waiting` drops
  every callback still waiting for a reply, because a callback that holds a `Client` keeps the
  connection's sender alive and so the process with it.
- **Replies.** The server's own requests (`window/showMessageRequest`,
  `window/workDoneProgress/create`, `window/showDocument`) are numbered by the `Client`, which
  keeps what to do with each reply; the main loop hands every reply over by id
  (`Client::replied`). A reply never received leaves its handler waiting, and nothing blocks
  on it. `workspace/applyEdit` goes the same way.

- **Landing an edit**, for a product that writes (conjure, apprentice). `landing::workspace_edit`
  turns a change's files (`FileChange`: the path relative to the root, new or not, the text
  before and after, the `LineEdit`s) into one `WorkspaceEdit`: each open file a
  `TextDocumentEdit` at the version in the `Snapshot` taken when the change was asked for, a file
  not open with no version, a new file a `CreateFile` (`ignoreIfExists`) and its text written
  over lines 0 up to 1 (not inserted at 0:0: a new Neovim buffer already holds one empty line,
  which an insertion would leave as an extra last line; an edit past the end is one Neovim and
  VS Code both fit to the buffer, and Neovim then drops the empty last line), or left
  out under `NewFiles::LeftOut`. `landing::moved_on` is the check before sending: an open file at
  another version, a file not open then but open now (or on disk) with other text, a new file
  that is there now. `undo_plan::undo_of` plans one file's undo from its text now
  (`versioned_text_now`: the editor's, else the disk's): back to its text before when it is still
  the after-text, nothing when it is already back, refused (`FileUndo::Edited`) when the person
  edited it; `landing::undo_edit` sends the undoings as one edit. Texts are compared with
  `line_edits::same_text`, which ignores line endings and one final newline: an editor may keep
  CRLF lines, and may end every buffer with a newline (Neovim's `eol`) where the change ended
  without one, and neither is the person's edit. The products keep what is
  theirs: which files a change touches, its record, its marks.

- **Telling tools' edits from the person's.** The editor reports every change alike
  (`didChange` with the whole text). So every product edit goes out through
  `Client::apply_tool_edit(state_dir, …)`, which first appends, per edited file, the trimmed
  lines its text edits put there to `.sorcerer/state/tool-edits.jsonl`
  (`tool_edits::note_tool_edit`; written before the request, so the note is there when the
  change is reported). apprentice reads `lines_tools_landed(state_dir, file, now)` (the notes
  of the last ten minutes) at each look and leaves those lines out of triage. The note is
  pruned to its last ten minutes once it holds more than 200 lines; a note that cannot be
  written is skipped and the edit still goes.

## The client's senders

`Client` is `Clone + Send + Sync`; every clone speaks over the same connection, from any
thread.

- `publish(path, diagnostics)`: `textDocument/publishDiagnostics`; an empty list clears.
- `work_done(title, message) -> WorkDone`, then `report(message)` and `end(message)`. It sends
  `window/workDoneProgress/create` and holds the begin (and anything after it) until the
  editor answers, as the protocol asks; a refused create drops them. When the editor did not
  announce `window.workDoneProgress`, every call does nothing.
- `show_working(text, places)` and `clear_working()`: the custom notification
  `sorcerer/working`, `{ "units": [{ "uri", "line", "text" }] }` with `line` from 0 (a place is a
  file and the unit's first line from 1), each call replacing the last; the editors draw `text`
  after that line. It is sent only to an editor whose `initializationOptions` hold
  `drawsWorking: true` (`Abilities::draws_working`, read in `serve::abilities_of`); to any other
  both calls do nothing. `clear_working` sends the empty list only when something is shown. The
  clients clear it themselves when the server exits.
- `notify_sorcerer(method, params) -> bool`: a `sorcerer/…` notification of the product's own
  (apprentice's `sorcerer/ghost`, `sorcerer/note`, `sorcerer/showDiff`), sent only to an editor
  whose `initializationOptions.handles` lists the method (`Abilities::handles`, read in
  `serve::abilities_of`); it answers whether it was sent, so the product falls back to a
  standard message (`say`, `show_document`) for any other editor. `handles(method)` asks the
  same without sending.
- `say(kind, text)`: `window/showMessage`, every time (an answer to what the person just did).
- `say_once(key, kind, text)`: `window/showMessage`, at most once per key for the life of the
  server; answers whether it was sent.
- `log(text)`: `window/logMessage`, the editor's log for the server, never shown unasked.
- `show_document(path, selection)`: `window/showDocument` with `takeFocus`.
- `ask(text, answers, on_answer)`: `window/showMessageRequest`. `on_answer` gets
  `Answer::Picked(title)`, `Answer::Closed` for a null reply, or `Answer::Unsupported` for an
  error reply, on the main loop when the reply arrives.
- `apply_edit(label, edit, on_reply)`: `workspace/applyEdit` with the label and the
  `WorkspaceEdit`. `on_reply` gets `Applied { applied, failure_reason }` on the main loop when
  the reply arrives; an error reply is `applied: false` with the error's message as the reason.
- `apply_tool_edit(state_dir, label, edit, on_reply)`: the same, with the edit's lines noted
  first in the project's state directory (none when the project did not open). Products use
  this one; spellbook's `send_edit`, conjure's `land` and undo, apprentice's `write_for_task`,
  undo and *Keep this answer* each pass their project's `state_dir`.

What the editor can do, read at the handshake: `related_information()`; `can_apply_edits()`
(it announced both `workspace.applyEdit` and `workspaceEdit.documentChanges`, without which a
product cannot land an edit tied to a document's version); `can_create_files()` and
`can_delete_files()` (its `resourceOperations` hold `create`, `delete`).

## Threads

The main loop runs on the thread that called `serve`. It never waits on a product's work:
each `Product` method must return at once. Slow work runs elsewhere:

- `Jobs::submit(key, delay, job)` runs `job` on a thread of its own after `delay`, unless a
  later submit under the same key or `Jobs::cancel(key)` comes first. A resubmit replaces the
  waiting job and starts the delay again, which is how a burst of saves becomes one pass.
  Each submit takes a ticket; the job runs only if its ticket is still the live one for its key
  when the delay ends. Dropping `Jobs` drops what is waiting.
- A product may start its own threads (scry's pass worker); they speak through clones of the
  `Client`.

## Plugging a product in

```rust
struct Mine { client: Client }

impl Product for Mine {
    fn capabilities() -> ServerCapabilities { ServerCapabilities::default() }
    fn opened(&mut self, path: &Path) { /* publish what you know about it */ }
    fn closed(&mut self, path: &Path) { self.client.publish(path, Vec::new()) }
    fn saved(&mut self, path: &Path) { /* submit a job */ }
    fn request(&mut self, method: &str, _: Value) -> Result<Value, ResponseError> {
        Err(unknown_method(method))
    }
    fn shutdown(&mut self) {}
}

serve(Connection::stdio().0, |client, _root| Mine { client })
```

Three methods have defaults a product may leave alone: `tracks_text()` (false; read once at the
handshake, like `capabilities`), `changed(path, version, text)` (nothing; called after each
change of an open document, only when `tracks_text` is true) and `request_later(method, params,
reply)` (answers at once with `request`). A product that lands edits asks
`can_apply_edits`, `can_create_files` and `can_delete_files` before it offers to.

Conjure (`crates/conjure/src/adapters/lsp/`, [Conjure](conjure.md#the-server)) is the product that
tracks text and edits: it reads `text_of` for every open document when a change is asked for,
sends one `apply_edit` per change (and per undo) with each open file's version, checks
`text_of` (and the disk) again before it sends, and after the editor answers `applied` checks that
each file reads as sent. Scry tracks no text and never edits. Two things the editors do
that a product must allow for: Neovim answers `workspace/applyEdit` before it sends the
`didChange` for it (its changes are debounced), and checks the version of only the first
document in an edit.

The tests (`tests/serve.rs`, `tests/jobs.rs`) drive the host over `Connection::memory()` with
a product that records what reaches it.
