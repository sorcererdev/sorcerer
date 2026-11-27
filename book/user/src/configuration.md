# Configuration

Everything is optional. With no `.sorcerer/config.toml`, the tools use their defaults
(see [What sorcerer is](what-sorcerer-is.md)). Each tool reads its own table (`[scry]`,
`[conjure]`, `[spellbook]`, `[lackey]`) and refuses keys it does not know; a top-level
table no tool knows is reported as a warning, since it is probably a typo.

## `[conjure]`

| key | meaning |
|---|---|
| `writer` | the model alias that writes the first attempt: `haiku` by default |
| `try_harder` | the stronger model Try harder asks: `sonnet` by default; `""` turns Try harder off |
| `budget_per_day` | the most conjure may spend in a local day, in dollars: 3.00 by default |
| `exclude` | globs of files conjure never writes, such as `gen/**` |

See [Conjure](conjure.md).

## `[spellbook]`

| key | meaning |
|---|---|
| `enabled` | whether spellbook runs in this project: `true` by default |
| `exclude` | globs of files no rule looks at, such as `gen/**`: `spellbook check` and the editor's findings skip them. It is the rules', not generation's: a generator ignores it, since it writes only what you ask for by name |
| `sync_on_save` | whether the editor's server refreshes stale owned regions when a file is saved: `true` by default. The project's setting, not the editor's, since the team decides whether its lists are kept by hand. Off, a stale block is left for its `region-stale` finding and its quick fix ([Spellbook](spellbook.md#in-the-editor)) |

`[spellbook.rules]` is a table of rule ids, each `"hint"`, `"warn"`, `"error"` or `"off"`: how strict
the project is with that rule, over the rule's own `severity` (`off` does not run it). A setting for
no rule stops the check, naming it. See [Adopting check on code with
debt](spellbook.md#adopting-check-on-code-with-debt).

```toml
[spellbook.rules]
commands-use-ports = "warn"
```

See [Spellbook](spellbook.md).

## `[lackey]`

`model`, `budget_per_task`, `budget_per_day`, `triage_per_day`, `max_turns`,
`ask_timeout_seconds`, `estimate_start`: see [Lackey](lackey.md#usage-and-the-caps).

## `[model]`

| key | meaning |
|---|---|
| `provider` | the provider to use: `claude`, `anthropic`, or the name of a `[model.providers.<name>]` table |
| `not_evidence` | paths a model may never read, in addition to the ones that are always fenced (below). A glob (`docs/**/*.draft.md`) or a directory (`docs/plans`), which fences everything under it |

## `[model.providers.<name>]`

| key | meaning |
|---|---|
| `kind` | `anthropic`, `claude` or `openai-compatible`; defaults to the table's name |
| `url` | the server's address (`openai-compatible` needs one; `anthropic` defaults to Anthropic's API) |
| `runner` | for `claude`: the command to run instead of `claude` on PATH |
| `aliases` | which model each name means (`{ sonnet = "qwen3:32b" }`) |
| `prices` | dollars per million input and output tokens by name (`{ sonnet = [2.0, 10.0] }`), so spend is counted |

## `[decide]`

| key | meaning |
|---|---|
| `model` | the Jev model; pinned by default |

Keys come only from fixed environment variables: `ANTHROPIC_API_KEY` for `anthropic`,
`OPENAI_API_KEY` for `openai-compatible` (optional; without it no authorization header is
sent) and `JEV_API_KEY` for Jev. A project's config can never point scry at another secret,
so there is no setting to name a different variable.

## `[languages]`

| key | meaning |
|---|---|
| `names` | languages the project uses beyond the shipped ones; each needs a file under `.sorcerer/languages/<name>/` (see [Adding a language](adding-a-language.md)) |

## What a model never reads

Some paths are fenced: no model is shown them, and no citation may point at them.

- `.env` and anything starting with `.env`, in any directory;
- `.sorcerer/` (what the tools remember, including other findings and costs) and `.git/`;
  the one exception is `.sorcerer/notes/`, your notes and lackey's, which are read like
  any file (a link in it onto a fenced path stays fenced, and so does a `.env` in it);
- anything outside the project, including through a symbolic link;
- your `not_evidence` paths.

When the model runs through sorcerer's own tools (Anthropic's API, an OpenAI-compatible
server), every path is checked before it is read. When it runs through `claude -p`,
Claude Code enforces the fences with deny rules, including one for each symbolic link
that leads out of the project or onto a fenced path. The links are looked for in the whole
project, ignored folders included (`.gitignore` and `.ignore` do not count: git checks out an
ignored link that was added with `git add -f`, and a link made in an ignored folder is still
there to read through). Only `.git/` and `.sorcerer/` (but its notes) are skipped, both fenced
whole. The first task in a large project walks it all once; later tasks walk only the folders
that changed. This holds in a project that is not a git repository too. A hard link to a secret is not a symbolic link and
is not found either: keep secrets out of the project. Claude Code cannot be told to refuse every path outside the
project, though: there, only the instructions keep it inside. Keep secrets out of the
project's files, or under a fenced path, either way.
