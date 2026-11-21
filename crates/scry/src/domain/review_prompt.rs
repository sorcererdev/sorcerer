//! The review call as data: what the reviewer is asked (a batch of changed
//! units, the diff, the project's steering and the open findings to
//! answer), the prompt that asks it, and reading the findings out of its
//! reply.
//!
//! The prompt's words are the ones the eval measured: changing a word
//! changes what the corpus measured, so the golden
//! `tests/golden/review-prompt.txt` pins them.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Map, Value};
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Span;

use crate::domain::finding::Finding;
use crate::domain::issue_type::ISSUE_TYPES;

/// How long a review call may take: 7 minutes.
pub const REVIEW_TIMEOUT: Duration = Duration::from_secs(420);

/// How many numbered lines one review call may carry. A longer unit goes
/// in a call of its own.
const BATCH_LINES: u32 = 600;

/// A unit to review. `in_progress`: it probably stops before doing its job,
/// so it is reviewed for what is written, not for what is missing.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewUnit {
    pub file: PathBuf,
    pub name: String,
    pub lines: Span,
    pub header: Option<String>,
    pub text: String,
    pub in_progress: bool,
}

#[derive(Debug, Clone)]
pub struct ReviewAsk {
    pub units: Vec<ReviewUnit>,
    pub diff: String,
    pub steering: String,
    /// Always empty in v1 (spellbook's slot).
    pub reported: Vec<String>,
    pub open: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawFinding {
    pub issue_type: String,
    pub file: PathBuf,
    pub lines: Span,
    pub statement: String,
    pub rule: Option<String>,
    pub evidence: Vec<(PathBuf, Span)>,
}

/// The reviewer's answer about a finding still open from an earlier save.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenAnswer {
    pub id: String,
    pub holds: Holds,
    /// One line on why.
    pub why: String,
}

/// Whether the code still has an open finding's problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holds {
    /// "still holds": the problem is still there.
    Still,
    /// "no longer holds": the code no longer has it.
    NoLonger,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReviewReply {
    pub findings: Vec<RawFinding>,
    pub open: Vec<OpenAnswer>,
}

/// The prompt for one review call. In order, it tells the reviewer:
///
/// 1. that a developer just saved a change;
/// 2. the changed units, each with its lines numbered as in the file;
/// 3. the diff;
/// 4. the project's steering;
/// 5. the findings already reported, not to raise again;
/// 6. the findings still open on these units, each to answer "still holds"
///    or "no longer holds";
/// 7. when a unit is still being written, to report only what is there;
/// 8. what to report, the four issue types, and the JSON to reply with.
pub fn review_prompt(ask: &ReviewAsk) -> String {
    [
        "A developer has just saved a change in this repository (the current directory is the tree after the change).\n\n",
        &changed_units_section(&ask.units),
        &diff_section(&ask.diff),
        &steering_section(&ask.steering),
        &reported_section(&ask.reported),
        &open_findings_section(&ask.open),
        in_progress_note(&ask.units),
        WHAT_TO_REPORT,
        &issue_types_text(),
        REPLY_FORMAT,
    ]
    .concat()
}

fn changed_units_section(units: &[ReviewUnit]) -> String {
    let units = list_or(units.iter().map(numbered_unit), "\n\n", "(none)");
    format!(
        "The changed units (functions and methods this change adds or changes), with line numbers as in the file after the change:\n{units}\n\n"
    )
}

/// A unit as the reviewer reads it: a heading line, then each line of code
/// numbered.
///
/// Example:
/// ```text
/// --- src/a.py:3-5 A.f (in `class A:`)
///     3  def f(self, x):
///     4      return x
/// ```
fn numbered_unit(unit: &ReviewUnit) -> String {
    let enclosing = match unit.header.as_deref() {
        Some(header) if !header.is_empty() => format!(" (in `{header}`)"),
        _ => String::new(),
    };
    let in_progress = if unit.in_progress {
        " (in progress)"
    } else {
        ""
    };
    let heading = format!(
        "--- {}:{}-{} {}{enclosing}{in_progress}",
        forward_slashes(&unit.file),
        unit.lines.start,
        unit.lines.end,
        unit.name
    );
    let code = unit
        .text
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{:>5}  {line}", unit.lines.start as usize + i));
    std::iter::once(heading)
        .chain(code)
        .collect::<Vec<_>>()
        .join("\n")
}

fn diff_section(diff: &str) -> String {
    format!(
        "The change as a diff (tests included):\n{}\n\n",
        diff.trim_end()
    )
}

fn steering_section(steering: &str) -> String {
    format!(
        "The project's steering, which holds the rules this project keeps:\n<steering>\n{}\n</steering>\n\n",
        steering.trim_end()
    )
}

fn reported_section(reported: &[String]) -> String {
    let reported = list_or(reported.iter().map(|r| format!("- {r}")), "\n", "none");
    format!(
        "Findings already reported on these files, which you must not raise again:\n{reported}\n\n"
    )
}

/// The open findings, and, when there are any, how to answer them.
fn open_findings_section(open: &[Finding]) -> String {
    let lines = list_or(open.iter().map(open_finding_line), "\n", "none");
    let how_to_answer = if open.is_empty() {
        ""
    } else {
        "\nAnswer every open finding in \"open\": \"still holds\" when the code still has that problem, \"no longer holds\" when the code no longer has it, with one line why. Do not raise an open finding again in \"findings\".\n"
    };
    format!("Open findings from earlier saves on these units:\n{lines}\n{how_to_answer}\n")
}

/// Example: `- id abc123: [bug] src/a.py:3: It returns x unchanged.`
fn open_finding_line(finding: &Finding) -> String {
    format!(
        "- id {}: [{}] {}:{}: {}",
        finding.id.0,
        finding.issue_type,
        forward_slashes(&finding.place.file),
        finding.place.lines.start,
        finding.statement
    )
}

fn in_progress_note(units: &[ReviewUnit]) -> &'static str {
    if units.iter().any(|u| u.in_progress) {
        "Units marked (in progress) are still being written: report only rule breaks and bugs in the code that is there, never that something is missing or not done yet.\n"
    } else {
        ""
    }
}

/// The items joined with `separator`, or `empty` when there are none.
fn list_or(items: impl Iterator<Item = String>, separator: &str, empty: &str) -> String {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        empty.to_string()
    } else {
        items.join(separator)
    }
}

const WHAT_TO_REPORT: &str = "Review the changed units. List problems this change has or introduces: nothing else. An empty list is a normal and often the right answer; do not look for something to say. Do not speculate: a finding needs a named code path (the line, the call and what happens), found with Read, Grep and Glob. Do not report style or taste unless it breaks a rule in the steering file.\n\n";

/// The issue types, each with the sentence that defines it.
fn issue_types_text() -> String {
    let types: String = ISSUE_TYPES
        .iter()
        .map(|known| format!("- {}: {}\n", known.name, known.sentence))
        .collect();
    format!("Issue types:\n{types}\n")
}

const REPLY_FORMAT: &str = r#"End your answer with one JSON object and nothing after it:
{"findings": [{"issue_type": "bug|assumption|convention|design",
   "file": "path/relative/to/repo", "lines": [first line, last line] (in the file after the change),
   "statement": "two to three sentences: what is wrong and why it matters",
   "rule": "the id of the rule it breaks, exactly as listed under Rules in the steering, or empty",
   "evidence": [{"file": "path/relative/to/repo", "lines": [first line, last line]}]}],
 "open": [{"id": "the open finding's id", "holds": "still holds|no longer holds", "why": "one line"}]}
"#;

/// The JSON in a model's reply, which may wrap it in prose or code fences.
/// It tries, in order, and takes the first that decodes:
///
/// 1. each piece between ``` fences, the last first (a leading `json` tag
///    is dropped), since the answer usually ends the reply;
/// 2. the longest text that starts at a `{` and ends at the last `}`, or
///    starts at a `[` and ends at the last `]`, and decodes. Longest, so a
///    bare list of objects is read as the list and not as its first object,
///    and so braces or brackets in the prose before the JSON (`Booked: {}`,
///    `#[should_panic]`) are skipped without picking up a small fragment
///    such as a trailing `[]`.
///
/// Example: `I found one problem: {"findings": [...]}` gives the object.
pub fn json_in(reply: &str) -> Option<Value> {
    json_in_fences(reply).or_else(|| json_between_brackets(reply))
}

fn json_in_fences(reply: &str) -> Option<Value> {
    if !reply.contains("```") {
        return None;
    }
    let pieces: Vec<&str> = reply.split("```").collect();
    pieces.into_iter().rev().find_map(|piece| {
        let piece = piece.trim();
        decode(piece.strip_prefix("json").unwrap_or(piece))
    })
}

fn json_between_brackets(reply: &str) -> Option<Value> {
    [('{', '}'), ('[', ']')]
        .into_iter()
        .flat_map(|(open, close)| json_from_each_opening_to_last_closing(reply, open, close))
        .max_by_key(|(text_len, _)| *text_len)
        .map(|(_, value)| value)
}

/// Every text in REPLY that starts at an OPEN, ends at the last CLOSE and
/// decodes, each with its length. Openings that do not decode (a stray `{}`
/// in the prose before the JSON) are skipped.
///
/// Example: for `Booked: {} for {}. {"a": 1}` the texts from the first two
/// `{` do not decode and the one from the third does, giving `{"a": 1}`.
fn json_from_each_opening_to_last_closing(
    reply: &str,
    open: char,
    close: char,
) -> Vec<(usize, Value)> {
    let Some(last) = reply.rfind(close) else {
        return Vec::new();
    };
    reply[..=last]
        .match_indices(open)
        .filter_map(|(first, _)| Some((last + 1 - first, decode(&reply[first..=last])?)))
        .collect()
}

fn decode(text: &str) -> Option<Value> {
    serde_json::from_str(text.trim()).ok()
}

/// The reviewer's reply read: its findings, and its answers about the open
/// findings. Its JSON is either `{"findings": [...], "open": [...]}` or a
/// bare list of findings. A finding or answer that is malformed (an
/// unknown issue type, no file, no lines) is left out rather than failing
/// the reply. The only error is a reply with no JSON at all.
pub fn parse_review(text: &str) -> Result<ReviewReply, String> {
    match json_in(text) {
        None => Err("no JSON in the review reply".into()),
        Some(Value::Array(items)) => Ok(ReviewReply {
            findings: items.iter().filter_map(finding_of).collect(),
            open: vec![],
        }),
        Some(Value::Object(object)) => Ok(ReviewReply {
            findings: array_or_empty(&object, "findings")
                .iter()
                .filter_map(finding_of)
                .collect(),
            open: array_or_empty(&object, "open")
                .iter()
                .filter_map(open_answer_of)
                .collect(),
        }),
        Some(_) => Ok(ReviewReply::default()),
    }
}

/// One finding of the reply, when it has a known issue type, a file,
/// lines and a statement. Its lines are a pair `"lines": [12, 14]`, or a
/// single `"line": 12`. Evidence entries without a file and lines are left
/// out.
fn finding_of(value: &Value) -> Option<RawFinding> {
    let object = value.as_object()?;
    let issue_type = non_blank_text(object, "issue_type")?;
    if !ISSUE_TYPES.iter().any(|known| known.name == issue_type) {
        return None;
    }
    Some(RawFinding {
        issue_type,
        file: PathBuf::from(non_blank_text(object, "file")?),
        lines: lines_of(object)?,
        statement: non_blank_text(object, "statement")?,
        rule: non_blank_text(object, "rule"),
        evidence: evidence_of(object),
    })
}

fn lines_of(object: &Map<String, Value>) -> Option<Span> {
    match object.get("lines") {
        Some(lines) => line_range(lines),
        None => {
            let line = whole_number(object.get("line")?)?;
            Some(Span {
                start: line,
                end: line,
            })
        }
    }
}

fn evidence_of(object: &Map<String, Value>) -> Vec<(PathBuf, Span)> {
    array_or_empty(object, "evidence")
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_object()?;
            let file = PathBuf::from(non_blank_text(entry, "file")?);
            Some((file, line_range(entry.get("lines")?)?))
        })
        .collect()
}

/// One answer about an open finding: its id (a string, or a number the
/// model wrote unquoted) and "still holds" or "no longer holds". Any other
/// answer is left out.
fn open_answer_of(value: &Value) -> Option<OpenAnswer> {
    let object = value.as_object()?;
    let id = match object.get("id")? {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => n.as_f64()?.to_string(),
        },
        _ => return None,
    };
    let holds = match object.get("holds")?.as_str()? {
        "still holds" => Holds::Still,
        "no longer holds" => Holds::NoLonger,
        _ => return None,
    };
    Some(OpenAnswer {
        id,
        holds,
        why: non_blank_text(object, "why").unwrap_or_default(),
    })
}

/// A line range written as a list: its first and last numbers.
/// `[12, 14]` is lines 12 to 14; `[12]` is line 12.
fn line_range(value: &Value) -> Option<Span> {
    let numbers = value.as_array()?;
    let start = whole_number(numbers.first()?)?;
    let end = whole_number(numbers.last()?)?;
    Some(Span { start, end })
}

/// A non-negative whole number, whether written `12` or `12.0`.
fn whole_number(value: &Value) -> Option<u32> {
    let Value::Number(n) = value else {
        return None;
    };
    n.as_u64().and_then(|x| u32::try_from(x).ok()).or_else(|| {
        n.as_f64()
            .filter(|f| f.fract() == 0.0 && *f >= 0.0 && *f <= f64::from(u32::MAX))
            .map(|f| f as u32)
    })
}

/// The key's text, trimmed, when it is a string that is not blank.
fn non_blank_text(object: &Map<String, Value>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

fn array_or_empty<'a>(object: &'a Map<String, Value>, key: &str) -> &'a [Value] {
    match object.get(key) {
        Some(Value::Array(items)) => items,
        _ => &[],
    }
}

/// Units in order, batched into calls of at most 600 numbered lines; a unit
/// longer than that goes alone.
pub fn batch_units(units: Vec<ReviewUnit>) -> Vec<Vec<ReviewUnit>> {
    batch_by_lines(units, |u| u.lines)
}

/// Items in order, grouped into batches of at most 600 lines, where `lines`
/// says which lines an item covers; an item longer than 600 lines goes in a
/// batch of its own.
///
/// Example: units of 300, 250 and 100 lines make two batches, [300, 250]
/// and [100]; units of 700 and 10 lines make [700] and [10].
pub fn batch_by_lines<U>(items: Vec<U>, lines: impl Fn(&U) -> Span) -> Vec<Vec<U>> {
    let line_count = |item: &U| {
        let span = lines(item);
        span.end.saturating_sub(span.start) + 1
    };
    let mut batches: Vec<Vec<U>> = vec![];
    let mut lines_in_last_batch = 0;
    for item in items {
        let item_lines = line_count(&item);
        match batches.last_mut() {
            Some(last) if lines_in_last_batch + item_lines <= BATCH_LINES => {
                last.push(item);
                lines_in_last_batch += item_lines;
            }
            _ => {
                batches.push(vec![item]);
                lines_in_last_batch = item_lines;
            }
        }
    }
    batches
}
