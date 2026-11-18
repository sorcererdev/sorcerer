# The core

`crates/sorcerer-core`: one module a concern.

## Config

`config.rs`. One file, `.sorcerer/config.toml`, parsed once by `Config::parse`. Core owns
three tables and reads them strictly (an unknown key is an error naming it):

- `[model]` (`ModelConfig`): the provider to use, the user's `not_evidence` globs, and one
  `[model.providers.<name>]` table per provider (`ProviderConfig`: kind, url, key variable,
  runner, aliases, prices).
- `[decide]` (`DecideConfig`): the decider's model and key variable.
- `[languages]`: `names`, languages the project uses beyond the shipped ones.

Every other table stays in `Config::tables` for the product that owns it. A product reads
its table with `config.table::<ScryConfig>("scry")`; its struct carries
`#[serde(deny_unknown_fields)]`, so a typo inside `[scry]` is an `Error::UnknownKey`. A
top-level table no tool knows (`KNOWN_TABLES`) is probably a typo too, but another tool
might own it, so it is a warning in `Config::warnings`, not an error.

The environment is a seam (`env.rs`): `Env` answers variables and executables on PATH.
`SystemEnv` is the real one, `MapEnv` a fixed one for tests. `Cancel` (`cancel.rs`) is
the flag every long call checks.

## Project

`project.rs`. `Project::open(dir)` walks up from `dir`. The first directory holding
`.sorcerer/` is the root, and its `config.toml` is read if present. With no `.sorcerer/`
anywhere above, the git root is the project and the config is `Config::default()`: that is
the out-of-the-box case, and every product must work in it. With neither, the error says
so in one sentence.

`Project::state_dir()` creates `.sorcerer/state/` on first use and writes a `.gitignore`
of `*` into it, so nothing a tool remembers is ever committed.

`default_provider` and `default_decider` choose what runs a model when the config does not
say, and why, for `doctor` to print: `claude` when it is on PATH, else `anthropic` when
`ANTHROPIC_API_KEY` is set; Jev when `JEV_API_KEY` is set.
