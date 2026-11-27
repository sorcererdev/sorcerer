//! What a rules retry left out: the calls and definitions the first (rejected) answer had that
//! the second has nowhere in the change, and Jev's verdict on each. A drop never holds a change
//! back; it is said, so the person can look.
//!
//! The comparison is language-neutral: the names come from the language's queries (a
//! `@reference.call`'s callee, a definition's name), so a call the retry moved into another
//! file of the change is not a drop.

use std::collections::BTreeMap;

use sorcerer_models::domain::decide::{Answer, Question, Shape, UnsureAnswer, Weighed};

use crate::domain::spellbook_findings::RulesOutcome;

/// At most 5 drops are put to Jev; any more are said as not checked.
pub const MAX_DROPS_ASKED: usize = 5;

/// A drop is legitimate when Jev puts at least this much of its answer on "the rule asked for
/// it" and "it moved elsewhere" together.
const LEGITIMATE_AT: f64 = 0.7;
/// A drop looks unintended when Jev puts at most this much there.
const UNINTENDED_AT: f64 = 0.3;

/// The longest text of either answer's diff Jev is given, in bytes: about 3,000 tokens.
const MAX_DIFF_BYTES: usize = 12_000;

/// What a name in a text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameKind {
    /// A call, named by its callee as written: `std::fs::write`, `self.clock.now`.
    Call,
    /// A definition (a function, a method, a type), named by the unit's name.
    Definition,
}

/// One call or definition in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedInText {
    pub kind: NameKind,
    pub name: String,
    /// From 1.
    pub line: u32,
    /// Where on its line it starts, in bytes from 0.
    pub column: u32,
}

/// One file of an answer as the comparison sees it: its path, its text, and what is named in
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct FileNames {
    /// Relative to the project root, with `/`.
    pub path: String,
    pub text: String,
    pub names: Vec<NamedInText>,
}

/// A call or definition the first answer had and the retry has nowhere in the change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub kind: NameKind,
    pub name: String,
    /// The file the first answer had it in, relative to the root, with `/`.
    pub file: String,
    /// Its line in the first answer, from 1, and that line's text.
    pub line: u32,
    pub line_text: String,
    /// Where on the line it starts, in bytes from 0: of two drops on one line, the first said.
    pub column: u32,
}

/// Why Jev thinks a drop was right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhyRemoved {
    /// The broken rule forbids the code, and nothing the goal needs went with it.
    RuleAskedForIt,
    /// What it did is still done, by other code of the retry.
    MovedElsewhere,
}

/// Jev's verdict on one drop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropVerdict {
    /// Clearly right to drop: said quietly (the mark, the record), not in the step message.
    Legitimate(WhyRemoved),
    /// Clearly behaviour the goal needed, lost.
    LooksUnintended,
    /// Jev was not sure either way.
    Unsure,
    /// Jev was not asked (no key, the call failed, more than 5 drops).
    NotChecked,
}

impl DropVerdict {
    /// Whether the step message says the drop; a legitimate one is said only quietly.
    pub fn is_said(&self) -> bool {
        !matches!(self, DropVerdict::Legitimate(_))
    }

    /// The words after the drop: `looks unintended`, `moved elsewhere`.
    pub fn label(&self) -> &'static str {
        match self {
            DropVerdict::Legitimate(WhyRemoved::RuleAskedForIt) => "the rule asked for it",
            DropVerdict::Legitimate(WhyRemoved::MovedElsewhere) => "moved elsewhere",
            DropVerdict::LooksUnintended => "looks unintended",
            DropVerdict::Unsure => "maybe unintended",
            DropVerdict::NotChecked => "not checked",
        }
    }
}

/// A drop with its verdict and the rules the retry was asked to keep.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedDrop {
    pub dropped: Dropped,
    pub verdict: DropVerdict,
    /// Each rule the first answer broke: `commands-use-ports: Commands talk to ports`.
    pub rules: Vec<String>,
}

impl CheckedDrop {
    /// `std::fs::write in close_table.rs (looks unintended)`, the file by its name alone.
    pub fn text(&self) -> String {
        let name = self
            .dropped
            .file
            .rsplit('/')
            .next()
            .unwrap_or(&self.dropped.file);
        format!("{} in {name} ({})", self.dropped.name, self.verdict.label())
    }
}

/// The change's files three ways: before it, after the first answer, after the retry. Each
/// list holds the same files (a file an answer left alone is there with its text before the
/// change; a file it did not create is empty).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NamesOfTheChange {
    pub before: Vec<FileNames>,
    pub first: Vec<FileNames>,
    pub retry: Vec<FileNames>,
}

/// The calls and definitions the first answer added (it has more of them, over all the
/// change's files, than the files had before) and the retry has fewer of: one drop for each
/// name, placed in the first file that lost it, and one drop per line (the one that starts
/// first, then the shortest name), so `std::fs::write(name, format!(…)).ok()` is one drop,
/// `std::fs::write`. A call moved from one file of the change to another is not a drop, and
/// neither is a placeholder the first answer left alone and the retry filled.
///
/// Example: the first answer's `seat.rs` calls `std::fs::write` once; the retry's `seat.rs`
/// calls `self.log.append` and its `log.rs` calls `std::fs::write`: nothing is dropped. Had the
/// retry only removed the call, `std::fs::write in seat.rs` would be.
pub fn dropped_by_retry(change: &NamesOfTheChange) -> Vec<Dropped> {
    let in_before = counted(&change.before);
    let in_first = counted(&change.first);
    let in_retry = counted(&change.retry);
    let fewer_in = |counts: &BTreeMap<NameKey, usize>, key: &NameKey, count: usize| {
        counts.get(key).copied().unwrap_or(0) < count
    };
    let all: Vec<Dropped> = in_first
        .iter()
        .filter(|(key, count)| fewer_in(&in_before, key, **count))
        .filter(|(key, count)| fewer_in(&in_retry, key, **count))
        .filter_map(|((kind, name), _)| first_place_lost(*kind, name, &change.first, &change.retry))
        .collect();
    one_per_line(all)
}

/// The drops with one kept on each line of each file, the one that starts first (the shortest
/// name of those), in file order.
fn one_per_line(drops: Vec<Dropped>) -> Vec<Dropped> {
    let mut by_line: BTreeMap<(String, u32), Dropped> = BTreeMap::new();
    for drop in drops {
        let place = (drop.file.clone(), drop.line);
        match by_line.get(&place) {
            Some(kept) if (kept.column, kept.name.len()) <= (drop.column, drop.name.len()) => {}
            _ => {
                by_line.insert(place, drop);
            }
        }
    }
    by_line.into_values().collect()
}

type NameKey = (NameKind, String);

fn counted(files: &[FileNames]) -> BTreeMap<NameKey, usize> {
    files
        .iter()
        .flat_map(|file| &file.names)
        .fold(BTreeMap::new(), |mut counts, named| {
            *counts.entry((named.kind, named.name.clone())).or_insert(0) += 1;
            counts
        })
}

/// Where the first answer had the name in a file that has fewer of it in the retry; the first
/// place it had it at all when no one file lost it.
fn first_place_lost(
    kind: NameKind,
    name: &str,
    first: &[FileNames],
    retry: &[FileNames],
) -> Option<Dropped> {
    let count_in = |file: &FileNames| {
        file.names
            .iter()
            .filter(|named| named.kind == kind && named.name == name)
            .count()
    };
    let retry_count = |path: &str| {
        retry
            .iter()
            .find(|file| file.path == path)
            .map_or(0, count_in)
    };
    let file = first
        .iter()
        .find(|file| count_in(file) > retry_count(&file.path))
        .or_else(|| first.iter().find(|file| count_in(file) > 0))?;
    let named = file
        .names
        .iter()
        .find(|named| named.kind == kind && named.name == name)?;
    Some(Dropped {
        kind,
        name: name.to_string(),
        file: file.path.clone(),
        line: named.line,
        column: named.column,
        line_text: file
            .text
            .lines()
            .nth(named.line.saturating_sub(1) as usize)
            .unwrap_or("")
            .trim()
            .to_string(),
    })
}

/// What Jev reads about the retry, shared by every drop's question.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryAsked {
    /// Each rule the first answer broke, with its line: one per line.
    pub rules: String,
    /// The retry's goal.
    pub goal: String,
    /// The first answer, as a diff of the files before the change.
    pub first_diff: String,
    /// The retry, as a diff of the files before the change.
    pub retry_diff: String,
}

/// One question per drop (at most [`MAX_DROPS_ASKED`]), each a choice between the rule asking
/// for the removal, the code moving elsewhere, and behaviour the goal needs being lost. They
/// share the rules, the goal and both diffs, so they go to Jev as one call.
///
/// Example: the drop `std::fs::write in seat.rs`, after the rule `commands-use-ports`, is asked
/// as `retry-drop-0` with the context `dropped-0` = ``the call `std::fs::write` at
/// src/seat.rs:36: `std::fs::write("walk-ins.txt", line).ok();` ``.
pub fn drop_questions(drops: &[Dropped], asked: &RetryAsked) -> Vec<Question> {
    drops
        .iter()
        .take(MAX_DROPS_ASKED)
        .enumerate()
        .map(|(n, dropped)| Question {
            kind: format!("retry-drop-{n}"),
            instructions: format!(
                "A code-writing model was asked to take a step (`goal`). Its first answer (`first_answer`, a diff of the files before the change) broke the project's checked rules (`rules`), so it was asked again; its second answer is `retry` (a diff of the same files). The second answer no longer has the code in `dropped-{n}` anywhere in the change. Why is it gone? Read the lines `retry` removes and adds, including placeholders that said what the code should do."
            ),
            shape: Shape::Choice(why_gone_options()),
            context: vec![
                ("goal".into(), asked.goal.clone()),
                ("rules".into(), asked.rules.clone()),
                ("first_answer".into(), shortened(&asked.first_diff)),
                ("retry".into(), shortened(&asked.retry_diff)),
                (format!("dropped-{n}"), dropped_text(dropped)),
            ],
            unsure: UnsureAnswer::NotAllowed,
        })
        .collect()
}

const RULE_ASKED: &str = "rule_asked_for_removal";
const MOVED: &str = "moved_elsewhere";
const LOST: &str = "behaviour_lost";

fn why_gone_options() -> Vec<(String, String)> {
    vec![
        (
            RULE_ASKED.into(),
            "The code was not needed: what it did is not part of the step (a stray debug line, a leftover, a duplicate, an unasked extra). A rule that says where such work belongs (a port, an adapter) does not make the work unneeded: that is moved_elsewhere or behaviour_lost.".into(),
        ),
        (
            MOVED.into(),
            "What it did is still done in `retry`, by other code that does the same job: a method on an interface (a port) and its implementation, an adapter, or a rewritten call. Name the code in `retry` that does it.".into(),
        ),
        (
            LOST.into(),
            "What it did (writing a file, saving, sending, recording, logging, checking) is work the code was meant to do (the goal, or the code before the change, asks for it), and nothing in `retry` does that job any more: behaviour was lost.".into(),
        ),
    ]
}

/// ``the call `std::fs::write` at src/seat.rs:36: `std::fs::write(…)` ``.
fn dropped_text(dropped: &Dropped) -> String {
    let what = match dropped.kind {
        NameKind::Call => "the call",
        NameKind::Definition => "the definition",
    };
    format!(
        "{what} `{}` at {}:{}: `{}`",
        dropped.name, dropped.file, dropped.line, dropped.line_text
    )
}

/// The diff cut to [`MAX_DIFF_BYTES`] at a line's end, saying so.
fn shortened(diff: &str) -> String {
    if diff.len() <= MAX_DIFF_BYTES {
        return diff.to_string();
    }
    let cut = diff[..MAX_DIFF_BYTES].rfind('\n').unwrap_or(0);
    format!("{}\n(the rest of the diff is left out)\n", &diff[..cut])
}

/// The verdict a choice's answer gives: legitimate when at least 0.7 of it is on the rule or on
/// the move (the likelier names the reason), unintended when at most 0.3 is, unsure between.
/// An answer with no probabilities is unsure.
///
/// Example: `rule_asked_for_removal` 0.1, `moved_elsewhere` 0.15, `behaviour_lost` 0.75 is
/// `LooksUnintended`; 0.3, 0.3, 0.4 is `Unsure`.
pub fn verdict_of(weighed: &Weighed) -> DropVerdict {
    let p = |option: &str| {
        weighed
            .distribution
            .iter()
            .find(|(name, _)| name == option)
            .map_or(0.0, |(_, p)| *p)
    };
    if weighed.distribution.is_empty() || matches!(weighed.answer, Answer::Unsure) {
        return DropVerdict::Unsure;
    }
    let (rule, moved) = (p(RULE_ASKED), p(MOVED));
    let legitimate = rule + moved;
    if legitimate >= LEGITIMATE_AT {
        DropVerdict::Legitimate(if moved >= rule {
            WhyRemoved::MovedElsewhere
        } else {
            WhyRemoved::RuleAskedForIt
        })
    } else if legitimate <= UNINTENDED_AT {
        DropVerdict::LooksUnintended
    } else {
        DropVerdict::Unsure
    }
}

/// `dropped: std::fs::write in seat.rs (looks unintended)`: the drops the step message says,
/// joined; empty when there are none.
pub fn said_drops_text(drops: &[CheckedDrop]) -> String {
    let said: Vec<String> = drops
        .iter()
        .filter(|drop| drop.verdict.is_said())
        .map(CheckedDrop::text)
        .collect();
    if said.is_empty() {
        return String::new();
    }
    format!("dropped: {}", said.join(", "))
}

/// What the step message adds after a rules retry, each part after ` · `: `asked again for
/// commands-use-ports · dropped: std::fs::write in close_table.rs (looks unintended)`. Empty
/// when conjure did not ask again; a drop Jev calls legitimate is not said here.
pub fn retry_step_text(rules: &RulesOutcome, drops: &[CheckedDrop]) -> String {
    let RulesOutcome::AskedAgain(broken) = rules else {
        return String::new();
    };
    let mut ids: Vec<&str> = Vec::new();
    for finding in broken {
        if !ids.contains(&finding.rule.as_str()) {
            ids.push(&finding.rule);
        }
    }
    let said = said_drops_text(drops);
    let dropped = if said.is_empty() {
        String::new()
    } else {
        format!(" \u{b7} {said}")
    };
    format!(" \u{b7} asked again for {}{dropped}", ids.join(", "))
}

/// `dropped on purpose: std::fs::write in seat.rs (moved elsewhere)`: the drops said quietly;
/// empty when there are none.
pub fn quiet_drops_text(drops: &[CheckedDrop]) -> String {
    let quiet: Vec<String> = drops
        .iter()
        .filter(|drop| !drop.verdict.is_said())
        .map(CheckedDrop::text)
        .collect();
    if quiet.is_empty() {
        return String::new();
    }
    format!("dropped on purpose: {}", quiet.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sorcerer_models::domain::decide::Confidence;

    fn call(name: &str, line: u32) -> NamedInText {
        call_at(name, line, 0)
    }

    fn call_at(name: &str, line: u32, column: u32) -> NamedInText {
        NamedInText {
            kind: NameKind::Call,
            name: name.into(),
            line,
            column,
        }
    }

    fn file(path: &str, text: &str, names: Vec<NamedInText>) -> FileNames {
        FileNames {
            path: path.into(),
            text: text.into(),
            names,
        }
    }

    #[test]
    fn a_call_the_retry_removed_is_dropped_at_its_line() {
        let first = [file(
            "src/seat.txt",
            "fn seat() {\n    write(\"a\");\n}\n",
            vec![call("write", 2)],
        )];
        let retry = [file("src/seat.txt", "fn seat() {\n}\n", vec![])];
        let dropped = dropped_by_retry(&NamesOfTheChange {
            before: vec![],
            first: first.to_vec(),
            retry: retry.to_vec(),
        });
        assert_eq!(dropped.len(), 1);
        assert_eq!(
            (
                dropped[0].name.as_str(),
                dropped[0].line,
                dropped[0].line_text.as_str()
            ),
            ("write", 2, "write(\"a\");")
        );
    }

    #[test]
    fn a_call_moved_into_another_file_of_the_change_is_not_dropped() {
        let first = [
            file("src/seat.txt", "write()\n", vec![call("write", 1)]),
            file("src/log.txt", "", vec![]),
        ];
        let retry = [
            file(
                "src/seat.txt",
                "log.append()\n",
                vec![call("log.append", 1)],
            ),
            file("src/log.txt", "write()\n", vec![call("write", 1)]),
        ];
        assert!(
            dropped_by_retry(&NamesOfTheChange {
                before: vec![],
                first: first.to_vec(),
                retry: retry.to_vec()
            })
            .is_empty()
        );
    }

    #[test]
    fn calls_dropped_on_one_line_are_one_drop_by_the_first_and_shortest_name() {
        let first = [file(
            "a",
            "w().ok()\n",
            vec![call("w().ok", 1), call("w", 1), call_at("f", 1, 2)],
        )];
        let retry = [file("a", "", vec![])];
        let dropped = dropped_by_retry(&NamesOfTheChange {
            before: vec![],
            first: first.to_vec(),
            retry: retry.to_vec(),
        });
        assert_eq!(dropped.len(), 1);
        assert_eq!(
            dropped[0].name, "w",
            "not `f`, an argument that starts later"
        );
    }

    #[test]
    fn a_call_the_files_had_before_is_not_the_first_answers_to_drop() {
        let had = [file("a", "todo()\n", vec![call("todo", 1)])];
        let names = NamesOfTheChange {
            before: had.to_vec(),
            first: had.to_vec(),
            retry: vec![file("a", "", vec![])],
        };
        assert!(dropped_by_retry(&names).is_empty());
    }

    #[test]
    fn one_of_two_same_calls_gone_is_one_drop() {
        let first = [file("a", "w()\nw()\n", vec![call("w", 1), call("w", 2)])];
        let retry = [file("a", "w()\n", vec![call("w", 1)])];
        assert_eq!(
            dropped_by_retry(&NamesOfTheChange {
                before: vec![],
                first: first.to_vec(),
                retry: retry.to_vec()
            })
            .len(),
            1
        );
    }

    fn weighed(rule: f64, moved: f64, lost: f64) -> Weighed {
        Weighed {
            answer: Answer::Chosen(LOST.into(), Confidence::Known(lost)),
            distribution: vec![
                (RULE_ASKED.into(), rule),
                (MOVED.into(), moved),
                (LOST.into(), lost),
            ],
        }
    }

    #[test]
    fn the_verdict_sums_both_legitimate_reasons_against_the_loss() {
        assert_eq!(
            verdict_of(&weighed(0.1, 0.15, 0.75)),
            DropVerdict::LooksUnintended
        );
        assert_eq!(verdict_of(&weighed(0.3, 0.3, 0.4)), DropVerdict::Unsure);
        assert_eq!(
            verdict_of(&weighed(0.35, 0.4, 0.25)),
            DropVerdict::Legitimate(WhyRemoved::MovedElsewhere)
        );
        assert_eq!(verdict_of(&Weighed::unsure()), DropVerdict::Unsure);
    }

    #[test]
    fn only_drops_that_are_not_legitimate_are_said() {
        let drop = |verdict| CheckedDrop {
            dropped: Dropped {
                kind: NameKind::Call,
                name: "std::fs::write".into(),
                file: "src/commands/close_table.txt".into(),
                line: 3,
                line_text: String::new(),
                column: 0,
            },
            verdict,
            rules: vec![],
        };
        let drops = [
            drop(DropVerdict::LooksUnintended),
            drop(DropVerdict::Legitimate(WhyRemoved::MovedElsewhere)),
        ];
        assert_eq!(
            said_drops_text(&drops),
            "dropped: std::fs::write in close_table.txt (looks unintended)"
        );
        assert_eq!(
            quiet_drops_text(&drops),
            "dropped on purpose: std::fs::write in close_table.txt (moved elsewhere)"
        );
    }
}
