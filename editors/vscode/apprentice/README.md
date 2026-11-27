# Apprentice for VS Code

[Apprentice](https://github.com/sorcererdev/sorcerer) in VS Code: a coding agent whose whole
interface is your editor (an early release). There is no chat. Write in your code (a comment, a
sentence in a docstring, a function whose body is `pass`) and apprentice answers where you wrote,
offers to do what you asked, and when you accept, works in your buffers while you keep typing.
Everything it makes is a change in your editor, unsaved, that you review, edit and undo.

**It runs Claude Code on your own subscription.** Each task you accept is one Claude Code session
(`claude -p`, started by apprentice in your project) on your own login, so you need
[Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed and signed in (`claude` on
your `PATH`). Tasks count against your Claude usage; apprentice caps each one (2.00 dollars of
usage by default, `budget_per_task`) and `apprentice cost` shows what it spent. Answering
questions and checking claims use the provider in your Sorcerer config, and the triage that
finds them uses Jev (`JEV_API_KEY`); without it only empty function bodies are offered.

The extension is thin: it starts `apprentice lsp` for each workspace folder and VS Code does most
of the rest. **Marks** are diagnostics of source `apprentice` (the Problems panel, the squiggles,
the hover); their **code actions** are the lightbulb's: `apprentice: do this · ~$0.50 · sonnet`,
*Show code*, *Keep this answer*, *Dismiss*, the four labels (`apprentice: this was a question` …),
and on a finished task's mark *Show diff*, *Undo this task* and *Show journal*; *Stop this task*
on any line while one runs. A question the agent asks on a line is a mark there with one action
per option; a command it wants to run is asked in a notification with buttons. The status bar
says what a running task does.

What the extension draws itself:

- the grey `⟳ apprentice: writing…` after the line being written;
- **ghost lines**: a line the agent suggests you type, as grey text after the line it belongs
  under (`⤷ …`, the whole of it on hover); your next edit of that line takes it away;
- the agent's **notes**, opened in the column beside your code;
- **Show diff**: VS Code's diff view of the file before the task against the live buffer, in a
  tab of its own;
- a violet sparkle in the gutter (and a tick in the overview ruler) on each file's first line a
  finished task changed.

## Keys

The same letters as the Neovim plugin's `<leader>m` keys, each running the action the lightbulb
would offer on the cursor's line:

| keys | does |
|---|---|
| `ctrl+m a` | `apprentice: do this`, whatever its price |
| `ctrl+m q` | `apprentice: Stop this task` |
| `ctrl+m d` | `apprentice: Show diff` |
| `ctrl+m z` | `apprentice: Undo this task` |
| `ctrl+m j` | `apprentice: Show journal` |
| `ctrl+m x` | `apprentice: Dismiss` |
| `ctrl+m l` | the four labels, to pick one |

On macOS `ctrl` is the Control key, not Command. A line that offers no such action says so in the
status bar. On Windows and Linux `ctrl+m` alone is VS Code's "Toggle Tab Key Moves Focus"; as
the first key of a chord it no longer does that. Change or remove a binding in Keyboard
Shortcuts (search `apprentice.keys`).

## Permission questions

Every command the agent wants to run is asked: *Allow once*, *Always for this project (…)* or
*Deny*, in a notification with those buttons. Closing it, or leaving it for two minutes, is a
deny. With VS Code's *Do Not Disturb* on, the notification is hidden in the notification centre
(the bell), so the command waits there and is denied when the two minutes are up.

## Install

Install the extension from the Marketplace (VS Code forks such as VSCodium get it from Open VSX):

```sh
code --install-extension sorcererdev.apprentice
```

and the program. macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/sorcererdev/sorcerer/releases/latest/download/sorcerer-apprentice-installer.ps1 | iex"
```

Or with Rust: `cargo install sorcerer-apprentice`.

The extension finds `apprentice` from the `apprentice.path` user setting, then `PATH`, then
`~/.cargo/bin` and `~/.local/bin`. With none it offers to download it, or tells you the install
line above. apprentice needs `claude` on your PATH (signed in) and a project you have trusted
(`apprentice trust` in it); `apprentice doctor` checks both. See the
[user book](https://github.com/sorcererdev/sorcerer/blob/main/book/user/src/apprentice.md).
