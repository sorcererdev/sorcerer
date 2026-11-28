# Scry for VS Code

[Scry](https://github.com/sorcererdev/sorcerer) in VS Code. Save a file and scry reviews what
changed; each verified finding appears on its line as a diagnostic (source `scry`: an error for
a bug, a warning for the rest). When there is nothing to say, nothing is shown.

The extension is thin: it starts `scry lsp` for each workspace folder and VS Code does the
rest. Quick fixes on a finding offer Show why, Show code and Dismiss; the first time a project's
own config asks for something that runs code, scry asks whether to trust the project.

## Keys

The same letters as the Neovim plugin's `<leader>m` keys, under **scry** in the Command Palette:

| keys | does |
|---|---|
| `ctrl+m w`, `ctrl+m s w` | why is this flagged? (**scry: Show why** on the line) |
| `ctrl+m s r` | review this file now (a file with unsaved changes is saved, which reviews it) |
| `ctrl+m s x` | dismiss the finding, here or everywhere |
| `ctrl+m s l` | list findings (the Problems view) |
| `ctrl+m s c` | show the code the finding cites |

On macOS `ctrl` is the Control key, not Command. On Windows and Linux `ctrl+m` alone is VS Code's
"Toggle Tab Key Moves Focus"; as the first key of a chord it no longer does that. Change a
binding in Keyboard Shortcuts (search `scry.keys`).

## Install the binary

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-scry-installer.ps1 | iex"
```

With Rust: `cargo install sorcerer-scry`.

The installers put `scry` in `~/.local/bin`. Until the extension looks there too, set
`scry.path` to it, or start VS Code from a shell that has `~/.local/bin` on its PATH (on
macOS, VS Code started from the Dock may not).

The extension finds `scry` from the `scry.path` setting, then `PATH`, then `~/.cargo/bin`. If it
finds none it says so once and does nothing else.

## API keys

scry reads its API keys (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `JEV_API_KEY`) from the
environment. On macOS, VS Code started from the Dock copies the login shell's environment
(`~/.zshrc`) at start-up, so a key set there is seen. If scry says
`scry is not reviewing: no JEV_API_KEY. Run "scry doctor".`, set the key in `~/.zshrc` and
restart VS Code, or open the project from a terminal with `code .`.

## Settings

- `scry.path`: the `scry` binary (empty: look it up).

## Development

`npm ci`, then `npm run lint`, `npm run test:unit`, `npm run build`. `npm run test:e2e` downloads
a VS Code and checks a save against a Python stand-in for `scry lsp` (`test/fake-lsp`). Press F5 to
run the extension. `npm run package` makes the `.vsix`.
