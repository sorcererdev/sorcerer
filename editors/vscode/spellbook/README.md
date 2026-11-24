# Spellbook for VS Code

[Spellbook](https://github.com/sorcererdev/sorcerer) in VS Code: a project's generators with holes
to fill, and its rules in Problems.

**Spellbook: New** picks one of the project's generators (`.sorcerer/spellbook/generators/`), asks
its inputs, shows the plan (each new file as a read-only preview, coloured as its language) and,
on **Apply**, writes the files. The opened file's holes become VS Code's own snippet
placeholders: the first is selected, typing replaces it, `Tab` and `Shift+Tab` move between them,
and the status bar says which hole you are in and what it wants. **Spellbook: Check** runs the
project's rules (`.sorcerer/spellbook/rules.toml`) over the whole folder into Problems.

The extension is thin: it starts `spellbook lsp` for each workspace folder and asks it. Spellbook
calls no model; the extension holds no key. A hole's line offers **Conjure here** when the Conjure
extension (`sorcererdev.conjure`) is installed.

## Keys

| keys | does |
|---|---|
| `ctrl+m n` | **Spellbook: New** |
| `ctrl+m k` | **Spellbook: Check** |
| `ctrl+m c` | **Conjure here**, on a hole too (from the Conjure extension) |

They are the Neovim plugin's `<leader>m` keys, and work while the cursor is in an editor (in the
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
{ "key": "ctrl+m b", "command": "spellbook.new", "when": "editorTextFocus",
  "args": { "generator": "command", "inputs": { "service": "booking" } } }
```

## Install

The extension is not on the Marketplace yet, and `spellbook` has no installer yet. Build the
program with Rust:

```sh
cargo install --git https://github.com/sorcererdev/sorcerer sorcerer-spellbook
```

and the extension from this folder (`npm ci && npm run package`), then sideload it:

```sh
code --install-extension spellbook-0.1.0.vsix
```

The extension finds `spellbook` from the `spellbook.path` setting, then `PATH`, then
`~/.cargo/bin` and `~/.local/bin`. With none it tells you the install line above.

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
a VS Code and runs Check, New and the hole session against the real `spellbook lsp` on spellbook's
bistro corpus (build it first: `cargo build -p sorcerer-spellbook`, or point
`SPELLBOOK_E2E_BINARY` at one). `src/binary.ts`, `src/download.ts` and `src/offer.ts` are copies
of scry's extension's, by product name; CI fails when they drift. Press F5 to run the extension.
`npm run package` makes the `.vsix`.
