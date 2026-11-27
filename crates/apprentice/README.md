# sorcerer-apprentice

Apprentice is a coding agent whose whole interface is your editor: you ask from a line, it works as a Claude Code task, and its edits land in your buffers.

**It runs Claude Code on your own subscription:** each task is one `claude -p` session on your own login, so it needs [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed and signed in (`claude` on your PATH). Tasks count against your Claude usage, capped per task and per day.

## Install and run

```sh
cargo install sorcerer-apprentice
apprentice doctor
```

Or the release's installers (`sorcerer-apprentice-installer.sh` / `.ps1`); see the [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md#install).

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

A library (`apprentice`), a binary, and `apprentice-fake-claude`, a stand-in agent for tests (built only with the `fake-claude` feature, so `cargo install` leaves it out). The agent's helper processes reach the server over a loopback port with a token. Builds on `sorcerer-core`, `sorcerer-models` and `sorcerer-lsp`.

Book: [Apprentice](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/apprentice.md) (using it), [Apprentice internals](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/apprentice.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
