# Changelog

## 0.1.0 — 2026-11-23

The first release: scry and conjure.

- **scry**, a reviewer in the margin: reviews what you changed and keeps each finding that
  survives a claim check and a nitpick screen. Python and Rust.
- **conjure**, one small change where you ask: `conjure FILE:LINE` prints one; in an editor,
  *Conjure here* (or *Conjure for this finding*) lands it in your buffers and marks it, with
  Undo and Try harder. About $0.001 a change.
- **Three operating systems.** Windows, macOS and Linux, tested in CI on each; one platform
  adapter holds every difference (paths, user directories, the C compiler, starting and stopping
  a runner and everything it started).
- **The editor clients.** `scry lsp` and `conjure lsp` are language servers. Neovim
  (`sorcerer.nvim`, conjure opt-in) starts them; the Scry and Conjure VS Code extensions find
  the program or offer to download their own version, checked against its sha256, and are
  published to the Marketplace and Open VSX with this release.
- **Installers.** Shell and PowerShell installers for each program
  (`sorcerer-scry-installer`, `sorcerer-conjure-installer`), and `cargo install sorcerer-scry
  sorcerer-conjure`.
- **Prebuilt grammars.** The Python and Rust grammars are downloaded prebuilt from the release
  on first use, so no C compiler is needed; any failure falls back to compiling, and a project's
  own grammar is always compiled locally. `scry doctor` says which it did.
- **Spending caps.** $0.25 a pass and $3 a day by default.
- **The eval's two gates.** The recorded reviews of the labelled cases in Python and Rust must
  pass their scores with the project's rules, and a second table holds the bare (no rules)
  scores.
