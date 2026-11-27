# sorcerer-lackey

Lackey is a coding agent whose whole interface is your editor: you ask from a line, it works as a Claude Code task, and its edits land in your buffers. A prototype; not published to crates.io or in a release yet.

## Build and run

```sh
cargo run -p sorcerer-lackey --bin lackey -- doctor
```

Needs `claude` on your PATH, signed in.

Main commands:

- `lackey FILE:LINE "instruction"`: start a task.
- `lackey lsp`: the editor's server, where tasks run and show.
- `lackey log`, `cost`, `doctor`, `trust`: past tasks, spend, a health check, and permission for the editor to run it.
- `lackey mcp` and `lackey hook`: started by the agent itself, not by you.

## Example

```sh
lackey src/cart.py:12 "round the total to cents"
```

## Inside

A library (`lackey`), a binary, and `lackey-fake-claude`, a stand-in agent for tests. The agent's helper processes reach the server over a loopback port with a token. Builds on `sorcerer-core`, `sorcerer-models` and `sorcerer-lsp`.

Book: [Lackey](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/lackey.md) (using it), [Lackey internals](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/lackey.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
