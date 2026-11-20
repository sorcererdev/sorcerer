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
// before: // one call's cap: what the caps leave, never more than a pass
//         // may spend (…)
//         let cap = self.budget.room().map_or(self.cfg.budget_per_pass, |r| { … });
// after:  let cap = self.cap_for_one_call();
```

**4. Run-log and report text lives in its own functions** (`log_…`, `…_text`), so the logic reads
without it.

```rust
self.t(&held_back_text(self.review_estimate, self.budget.room()));
fn held_back_text(estimate: f64, room: Option<f64>) -> String { format!(…) }
```

**5. Helpers sit below their caller, private, in reading order.** A reader goes down the file and
does not leave it. No helper file of unrelated odds and ends.

```rust
pub(super) fn send(…) -> Asked { … self.units_not_yet_reviewed(…) … }
fn units_not_yet_reviewed(…) { … }   // directly below, private
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
let batches = batch_units(units);   // the established word for a group sent together
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

## Before and after: the review calls

`Run::send` in `crates/scry/src/usecases/review_change/ask.rs` sends a pass's units to the
reviewer. It was one function of more than two hundred lines; its first forty read:

```rust
pub(super) fn send(&mut self, scope: &Scope, fresh: bool) -> std::result::Result<Asked, Halt> {
    // the units to send, batched
    let to_send: Vec<&ScopedUnit> = scope
        .units
        .iter()
        .filter(|u| {
            fresh
                || !self
                    .mine
                    .reviewed
                    .get(&u.file)
                    .is_some_and(|s| s.contains(u.unit.fingerprint.as_str()))
        })
        .collect();
    let mut units_text = section("Units");
    for u in &scope.units {
        let sending = to_send.iter().any(|s| std::ptr::eq(*s, u));
        let what = if sending && fresh {
            "to send (fresh)"
        } else if sending {
            "to send"
        } else {
            "already reviewed at this fingerprint"
        };
        units_text.push_str(&format!(
            "- `{}` `{}` ({}-{}, fingerprint {}): {what}\n",
            u.file.display(),
            u.unit.name,
            u.unit.span.start,
            u.unit.span.end,
            u.unit.fingerprint,
        ));
    }
    if scope.units.is_empty() {
        units_text.push_str("- no unit in scope\n");
    }
    self.t(&units_text);
    let mut prints: HashMap<(PathBuf, String, u32), Fingerprint> = HashMap::new();
    let mut units = Vec::new();
    for u in &to_send {
    …
```

A reader meets a filter over the store, then the run log's text unit by unit, then a map of
fingerprints, before learning what the function is for. After, `send` is the story, and each
step is a helper below it:

```rust
pub(super) fn send(&mut self, scope: &Scope, fresh: bool) -> Asked {
    let to_send = self.units_not_yet_reviewed(scope, fresh);
    self.t(&units_text(scope, &to_send, fresh));
    let sent = sent_fingerprints(&to_send);
    let batches = self.batches_of(&to_send);
    self.t(&review_calls_heading_text(batches.is_empty()));
    let mut asked = Asked::default();
    for (n, batch) in batches.into_iter().enumerate() {
        self.t(&batch_heading_text(n, &batch));
        if self.must_hold_back(&asked) {
            self.hold_back_for_a_later_run(&batch, &mut asked);
        } else if self.cancel.is_cancelled() {
            self.fail_as_cancelled(&batch, &mut asked);
        } else {
            self.review_one_batch(scope, batch, &sent, &mut asked);
        }
    }
    asked
}
```

`self.t(…)` writes to the run log; `t`, `st` and `raws` keep their short names until the rename
table is ruled on, though rule 7 bans them.

The rules the old comments recorded now sit on the helpers that keep them: `must_hold_back`
(once one batch is held back every later one is), `cap_for_one_call` (never more than a pass
may spend) and `cost_of_call` (what a call is charged when it has no price or fails). The run
log's text is built by the `…_text` functions at the end of the file, and a golden file
(`crates/scry/tests/golden/run-log-send.md`) holds what they write.
