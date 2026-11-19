# Sorcerer conventions

Sorcerer is a Rust workspace: `sorcerer-core` (understanding a codebase), `sorcerer-models`
(talking to models and Jev), and one crate per product (`scry`, later `conjure`, `spellbook`).
Every crate is layered the same way: **use cases** are its API, **domain** holds its pure
logic and data, **ports** are the traits it needs from outside, **adapters** implement them,
and **cli/** (or an editor server) drives the use cases. Dependencies point inward only:
cli and adapters → use cases → ports → domain. The layout is moving to this shape; hold new
and changed code to it.

## Rules

### usecases-are-the-api
Each feature a crate offers is one module under `usecases/`, and its public function is that
feature's whole API (for example `review_change(project, &ReviewRequest, &ReviewDeps)`). The
steps a use case runs are private modules of it. Callers (cli, an editor server, another
crate) call use cases, never a use case's steps or a domain function to do a feature's work.

### domain-is-pure
Code under `domain/` does no I/O: it reads no files, runs no processes, makes no network
calls, reads no clock and no environment. It depends on nothing else in its crate
(`use crate::ports`, `crate::usecases`, `crate::adapters` and `crate::cli` are all
violations). Its functions take values and return values, so it is tested without doubles.

### usecases-depend-inward
Code under `usecases/` uses `domain/` and `ports/` only. It never names an adapter or the cli,
never constructs a real implementation, and never touches the filesystem, a process or the
clock except through a port.

### ports-are-traits
`ports/` holds traits and the plain data they pass, nothing else: no implementation of a port
lives there, and a port names what the use case needs in its own words, not the technology
behind it (`Reviewer`, not `ClaudeClient`).

### adapters-implement-ports
Code under `adapters/` implements ports (files, git, models, Jev, the clock, logs) and holds no
feature logic: a decision about findings, scope, confidence or budgets belongs in the domain
or a use case. Real adapters are built for a project in one place (`adapters/wiring.rs`).

### cli-is-thin
A cli command parses its arguments, calls one use case, and formats the result. It makes no
decisions a use case or the domain should make, so an editor server can offer the same
feature by calling the same use case.

### one-name-per-concept
A concept has one name everywhere: config keys, code, command output, JSON fields, and the
books (an issue type is `issue_type`, not `kind` in one place and `type` in another; the
review step is "review" everywhere). A new name for an existing concept, or a reused name for
a different one, is a finding.

### the-engine-knows-no-language
No language name, file extension or language-specific syntax appears in `crates/*/src/`:
languages are data under `library/languages/<name>/`. A prompt or rule that assumes one
language's conventions (such as `Class.method`) takes them from the language's data instead.

### nothing-outside-the-repository
Code, comments, books, scripts and corpus files never point at something a reader of the
repository cannot open: no paths on one person's machine, no private notes, no design
documents kept elsewhere. What a reader needs to understand the code is in the code or the
books or other documentation that is publicly available.

### books-change-with-the-code
A change to behaviour, commands or config keys updates `book/user/`; a new or reshaped
module, data flow or seam updates `book/internals/`, naming the functions and files a reader
would open. A chapter that disagrees with the code is a bug.
