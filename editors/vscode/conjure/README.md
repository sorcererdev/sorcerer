# Conjure for VS Code

[Conjure](https://github.com/sorcererdev/sorcerer) in VS Code. Put the cursor in a function (or
select lines), open the quick fixes, and run **Conjure here**: conjure writes one small change
through the editor, in every file it touches, unsaved. On a line with a scry finding, **Conjure
for this finding** does the same for that finding.

The extension is thin: it starts `conjure lsp` for each workspace folder and VS Code does the
rest. A **mark** (a diagnostic of source `conjure`, in the Problems panel and on the first
changed line of each file) says what the change did and what it cost, and offers **conjure:
Undo**, **conjure: Try harder** (undo, then a stronger model), **conjure: Show why** and
**conjure: Show &lt;file&gt;** for each other file. Marks clear when you save, undo or take the
next change. Conjure needs `claude` on your PATH (signed in) and a project you have trusted
(`conjure trust` in it); `conjure doctor` checks both.

## Install the binary

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-conjure-installer.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-conjure-installer.ps1 | iex"
```

With Rust: `cargo install sorcerer-conjure`.

The extension finds `conjure` from the `conjure.path` setting, then `PATH`, then `~/.cargo/bin`
and `~/.local/bin`. If it finds none it offers to download this version's binary, checked
against its checksum.

## Settings

- `conjure.path`: the `conjure` binary (empty: look it up).

## Development

`npm ci`, then `npm run lint`, `npm run test:unit`, `npm run build`. `npm run test:e2e` downloads
a VS Code and runs *Conjure here* and *Undo* against the real `conjure lsp` with a fake
`claude` (build them first: `cargo build -p sorcerer-conjure`, then `cargo build -p sorcerer-models
--bin fake-claude`). `src/binary.ts`, `src/download.ts` and `src/offer.ts` are copies of scry's
extension's, by product name; CI fails when they drift. Press F5 to run the extension.
`npm run package` makes the `.vsix`.
