# Scry in your editor

Scry reviews as you work: you save a file, scry reviews the units you changed, and each
finding it can stand behind appears on its line. A save with nothing worth saying shows
nothing at all.

## What you see

You edit `cancel.rs` and save.

- **A status line**: `scry: reviewing 2 units` appears a couple of seconds after the save and
  clears when the review ends. It appears only when scry actually sends something to a model,
  so a save with nothing new to review shows nothing.
- **The unit under review**: grey text at the end of the first line of each unit scry sent to
  the model, `⟳ scry: reviewing…`, so you see where it is looking. It follows its line as you
  edit and goes when that review call ends (it finished, failed, or a newer save overtook it).
  It is drawn by the Neovim plugin and the VS Code extension, which tell the server so when
  they start; any other editor shows only the status line.
- **A finding**: a diagnostic on its line, from source `scry`. A bug is an error; every other
  issue type (`assumption`, `convention`, your project's own) is a warning. The message is the
  finding's statement, then `rule: <id>` when it cites one of your project's rules. Editors
  that show related locations (VS Code does) link the code it cites as evidence.
- **Nothing to say**: nothing. No "all clear", no count.
- **You edit the unit again**: the finding moves with its unit and, from your next save until
  the next review ends, says `(the code changed since this review)` on its last line (its
  code becomes `bug-stale`). The next review of that unit is shown the finding and judges it
  on the new code: it resolves it, or, when the problem is still there, keeps it on the new
  code, no longer marked stale. Undo your change
  so the unit is again what you committed, and the finding is resolved at the next save, with
  no model call. Redo the change and the next save reviews it again, so the finding returns.

A finding shows only after it passed the nitpick screen and, where it makes a claim that can
be checked, the claim check; a finding still waiting for its check is not shown. At most three
show in a file. These are the same rules `scry` follows in a terminal, and the editor and the
terminal share one store: `scry list` shows the same finding with the same id, and a dismissal
in either place holds in both.

Saves are reviewed one review at a time. Two quick saves (or your editor's autosave) become one
review, two seconds after the last. A save during a review waits for it, and if you change
every unit a running review call was sent, that call is stopped so your new code is reviewed
sooner. If a review cannot start at all (your config does not parse, say), the files you saved
are reviewed again at their next save. When a change is bigger than one review's budget (`budget_per_pass`), scry keeps going by
itself, one review after another, until the change is reviewed or the day's budget is spent.
The editor reviews within `[scry] budget_per_pass` and `budget_per_day`
([Scry](scry.md)).

## Setting it up

You need `scry` on your PATH:

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.ps1 | iex"
```

With Rust: `cargo install sorcerer-scry`. The installers put `scry` in `~/.local/bin` and need
neither Rust nor a C compiler: the grammars come prebuilt with the release.

And what `scry` itself needs: `JEV_API_KEY`, and Claude Code on PATH or
`ANTHROPIC_API_KEY` ([What sorcerer is](what-sorcerer-is.md)). Your editor starts `scry lsp`,
a language server, in the folder you open; you never run it yourself.

### Neovim

`sorcerer.nvim` (Neovim 0.10 or later), with lazy.nvim:

```lua
{
  "sorcererdev/sorcerer.nvim",
  config = function()
    require("sorcerer").setup({
      scry = {
        enabled = true,
        cmd = { "scry", "lsp" },
        signs = { bug = "!", assumption = "?", convention = "§", design = "~" },  -- false: plain diagnostics
        bug_text = true,   -- a bug's first sentence at the end of its line
      },
    })
  end,
}
```

Those are the defaults. The plugin starts `scry lsp` for every ordinary file buffer, at the
nearest directory upward holding `.sorcerer/`, else `.git`. Signs show one mark per finding
line by issue type (an issue type with no mark of its own gets `•`), tint the line number, and
dim a stale finding; Neovim's own sign and virtual text are turned off for scry's diagnostics
so nothing is drawn twice. No text in your buffer moves, not even the grey `⟳ scry: reviewing…`
(virtual text, in the `SorcererWorking` highlight, linked to `Comment`). `require("sorcerer").status()` gives
`scry: reviewing 2 units` or an empty string, for your status line; any LSP progress display
(fidget, `vim.lsp.status()`) shows the same. `:checkhealth sorcerer` says whether `scry` is on
your PATH and its version.

Conjure is off until you turn it on (`conjure = { enabled = true }` in `setup()`); install it with
`sorcerer-conjure-installer` (the same line with `conjure` for `scry`) or `cargo install
sorcerer-conjure`. sorcerer.nvim binds the keys itself: `<leader>mc` conjures here, `<leader>mu` undoes,
`<leader>mt` tries harder, `<leader>mw` says why (and `<leader>mn` / `<leader>mf` / `<leader>mk` are spellbook's
`:Spellbook new`, `:Spellbook fix` and `:Spellbook check`; see [Spellbook](spellbook.md#in-neovim)). Set `keys = false`, or
one key to `false`, to bind none; a key you already mapped is left alone and listed by
`:checkhealth sorcerer`. Neovim must find `scry` and `conjure` on the PATH it starts with
(`~/.local/bin`); started from a launcher it often cannot, so give `cmd = { "/full/path/to/scry",
"lsp" }`.

Apprentice (a prototype) is off too (`apprentice = { enabled = true }`); build it with `cargo
install --git https://github.com/sorcererdev/sorcerer sorcerer-apprentice`. Its keys are
`<leader>ma` (do this), `ms` (stop), `md` (show diff), `mz` (undo the task), `mj` (journal), `mx`
(dismiss) and `ml` (label); see [Apprentice](apprentice.md#in-neovim).

### VS Code

Install **Scry** (`sorcererdev.scry`) from the Extensions view (the Marketplace; VS Code forks
such as VSCodium use Open VSX), or `code --install-extension sorcererdev.scry`. **Conjure**
(`sorcererdev.conjure`) installs the same way. VS Code copies the environment of your login
shell when it starts, so put `JEV_API_KEY` and `ANTHROPIC_API_KEY` in your shell's profile and
restart VS Code completely. The Scry extension looks for `scry` (`scry.exe` on Windows) in the
setting `scry.path`, then your PATH, then `~/.cargo/bin`, then `~/.local/bin` (where the
installers put it), then the binary it downloaded itself. `scry.path` is read from your user
settings only: a workspace's `.vscode/settings.json` cannot choose the program the extension
runs, and in Restricted Mode the setting is not read at all. It starts one `scry lsp` per workspace
folder. The diagnostics, the lightbulb and the progress in the status bar are VS Code's own;
the grey `⟳ scry: reviewing…` after a unit's first line is the extension's (a text decoration in the
theme's `editorCodeLens.foreground` colour, italic, so a theme can change it).

**Spellbook** (`sorcererdev.spellbook`) installs the same way, from the Marketplace (Open VSX for
forks), and so does its program (`sorcerer-spellbook-installer`, or `cargo install
sorcerer-spellbook`). `ctrl+m n` is **Spellbook: New**, `ctrl+m k`
**Spellbook: Check** and `ctrl+m f` **Spellbook: Fix** (the finding's fix on the cursor's line); see [Spellbook](spellbook.md#in-vs-code).

With no `scry` in any of those places, it offers:

> scry 0.1.0 is not installed. Download it (12 MB, from github.com/sorcererdev/sorcerer
> releases)? [Download] [I'll install it myself]

**Download** fetches the release of the extension's own version for your machine (macOS on
Apple silicon or Intel, Linux on x64 or arm64, Windows on x64) and its `.sha256`, checks the
sum, and unpacks `scry` into the extension's own storage (VS Code's `globalStorage` directory
for `sorcererdev.scry`, under `bin/<version>/`); then it starts the server. A sum that does not
match installs nothing and says so. The next extension version offers its own `scry` the same
way and removes the old one. **I'll install it myself** shows the install line for your
platform and stays quiet until the next version. If the release cannot be reached (offline, or
GitHub answers with an error), it says so and asks again the next time VS Code starts. So does
a download that fails or whose sum does not match: nothing was installed, and the offer comes
back on the next start. Closing the offer without an answer also brings it back. A version
with no release at all (a build of your own from main, say) is told the install line once. On a machine the release has no binary for
(Linux on 32-bit ARM, Windows on ARM), it says once to use `cargo install sorcerer-scry`.

## The four actions

Your editor's code-action menu on a finding's line (Neovim: your code-action key, or `<leader>mc/u/t/w` for conjure's; VS Code:
the lightbulb, or `ctrl+m c/u/t/w` for conjure's) offers:

- **scry: Show why**: the finding in full, as `scry why` prints it, opened beside your code
  (it is written to `.sorcerer/state/scry/why-<id>.md`).
- **scry: Show code**: opens the first piece of code it cites, selected. Left out when it
  cites none.
- **scry: Dismiss here**: stops showing this finding. A finding waiting behind the limit of
  three takes its place.
- **scry: Dismiss everywhere**: also keeps the same problem from being raised in any other
  unit of the project.

## What it says unprompted

Four messages, each rare. Everything else goes to the editor's log for the server and to the
run log (`scry log`).

- **The day's budget is spent**, once a local day: `scry: today's budget ($3.00) is spent.
  Reviews resume tomorrow; "scry cost" shows the days.`
- **Scry cannot run** (no `JEV_API_KEY`, no provider), once a session, at the first save
  that had something to review: `scry is not reviewing: <reason>. Run "scry doctor".`
- **The trust question**, once a session (below).
- **A pass was skipped** after its review call, because Jev could not answer one of its
  questions (scry does not judge on a guess): `scry: a pass was skipped: Jev did not answer;
  nothing it found was stored, and its units are sent again at the next save`. The status line
  ends with `skipped, see "scry log"`, and the run log holds the review's whole reply.

A review call that fails (a model error, a timeout) ends the status line with `review failed,
see "scry log"` and shows no message.

## Trust

A project whose own files set something that runs code or moves your API key (a provider's
`runner` or `url`, or a grammar of its own in `.sorcerer/languages/`) is not reviewed by the
editor until you trust it, because your editor starts scry by itself in whatever folder you
open. At the first save in such a project the editor asks:

```
scry: this project's config sets
[model.providers.mine] runner = /opt/run
Trust this project?
```

*Trust* puts the project on your trust list and reviews what you saved. *Not now*, or closing
the question, leaves the project alone until your editor starts scry again. An editor that
cannot ask gets `scry is not reviewing: this project's config sets <key>. Run "scry trust" in
this project to allow that.` instead.

`scry trust` trusts the project from a terminal, `scry trust --revoke` stops trusting it, and
`scry doctor`'s `trust` row says where the project stands. [Scry](scry.md#trust) has the rest.
A project that sets none of these needs no trust, and is never asked about.

## What it never does

- Show a message, a count or a badge when there is nothing to say.
- Show a finding before its claim check.
- Review while you type: only a save starts a review.
- Slow or replace hover, go-to-definition or completion: scry adds no hover handler and hears
  only opens and saves.
- Move text in your buffer.
- Show findings for files you have not opened (`scry list` is the whole project).
- Filter findings its own way: the editor shows what `scry` would print.
- Offer a fix, for now.
