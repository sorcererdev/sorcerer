# What sorcerer is

Sorcerer is a suite of small tools that read your code alongside you.

| tool | what it does |
|---|---|
| **scry** | reads what you just changed, researches it against the codebase and your rules, and puts the few things worth acting on in the margin |
| **conjure** | writes one small change where you ask |
| **spellbook** | conventions as data: generators that write a convention's files with holes to fill (now); rules and `spellbook check` (now); owned regions, fixes and packs (later) |
| **apprentice** | a coding agent whose whole interface is your editor: no chat, it answers and works where you write (a prototype, built from source) |

Each tool is its own binary and its own language server. They never call each other: they
share one core library and compose through files in your project's `.sorcerer/`
directory.

```
.sorcerer/
  config.toml      one config file for every tool (committed)
  scry.toml        scry's steering: context, includes and rules (committed)
  languages/       your project's language files (committed)
  state/           what the tools remember (not committed)
```

You do not need a `.sorcerer/` directory to start. In a git repository with none, the
tools use their defaults.

## Two keys, no config

Out of the box, sorcerer needs two things:

- **A model.** If the `claude` command (Claude Code) is on your PATH, sorcerer uses it,
  on your subscription. Otherwise it uses Anthropic's API when `ANTHROPIC_API_KEY` is set.
  `[model] provider = "…"` in `.sorcerer/config.toml` overrides the choice.
- **Jev.** `JEV_API_KEY`, from a free sign-up at typesafe.ai. Jev answers the yes-or-no
  questions that keep the margin quiet (is this a nitpick, are these two findings the
  same) with calibrated probabilities, for a fraction of a cent.

Each tool's `doctor` command prints what it chose and why.

## Other providers

Any OpenAI-compatible server works as a provider. For a local Ollama:

```toml
# .sorcerer/config.toml
[model]
provider = "local"

[model.providers.local]
kind = "openai-compatible"
url = "http://localhost:11434/v1"
# which model each name the tools ask for means here
aliases = { sonnet = "qwen3:32b", haiku = "qwen3:8b" }
```

Set `OPENAI_API_KEY` when the server needs a key, and add `prices = { sonnet = [0.5,
1.5] }` (dollars per million input and output tokens) if you want its spend counted.
