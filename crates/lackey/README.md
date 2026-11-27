# sorcerer-lackey

Lackey is a coding agent whose whole interface is your editor: you ask from a line, it works as a Claude Code task, and its edits land in your buffers.

**It runs Claude Code on your own subscription:** each task is one `claude -p` session on your own login, so it needs [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed and signed in (run `claude` once and log in). Tasks count against your Claude usage, capped per task and per day.

**It reads what you write with Jev** (free sign-up at [typesafe.ai](https://typesafe.ai)): set `JEV_API_KEY` in your shell profile. Without it only empty function bodies are offered. The whole first-time path, with what to write to get an offer, is [Getting started](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/lackey.md#getting-started).

## Install and run

```sh
cargo install sorcerer-lackey
lackey doctor
```

Or the release's installers (`sorcerer-lackey-installer.sh` / `.ps1`); see the [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md#install).

Main commands:

- `lackey FILE:LINE "instruction"`: start a task from the terminal; its changes are written to the disk.
- `lackey lsp`: the editor's server, where tasks run and show.
- `lackey log`, `cost`, `doctor`, `trust`: past tasks, spend, a health check (`ok`, `warn` or `FAIL` per row), and trust for a project whose own config runs a program (`lackey doctor` says whether it is needed).
- `lackey mcp` and `lackey hook`: started by the agent itself, not by you.

## Example

```sh
lackey src/cart.py:12 "round the total to cents"
```

## Inside

A library (`lackey`), a binary, and `lackey-fake-claude`, a stand-in agent for tests (built only with the `fake-claude` feature, so `cargo install` leaves it out). The agent's helper processes reach the server over a loopback port with a token. Builds on `sorcerer-core`, `sorcerer-models` and `sorcerer-lsp`.

Book: [Lackey](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/lackey.md) (using it), [Lackey internals](https://github.com/sorcererdev/sorcerer/blob/main/book/internals/src/lackey.md) (how it works). Project: [root README](https://github.com/sorcererdev/sorcerer/blob/main/README.md).
