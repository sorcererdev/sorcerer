# Sorcerer

Small tools that read your code alongside you. **scry** reviews what you changed and puts each
finding on its line. **conjure** writes one small change where you ask, and marks it so you can
undo it. **spellbook** (conventions as data) is in its first milestone: generators write every file of a
convention, with holes that say what belongs there, and rules check that the conventions hold. They share one core and talk through
files in `.sorcerer/`.

**Status:** scry and conjure; spellbook's first milestone in the repository, not yet released; Python and Rust; Windows, macOS and Linux; early (0.1).

## Install

Each tool is one program. Install the ones you want.

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.sh | sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-conjure-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.ps1 | iex"
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-conjure-installer.ps1 | iex"
```

Or with Rust: `cargo install sorcerer-scry sorcerer-conjure`.

The installers put `scry` and `conjure` in `~/.local/bin` (make sure that folder is on your
PATH) and need neither Rust nor a C compiler: the language grammars come prebuilt with the
release.

## Quickstart

1. **Install** with the lines above.
2. **Two keys.** Sign up at typesafe.ai (free) and set `JEV_API_KEY`. For the model that does
   the reviewing, have either [Claude Code](https://claude.com/claude-code) on your PATH
   (signed in) or `ANTHROPIC_API_KEY` set. Conjure uses the same one.
3. **Check.** In a git project, run `scry doctor` (and `conjure doctor`). Every row should be
   `ok`; a row that is not says what to fix.
4. **Run.** Change some code, then run `scry`. It reviews what you changed against HEAD and
   prints each finding it can stand behind (a file, a line, what is wrong). When there is
   nothing to say it says nothing. It exits 1 when a finding is open.

   ```sh
   cd my-project
   scry doctor
   # edit something, then:
   scry
   scry why <id>       # the reasoning behind a finding (ids are printed with each)
   scry dismiss <id>   # a finding that is wrong, so it stays quiet
   ```
5. **Conjure.** Ask for one small change at a line, or at a scry finding:

   ```sh
   conjure src/cart.py:12           # prints the change; touches nothing
   conjure --finding <id> src/cart.py:43
   ```

   In an editor the same thing is one click (below), and the change lands in your buffers.

A first review on the first run of a language is a little slower while its grammar downloads.

### When it asks you to trust a project

A project's own `.sorcerer/config.toml` can name a program to run or a URL to send your code
and key to. Sorcerer will not do that until you say so, once per project:

```sh
scry trust        # or: conjure trust
```

`scry doctor` shows a `trust` row that says where the project stands. A project that sets
none of these needs no trust. `scry trust --revoke` takes it back. The editor's conjure also
needs the project trusted.

## In your editor

Both editors start the language servers (`scry lsp`, `conjure lsp`) for you, so install the
binaries first (above). Save a file and scry's findings appear on their lines. Put the cursor in
a function (or select lines), open the quick fixes (the lightbulb, or `Ctrl+.` / `Cmd+.`), and
run **Conjure here**; on a line with a scry finding, **Conjure for this finding**. The change
lands in your buffers, unsaved, with a mark that offers **Undo**, **Try harder**, **Show why**.
On a finding, the same menu has **Show why**, **Show code** and **Dismiss**.

### VS Code

Install **Scry** and **Conjure** (publisher `sorcererdev`; ids `sorcererdev.scry` and
`sorcererdev.conjure`) from the Extensions view, which uses the Marketplace. VS Code forks such
as VSCodium get them from Open VSX. From a terminal:

```sh
code --install-extension sorcererdev.scry
code --install-extension sorcererdev.conjure
```

- **Finding the program.** The extension looks for `scry` / `conjure` in the setting
  `scry.path` / `conjure.path` (your user settings only), then on PATH, then `~/.cargo/bin` and
  `~/.local/bin`. If it finds none it offers to download that version for you, checked against
  its sha256, so installing the binary first is optional.
- **Keys.** VS Code copies the environment of your login shell when it starts. Put
  `JEV_API_KEY` and `ANTHROPIC_API_KEY` in your shell's profile (`~/.zshrc`, `~/.profile`),
  then quit VS Code completely and start it again. Started from a launcher or the Dock, it may
  not see keys set only in a terminal session.

### Neovim

Needs Neovim 0.10 or newer. With [lazy.nvim](https://github.com/folke/lazy.nvim), the plugin
[`sorcerer.nvim`](https://github.com/sorcererdev/sorcerer.nvim) with conjure turned on:

```lua
{
  "sorcererdev/sorcerer.nvim",
  opts = {
    scry = { enabled = true },
    conjure = { enabled = true },   -- off by default
  },
  keys = {
    -- the plugin binds no keys: this is Neovim's own code-action menu
    { "<leader>ca", vim.lsp.buf.code_action, mode = { "n", "v" }, desc = "Code action" },
  },
}
```

On a finding's line, `<leader>ca` offers Show why, Show code and Dismiss; in a function it
offers **Conjure here**.

Neovim must be able to find `scry` and `conjure`: they are in `~/.local/bin`, so that folder
has to be on the PATH Neovim starts with. Started from a launcher rather than a terminal, it
often is not; then give the full path in `opts`:

```lua
scry = { cmd = { "/Users/you/.local/bin/scry", "lsp" } },
conjure = { enabled = true, cmd = { "/Users/you/.local/bin/conjure", "lsp" } },
```

Neovim also reads the API keys from the environment it started in. `:checkhealth sorcerer`
says whether it found the programs.

## What it costs

Sorcerer spends your model account, and keeps its own count.

- **A review** of a small change: about **$0.07** through the Anthropic API, or about **$0.04**
  of Claude Code usage.
- **A conjure:** about **$0.001**; **Try harder** (a stronger model) about **$0.03**.
- **Caps**, so a bad day cannot run away: **$0.25 a review pass** and **$3 a day** by default
  (`budget_per_pass`, `budget_per_day` under `[scry]` in `.sorcerer/config.toml`).

`scry cost` and `conjure cost` show what was spent, a day a row.

## When something is wrong

- **`scry doctor` / `conjure doctor`** check git, your keys, the model, the languages and the
  trust, one row each, and say what to do about a row that is not `ok`.
- **The editor shows nothing.** Run the doctor in the same terminal you start the editor from;
  then check that the editor can find the program and the keys (the VS Code and Neovim notes
  above).
- **Run logs.** Every review writes a Markdown log to `.sorcerer/state/scry/runs/` in the
  project: what it looked at, what it asked, what it spent. `scry log` prints the latest;
  `conjure log` shows the last changes the editor recorded.
- **Still stuck?** Open an issue at https://github.com/sorcererdev/sorcerer/issues with the
  doctor's output and the run log.

## More

`scry` and its editor chapters, configuration (`.sorcerer/config.toml`, your rules in
`.sorcerer/scry.toml`) and adding a language are in the user book.

## Books

Not published online yet. Build them locally with `mdbook` (`cargo install mdbook`):

- `book/user/`: using sorcerer (`mdbook serve book/user`)
- `book/internals/`: how it works, so you can change it (start with its Architecture chapter:
  every crate is in layers, checked by `scripts/layers`)

Scry's quality is measured by the eval (`cargo test -p sorcerer-scry --test eval`), which
replays recorded reviews of labelled cases in Python and Rust (`corpus/scry/`).

## License

Apache-2.0. Contributions carry a DCO sign-off; see `CONTRIBUTING.md`.
