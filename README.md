# Sorcerer

Small tools that read your code alongside you. **scry** reviews what you changed and puts each
finding on its line. **conjure** writes one small change where you ask, and marks it so you can
undo it. **spellbook** (conventions as data) is in its first milestone: generators write every file of a
convention, with holes that say what belongs there, and rules check that the conventions hold.
**apprentice** is a coding agent whose whole interface is your editor: you write in your code, it
answers there or offers to do it, and works in your buffers when you accept. It runs Claude Code
(`claude -p`) on your own subscription, so it needs `claude` installed and signed in. They share
one core and talk through files in `.sorcerer/`.

**Status:** scry, conjure, spellbook and apprentice (the newest, an early release); Python and Rust; Windows, macOS and Linux; early (0.1).

## Install

Each tool is one program. Install the ones you want (you can start with just scry).

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.sh | sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-conjure-installer.sh | sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-spellbook-installer.sh | sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.ps1 | iex"
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-conjure-installer.ps1 | iex"
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-spellbook-installer.ps1 | iex"
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.ps1 | iex"
```

Or with Rust: `cargo install sorcerer-scry sorcerer-conjure sorcerer-spellbook sorcerer-apprentice`.

The installers put `scry`, `conjure`, `spellbook` and `apprentice` in `~/.local/bin` and need neither Rust nor a C
compiler. They end by telling you to open a new terminal or run a `source` line; do that, then
check:

```sh
scry --version
```

If that says "command not found", `~/.local/bin` is not on your PATH yet.

## Get started

You need two things besides the programs, both set up once.

1. **A model account.** Either [Claude Code](https://claude.com/claude-code) on your PATH and
   signed in (`claude` works in your terminal), or `ANTHROPIC_API_KEY` set. Scry and conjure
   both use it.
2. **A Jev key.** Jev is the checker that confirms a finding before scry shows it. Sign up at
   typesafe.ai (free) and set `JEV_API_KEY` in your shell profile (`~/.zshrc`, `~/.bashrc`).

Then try it on a small project. Make one:

```sh
mkdir my-project && cd my-project && git init
mkdir src
cat > src/cart.py <<'PY'
def total(prices, discount):
    """Sum prices and apply a percentage discount."""
    s = 0
    for p in prices:
        s += p
    return s - s * discount / 100


def average(prices):
    return total(prices, 0) / len(prices)
PY
git add . && git commit -m "start"
```

**Check that everything is in place.** Every row should say `ok`; a row that does not says
what to fix (for example `provider FAIL no model provider: put claude on PATH or set
ANTHROPIC_API_KEY`):

```sh
scry doctor
```

```text
git        ok    /usr/bin/git
provider   ok    claude (claude is on PATH)
jev        ok    jev (JEV_API_KEY is set)
languages  ok    python load (downloaded)
...
```

(The first time it meets a language it downloads that language's grammar, which takes a
moment.)

**Make a mistake, and let scry find it.** Scry reviews what you changed since your last
commit. Break `average` so it divides by one too many:

```sh
sed -i.bak 's|len(prices)|(len(prices) - 1)|' src/cart.py && rm src/cart.py.bak
scry
```

```text
src/cart.py:9  bug  average divides the total by len(prices) - 1 instead of len(prices), so it no
                    longer returns the arithmetic mean (e.g. [10,20,30] gives 30.0 instead of 20.0).
                    ...  (0.98)
  evidence: src/cart.py:9-10
  scry why 7386d0
1 finding, 1 unit looked at, $0.02
```

Your wording and id will differ. When scry has nothing to say it prints nothing. It exits 1 while
a finding is open, so it works in scripts and hooks.

**The everyday commands:**

```sh
scry why 7386d0       # the reasoning and evidence behind a finding (use the id it printed)
scry dismiss 7386d0   # the finding is wrong: it stays quiet from now on
scry list             # the findings it remembers
scry cost             # what you have spent, a day a row
```

**Ask conjure for a change.** Give it a file and a line, or a scry finding. From the command
line it only prints the change and touches nothing:

```sh
conjure src/cart.py:9
conjure --finding 7386d0 src/cart.py:9
```

```text
goal: Restore the divisor in average to len(prices) so it returns the arithmetic mean again.
--- src/cart.py
@@ -7,4 +7,4 @@
 
 
 def average(prices):
-    return total(prices, 0) / (len(prices) - 1)
+    return total(prices, 0) / len(prices)
parses
haiku · $0.000 · 2 s
```

To get the change into your file, use conjure in an editor (below): it lands in your buffer,
unsaved, with an Undo.

### When it asks you to trust a project

A project's own `.sorcerer/config.toml` can name a program to run or a URL to send your code
and key to. Sorcerer will not do that until you say so, once per project:

```sh
scry trust        # or: conjure trust
```

`scry doctor` shows a `trust` row that says where the project stands. A project that sets
none of these (like the one above) needs no trust. `scry trust --revoke` takes it back. The
editor's conjure also needs the project trusted.

## In your editor

Both editors start the language servers (`scry lsp`, `conjure lsp`) for you, so install the
binaries first (above). Save a file and scry's findings appear on their lines. Put the cursor in
a function (or select lines), open the quick fixes (the lightbulb, or `Ctrl+.` / `Cmd+.`), and
run **Conjure here**; on a line with a scry finding, **Conjure for this finding**. The change
lands in your buffers, unsaved, with a mark that offers **Undo**, **Try harder**, **Show why**.
On a finding, the same menu has **Show why**, **Show code** and **Dismiss**.

### VS Code

Install **Scry**, **Conjure**, **Spellbook** and **Apprentice** (publisher `sorcererdev`; ids
`sorcererdev.scry`, `sorcererdev.conjure`, `sorcererdev.spellbook` and `sorcererdev.apprentice`) from the Extensions view, which uses the Marketplace. VS Code forks such
as VSCodium get them from Open VSX. From a terminal:

```sh
code --install-extension sorcererdev.scry
code --install-extension sorcererdev.conjure
code --install-extension sorcererdev.spellbook
code --install-extension sorcererdev.apprentice
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
- **An apprentice task** is a Claude Code session on your own subscription: usage, not dollars,
  capped at **$2.00** of usage a task by default (`budget_per_task` under `[apprentice]`).
  `apprentice cost` shows it.
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

## For contributors

How it works is in `book/internals/` (build with `mdbook serve book/internals`; start with its
Architecture chapter). Scry's quality is measured by the eval
(`cargo test -p sorcerer-scry --test eval`), which replays recorded reviews of labelled cases
in Python and Rust (`corpus/scry/`).

## License

Apache-2.0. Contributions carry a DCO sign-off; see `CONTRIBUTING.md`.
