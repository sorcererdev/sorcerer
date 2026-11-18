# What sorcerer is

Sorcerer is a suite of small tools that read your code alongside you.

| tool | what it does |
|---|---|
| **scry** | reads what you just changed, researches it against the codebase and your rules, and puts the few things worth acting on in the margin |
| **conjure** | writes one reviewed step at the cursor |
| **spellbook** | conventions: generators, owned regions, rules and checks |

Each tool is its own binary and its own language server. They never call each other: they
share one core library and compose through files in your project's `.sorcerer/`
directory.

```
.sorcerer/
  config.toml      one config file for every tool (committed)
  scry.md          scry's steering: your rules (committed)
  languages/       your project's language files (committed)
  state/           what the tools remember (not committed)
```

You do not need a `.sorcerer/` directory to start. In a git repository with none, the
tools use their defaults.
