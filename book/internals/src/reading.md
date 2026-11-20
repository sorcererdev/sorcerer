# How the code reads

A reader opening a file must be able to tell what each block is for without first working out
how it does it. Sorcerer's code is held to eight rules so that the top of a file tells the story
and the detail sits below, one step at a time. `CLAUDE.md` holds the same rules for anyone, or
any agent, changing the code.

## The rules

**1. One level per body.** A function either names its steps or does one step's detail. Never
both.

```rust
let to_send = self.units_not_yet_reviewed(scope, fresh);   // names a step
for batch in self.batches_of(&to_send) { … }               // not: a loop over fields here
```

**2. A function that tells a story is short.** The top function of a file reads as the list of
what happens, in order; each line is a call whose name says the step.

```rust
let scoping = Scoping::new(project, view, &request, all, cfg)?;
for file in scoping.candidate_files(&request)? { … scoping.look_at(file)? … }
```

**3. Section comments become function names.** A comment such as `// the units to send, in
batches` marks where a helper belongs.

```rust
// before: `// the files changed since the base` above twenty lines
// after:  let files = self.candidate_files(&request)?;
```

**4. Run-log and report text lives in its own functions** (`log_…`, `…_text`), so the logic reads
without it.

```rust
self.t(&held_back_text(estimate, room));
fn held_back_text(estimate: f64, room: f64) -> String { format!(…) }
```

**5. Helpers sit below their caller, private, in reading order.** A reader goes down the file and
does not leave it. No helper file of unrelated odds and ends.

```rust
pub fn scope_of(…) { … look_at(…) … }
fn look_at(…) { … }          // directly below, private
```

**6. Arguments that travel together become a small struct with methods.**

```rust
struct Scoping<'a> { project, view, fences, exclude, base, all }
impl Scoping<'_> { fn look_at(&self, file: PathBuf) -> Result<FileInScope> { … } }
```

**7. Names.** The established word first; a made-up word only where none exists. No abbreviation a
newcomer cannot expand (`st`, `fp`, `raws`, `t`). A name says what the thing is for, not how it
is made.

```rust
fn units_not_yet_reviewed(…)   // not: fn filter_units(…)
let batches = batch_units(units);   // "batch", not a made-up "pack"
```

**8. Doc comments.** One plain sentence of intent on every function whose name does not already
say it all. A module's opening comment lists its steps in order, using the helpers' names. A
comment inside a body explains a *why* the code cannot (a rule, a trap), never a *what*.

```rust
/// The revision units are compared with; empty when the repository has no commit.
fn base_revision(…) -> Result<String>
```

What the rules do not ask for: a wrapper that only renames one call; a helper named after
mechanics (`process_item`, `handle_reply_2`); a split so fine that the detail is more than two
jumps from the story; comments that repeat the code.

## Two tools keep them

**clippy, a tripwire.** `clippy.toml` sets `too-many-lines-threshold = 100` and the workspace
turns on `clippy::too_many_lines`, so CI stops a function over 100 lines. The limit is not the
standard: a function of sixty lines that mixes levels still breaks rule 1. An exception is
written down where it is made:

```rust
#[expect(clippy::too_many_lines, reason = "readability stage 2")]
fn verify(…)
```

`expect`, not `allow`: when the function is shortened and the mark is left behind, the build
fails, so marks cannot go stale. Tests are exempt (`#![allow(clippy::too_many_lines)]` at the top
of each `tests/*.rs` and on each `mod tests`): a test is one story told in order.

**scry, the rules themselves.** The rules a linter cannot measure are `[[rule]]`s in sorcerer's
own `.sorcerer/scry.toml` (`one-level-per-body`, `helpers-say-intent`, `log-text-apart`,
`plain-names`, `comments-say-why`), so `scry` reviews each changed function against them. When a
rule yields nitpicks, its text is what gets fixed.
