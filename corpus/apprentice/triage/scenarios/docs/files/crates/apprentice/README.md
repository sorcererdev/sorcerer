# sorcerer-apprentice

Apprentice is a coding agent whose whole interface is your editor: you ask from a line, it works as a Claude Code task, and its edits land in your buffers. A prototype; not published to crates.io or in a release yet.

## Build and run

```sh
cargo run -p sorcerer-apprentice --bin apprentice -- doctor
```

Needs `claude` on your PATH, signed in.

Main commands:

- `apprentice FILE:LINE "instruction"`: start a task.
- `apprentice lsp`: the editor's server, where tasks run and show.
- `apprentice log`, `cost`, `doctor`, `trust`: past tasks, spend, a health check, and permission for the editor to run it.
- `apprentice mcp` and `apprentice hook`: started by the agent itself, not by you.

## Example

```sh
apprentice src/cart.py:12 "round the total to cents"
```

## Inside

A library (`apprentice`), a binary, and `apprentice-fake-claude`, a stand-in agent for tests. The agent's helper processes reach the server over a loopback port with a token. Builds on `sorcerer-core`, `sorcerer-models` and `sorcerer-lsp`.

Book: [Apprentice](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/apprentice.md) (using it), [Apprentice internals](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/apprentice.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
