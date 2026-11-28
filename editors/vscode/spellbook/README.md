# Spellbook for VS Code

[Spellbook](https://github.com/sorcererdev/sorcerer) in VS Code: a project's generators with holes
to fill, and its rules in Problems.

**Spellbook: New** picks one of the project's generators (`.sorcerer/spellbook/generators/`), asks
its inputs, shows the plan (each new file as a read-only preview, coloured as its language) and,
on **Apply**, writes the files. The opened file's holes become VS Code's own snippet
placeholders: the first is selected, typing replaces it, `Tab` and `Shift+Tab` move between them,
and the status bar says which hole you are in and what it wants. **spellbook: Check the project's rules** runs the
project's rules (`.sorcerer/spellbook/rules.toml`) over the whole folder into Problems.
On a finding's line the lightbulb offers its **fix** (`ctrl+m f`, fix this, runs the first): the plan shows with a diff for each changed file, and Apply changes the open files in the editor (`ctrl+z` takes it back).

**Owned regions** (`.sorcerer/spellbook/regions.toml`) are kept as you save: write or delete a
file a region lists and save, and the region's block is refreshed, in the editor when its file is
open (unsaved, one `ctrl+z` takes it back), on disk when it is not; the status bar says
`spellbook: synced region modules in src/booking/commands/mod.rs (+1 −0)`. With
`[spellbook] sync_on_save = false` in `.sorcerer/config.toml` a stale block is a `region-stale`
problem instead, whose quick fix is **Sync region NAME**. **spellbook: Sync regions** refreshes every
region the same way: the plan, then Apply.

The extension is thin: it starts `spellbook lsp` for each workspace folder and asks it. Spellbook
calls no model; the extension holds no key. A hole's line offers **Conjure here** when the Conjure
extension (`sorcererdev.conjure`) is installed.

## Keys

| keys | does |
|---|---|
| `ctrl+m g`, `ctrl+m b g` | **Generate…**: pick a generator and write its files |
| `ctrl+m f` | **Fix this**: the finding's spellbook fix on the cursor's line, else the line's preferred quick fix |
| `ctrl+m b f` | **Fix**: the spellbook fix only |
| `ctrl+m b k` | **Check the project's rules** into Problems (an open file is checked as you type) |
| `ctrl+m b s` | **Sync regions**: refresh the owned regions |
| `ctrl+m b h` | **List holes**: the holes still open, in a picker |
| `ctrl+m u` | undo the last change, when spellbook made it (VS Code's undo in each file it changed) |
| `ctrl+m n` | next step (**Conjure here**), on a hole too (from the Conjure extension) |

They are the Neovim plugin's `<leader>m` keys, under **spellbook** in the Command Palette, and work while the cursor is in an editor (in the
integrated terminal `ctrl+m` is Enter). On macOS `ctrl` is the Control key, not Command.

**`ctrl+m` and Tab.** On Windows and Linux, `ctrl+m` is VS Code's own "Toggle Tab Key Moves
Focus"; as the first key of a chord, it no longer does that. To keep it, or to use other keys,
open Keyboard Shortcuts (`ctrl+k ctrl+s`), search `spellbook.`, and change or remove a binding; or
put your own in `keybindings.json`:

```json
{ "key": "ctrl+alt+n", "command": "spellbook.new", "when": "editorTextFocus" }
```

`spellbook.new` takes arguments that skip questions:

```json
{ "key": "ctrl+alt+g", "command": "spellbook.new", "when": "editorTextFocus",
  "args": { "generator": "command", "inputs": { "service": "booking" } } }
```

## Install

Install the extension from the Marketplace (VS Code forks such as VSCodium get it from Open VSX):

```sh
code --install-extension sorcererdev.spellbook
```

and the program. macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-spellbook-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-spellbook-installer.ps1 | iex"
```

Or with Rust: `cargo install sorcerer-spellbook`. To build the extension yourself, see Development below.

The extension finds `spellbook` from the `spellbook.path` setting, then `PATH`, then
`~/.cargo/bin` and `~/.local/bin`. With none it offers to download it, or tells you the install line above.

## Trust

A project whose own config could run a program (a provider `runner` or `url`, a grammar of its
own) must be trusted first: until then the server says `spellbook: this project is not trusted;
run "spellbook trust" in it`. Run `spellbook trust` in a terminal in the project.
`spellbook doctor` on the command line checks the rest; the server's log is the Output channel
**Spellbook**.

## Settings

- `spellbook.path`: the `spellbook` binary (empty: look it up). Read from your user settings only.
- `spellbook.holeSession` (on): after New, the opened file's holes as snippet placeholders.
- `spellbook.conjureHoles` (off): after New, run conjure on the first hole instead (needs the
  Conjure extension).

## Development

`npm ci`, then `npm run lint`, `npm run test:unit`, `npm run build`. `npm run test:e2e` downloads
a VS Code and runs Check, New, the hole session, a fix and a save's sync against the real `spellbook lsp` on spellbook's
bistro corpus (build it first: `cargo build -p sorcerer-spellbook`, or point
`SPELLBOOK_E2E_BINARY` at one). `src/binary.ts`, `src/download.ts` and `src/offer.ts` are copies
of scry's extension's, by product name; CI fails when they drift. Press F5 to run the extension.
`npm run package` makes the `.vsix`.
