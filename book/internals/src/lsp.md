# The language-server host

`crates/sorcerer-lsp` is everything about being a language server that is not about one
product. Scry's server (`scry lsp`) is the first product on it; conjure's and spellbook's will
plug in the same way. No scry type appears in the crate.

It is synchronous like the rest of the workspace: `lsp-server` gives a connection as two
channels, and `lsp-types` the message types (its `Uri`, which `uri.rs` turns into paths
through the `url` crate). There is no async runtime.

## The four files

| file | holds |
|---|---|
| `src/serve.rs` | `Product`, the trait a product implements; `serve`, the handshake and the main loop; `unknown_method` |
| `src/client.rs` | `Client`, what a product says to the editor; `WorkDone`, a progress line; `Answer`, the reply to a question |
| `src/jobs.rs` | `Jobs`, work to run after a delay, one waiting job per key |
| `src/uri.rs` | `path_of` and `uri_of`: only `file:` URIs are files |

## What the host owns

- **The handshake** (`serve::handshake`). It reads two of the editor's abilities
  (`window.workDoneProgress` and `textDocument.publishDiagnostics.relatedInformation`) and the
  workspace root (the first workspace folder, else `rootUri`), and announces the product's
  capabilities with the text sync replaced by its own: open and close, save without the text,
  and no `change`. Nothing reaches a product while the person types.
- **The open set.** `didOpen` and `didClose` update it before the product hears of them, so
  `Client::is_open` and `Client::open_paths` already agree when `Product::opened` runs.
  Notifications about documents that are not files (`untitled:`, `fugitive://`) are dropped.
- **Requests.** Each goes to `Product::request` by method name; a product answers a method it
  does not serve with `unknown_method` (`-32601`). `shutdown` calls `Product::shutdown` and is
  answered with null; `exit` ends `serve`. So does the editor hanging up (stdin EOF): `serve` then calls
  `Product::shutdown` itself, so a running pass stops. Either way `Client::forget_waiting` drops
  every callback still waiting for a reply, because a callback that holds a `Client` keeps the
  connection's sender alive and so the process with it.
- **Replies.** The server's own requests (`window/showMessageRequest`,
  `window/workDoneProgress/create`, `window/showDocument`) are numbered by the `Client`, which
  keeps what to do with each reply; the main loop hands every reply over by id
  (`Client::replied`). A reply never received leaves its handler waiting, and nothing blocks
  on it.

## The client's senders

`Client` is `Clone + Send + Sync`; every clone speaks over the same connection, from any
thread.

- `publish(path, diagnostics)`: `textDocument/publishDiagnostics`; an empty list clears.
- `work_done(title, message) -> WorkDone`, then `report(message)` and `end(message)`. It sends
  `window/workDoneProgress/create` and holds the begin (and anything after it) until the
  editor answers, as the protocol asks; a refused create drops them. When the editor did not
  announce `window.workDoneProgress`, every call does nothing.
- `say_once(key, kind, text)`: `window/showMessage`, at most once per key for the life of the
  server; answers whether it was sent.
- `log(text)`: `window/logMessage`, the editor's log for the server, never shown unasked.
- `show_document(path, selection)`: `window/showDocument` with `takeFocus`.
- `ask(text, answers, on_answer)`: `window/showMessageRequest`. `on_answer` gets
  `Answer::Picked(title)`, `Answer::Closed` for a null reply, or `Answer::Unsupported` for an
  error reply, on the main loop when the reply arrives.

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

The tests (`tests/serve.rs`, `tests/jobs.rs`) drive the host over `Connection::memory()` with
a product that records what reaches it.
