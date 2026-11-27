# sorcerer-lsp

The language-server host for Sorcerer's programs: the connection, delayed jobs, and the senders a product's server uses. Everything about being a server that is not about one product.

## Who uses it

`scry lsp`, `conjure lsp`, `spellbook lsp` and `lackey lsp`. No async runtime: it is synchronous, over `lsp-server`.

## Main parts

- `serve.rs`: the `Product` trait a product implements, and `serve`, the handshake and main loop.
- `client.rs`: what a product says to the editor (progress, questions, edits).
- `jobs.rs`: work to run after a delay. `reply.rs`: answer a request later.
- `landing.rs`, `undo_plan.rs`, `line_edits.rs`: land a change as the fewest line edits, and take it back.

## Example

```rust
struct MyServer;
impl sorcerer_lsp::serve::Product for MyServer { /* handle requests */ }
// sorcerer_lsp::serve::serve(connection, MyServer, ...)
```

See `crates/scry/src/adapters/lsp/` for a full one.

Book: [Using Sorcerer in an editor](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/editor.md) (using it), [The language-server host](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/lsp.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
