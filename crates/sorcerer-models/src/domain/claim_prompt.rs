//! The claim checker's texts: the prompt a researcher checks a claim with,
//! the parser for its reply, and the three questions Jev guards the
//! verdict with. The texts are what the prose corpus measured
//! (`tests/golden/claim-prompt.txt`): change them only against the record.
//!
//! The prompt asks the researcher to cite the code its verdict rests on,
//! each place as a file, a unit (a function, class or method, in the words
//! of the file's language) and a line. It asks for the line too because a
//! unit name that is not found in the file falls back to the line
//! (`confirm_one` in `check_claim`).

use std::path::Path;

use serde_json::Value;
use sorcerer_core::domain::language::Languages;

use crate::domain::claim::{Claim, Verdict};
use crate::domain::decide::{Question, Shape, UnsureAnswer};

/// How a unit is named when no language says: the claim's files have no
/// language, or theirs has no `unit_words`.
pub const NEUTRAL_UNIT_WORDS: &str = "a function, class or method name";

/// The prompt a researcher is given to check a claim against the code.
///
/// It reads, in order: the task and the claim's sentence; a hint, when the
/// claim points at a place; how to look (Read, Grep, Glob, every layer of
/// the project); the JSON answer to give; and what a "unit" in that answer
/// is, in the words of the language the claim's files are in.
pub fn claim_prompt(claim: &Claim, languages: Option<&Languages>) -> String {
    let lines: Vec<String> = [
        task_lines(claim),
        hint_line(claim).into_iter().collect(),
        how_to_check_lines(),
        answer_shape_lines(),
        vec![unit_sentence(claim, languages)],
    ]
    .concat();
    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn task_lines(claim: &Claim) -> Vec<String> {
    vec![
        "Check one sentence of a software plan against this repository (the current directory)."
            .into(),
        String::new(),
        "Section: (none)".into(),
        format!("Sentence: {}", claim.statement),
    ]
}

/// The first place the claim points at, offered as where to start looking.
fn hint_line(claim: &Claim) -> Option<String> {
    let place = claim.about.first()?;
    Some(format!(
        "Hint: the writer pointed at `{}:{}`: a place to start, not the answer.",
        place.file.display(),
        place.lines.start
    ))
}

fn how_to_check_lines() -> Vec<String> {
    [
        "",
        "Use Read, Grep and Glob to find the code the sentence is about, and read it.",
        "Behaviour can live in any layer: server code, templates, and front-end scripts and styles. Before you conclude that something does not exist, search for it across the whole repository.",
        "Rule: judge what the sentence says is true of the code now; if it also wishes or asks for something, ignore that part.",
        "If the code cannot settle it (it is about people, documents or outside services), the verdict is cannot_tell.",
    ]
    .map(String::from)
    .to_vec()
}

const EVIDENCE_SHAPE: &str = r#"{"path": "<path relative to the current directory>", "unit": "<unit name, or null>", "line": <line number, or null>}"#;

fn answer_shape_lines() -> Vec<String> {
    vec![
        String::new(),
        "Answer with one fenced ```json block holding one object, after any notes:".into(),
        "```json".into(),
        format!(
            r#"{{"verdict": "right | wrong | cannot_tell", "why": "<one sentence>", "evidence": [{EVIDENCE_SHAPE}]}}"#
        ),
        "```".into(),
        "For right or wrong, give at least one evidence entry: the code the verdict rests on."
            .into(),
    ]
}

/// The sentence saying what "unit" means in the answer, in the words of the
/// languages the claim's files are in.
///
/// Example: a claim about `orders.py` alone gets Python's words ("a
/// function, class or method name ..."); a claim about `orders.py` and
/// `cart.ts` gets each language's words after its files ("in `orders.py`,
/// ...; in `cart.ts`, ..."); a claim about no file gets the neutral words.
fn unit_sentence(claim: &Claim, languages: Option<&Languages>) -> String {
    let head = r#"In evidence, "unit" is"#;
    let tail = "For anything else (a template, a heading, part of a function, several names) give null and the line. Give the line too, so the citation can be checked if the name is not found.";
    match files_by_unit_words(claim, languages).as_slice() {
        [] => {
            format!("{head} {NEUTRAL_UNIT_WORDS}, exactly as the file defines it, or null. {tail}")
        }
        [only] => format!(
            "{head} {}, exactly as the file defines it, or null. {tail}",
            only.unit_words
        ),
        several => {
            let each: Vec<String> = several
                .iter()
                .map(|group| format!("in {}, {}", group.files.join(", "), group.unit_words))
                .collect();
            format!(
                "{head} one name exactly as the file defines it, or null: {}. {tail}",
                each.join("; ")
            )
        }
    }
}

/// Files whose languages name a unit in the same words.
struct FilesNamingUnitsAlike {
    unit_words: String,
    /// Each file once, in backticks, in the order the claim lists them.
    files: Vec<String>,
}

/// The claim's files grouped by how their language names a unit, each
/// wording once, in the order the claim first mentions it.
fn files_by_unit_words(claim: &Claim, languages: Option<&Languages>) -> Vec<FilesNamingUnitsAlike> {
    claim.about.iter().fold(Vec::new(), |mut groups, place| {
        let unit_words = unit_words_for(&place.file, languages);
        let file = format!("`{}`", place.file.display());
        match groups
            .iter_mut()
            .find(|group: &&mut FilesNamingUnitsAlike| group.unit_words == unit_words)
        {
            Some(group) if group.files.contains(&file) => {}
            Some(group) => group.files.push(file),
            None => groups.push(FilesNamingUnitsAlike {
                unit_words,
                files: vec![file],
            }),
        }
        groups
    })
}

/// How the language of a file names a unit, or the neutral words when the
/// file has no language or its language does not say.
fn unit_words_for(file: &Path, languages: Option<&Languages>) -> String {
    languages
        .and_then(|languages| languages.for_path(file))
        .and_then(|language| language.unit_words.clone())
        .unwrap_or_else(|| NEUTRAL_UNIT_WORDS.to_string())
}

/// A place the researcher cited, as its reply gave it, before anyone has
/// looked for it in the project. Example: `orders.py`, unit `cancel`, line 42.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCitation {
    pub path: String,
    pub unit: Option<String>,
    pub line: Option<u32>,
}

/// The researcher's answer, read from its reply.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedReply {
    pub verdict: Verdict,
    pub why: String,
    pub citations: Vec<RawCitation>,
}

/// Reads the researcher's answer from its reply text.
///
/// The answer is a JSON object: the last ```json block in the reply, or,
/// with none, the first `{` from which the rest of the reply reads as an
/// object. The verdict may be written loosely ("Cannot tell" reads as
/// `cannot_tell`). The reply is refused, with the reason, when it holds no
/// object, an unknown verdict, or a right or wrong verdict that cites no
/// code.
pub fn parse_reply(text: &str) -> Result<ParsedReply, String> {
    let answer = answer_object(text)?;
    let verdict = verdict_in(&answer)?;
    let why = why_in(&answer);
    let citations = citations_in(&answer);
    if verdict != Verdict::CannotTell && citations.is_empty() {
        return Err("a verdict with no evidence".into());
    }
    Ok(ParsedReply {
        verdict,
        why,
        citations,
    })
}

type JsonObject = serde_json::Map<String, Value>;

fn answer_object(text: &str) -> Result<JsonObject, String> {
    let value = last_fenced_json(text)
        .or_else(|| first_bare_json_object(text))
        .ok_or("no JSON object in the reply")?;
    match value {
        Value::Object(object) => Ok(object),
        _ => Err("the reply's JSON is not an object".into()),
    }
}

fn last_fenced_json(text: &str) -> Option<Value> {
    let after_last_fence = text.split("```json").skip(1).last()?;
    serde_json::from_str(after_last_fence.split("```").next()?).ok()
}

/// The first `{` from which the text up to its last `}` reads as an object.
fn first_bare_json_object(text: &str) -> Option<Value> {
    text.char_indices()
        .filter(|(_, character)| *character == '{')
        .find_map(|(start, _)| {
            let rest = &text[start..];
            let end = rest.rfind('}')?;
            serde_json::from_str::<Value>(&rest[..=end])
                .ok()
                .filter(Value::is_object)
        })
}

fn verdict_in(answer: &JsonObject) -> Result<Verdict, String> {
    let written = answer
        .get("verdict")
        .and_then(Value::as_str)
        .ok_or("no verdict")?
        .trim()
        .replace(' ', "_")
        .to_lowercase();
    match written.as_str() {
        "right" => Ok(Verdict::Right),
        "wrong" => Ok(Verdict::Wrong),
        "cannot_tell" => Ok(Verdict::CannotTell),
        _ => Err(format!("unknown verdict `{written}`")),
    }
}

fn why_in(answer: &JsonObject) -> String {
    answer
        .get("why")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The answer's evidence entries that name a file; the rest are dropped.
fn citations_in(answer: &JsonObject) -> Vec<RawCitation> {
    answer
        .get("evidence")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(citation_in).collect())
        .unwrap_or_default()
}

/// One evidence entry as a citation. Models often put the line or the unit
/// on the path, so `orders.py:42` is read as line 42 and `orders.py:cancel`
/// as unit `cancel`; the entry's own `unit` and `line` win over the path's.
fn citation_in(entry: &Value) -> Option<RawCitation> {
    let (path, suffix) = path_and_suffix(entry.get("path")?.as_str()?.trim());
    if path.is_empty() {
        return None;
    }
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    let unit = match entry.get("unit").and_then(Value::as_str).map(str::trim) {
        Some(unit) if !unit.is_empty() => Some(unit.to_string()),
        _ => suffix.filter(|s| !all_digits(s)).map(String::from),
    };
    let line = match entry.get("line").and_then(Value::as_f64) {
        Some(number) => Some(number.round() as u32),
        None => suffix
            .filter(|s| all_digits(s))
            .and_then(|s| s.parse().ok()),
    };
    Some(RawCitation {
        path: path.into(),
        unit,
        line,
    })
}

/// Splits `orders.py:42` into the path and what follows its last `:`. A
/// colon that cannot start a suffix is part of the path: one at the start
/// or second place (`C:`), at the end, or followed by a `/`.
fn path_and_suffix(written: &str) -> (&str, Option<&str>) {
    match written.rsplit_once(':') {
        Some((before, after)) if before.len() > 1 && !after.is_empty() && !after.contains('/') => {
            (before, Some(after))
        }
        _ => (written, None),
    }
}

/// The three questions Jev answers about a checker's verdict, all asked in
/// one request: does its reason speak to the claim (`why-fits`), does the
/// code it cites (`evidence` is that code, already read) fit the claim
/// (`evidence-fits`), and does its verdict follow from its reason
/// (`verdict-follows`)? The first model's verdict stands only when Jev says
/// yes to all three.
pub fn guard_questions(
    claim: &Claim,
    verdict: Verdict,
    why: &str,
    evidence: &str,
) -> Vec<Question> {
    let shared_context = guard_context(claim, verdict, why, evidence);
    let yes_or_no = |kind: &str, instructions: &str, yes: &str, no: &str| Question {
        kind: kind.into(),
        instructions: instructions.into(),
        shape: Shape::Noul {
            yes: yes.into(),
            no: no.into(),
        },
        context: shared_context.clone(),
        unsure: UnsureAnswer::NotAllowed,
    };
    vec![
        yes_or_no("why-fits", WHY_INSTRUCTIONS, WHY_TRUE, WHY_FALSE),
        yes_or_no("evidence-fits", EV_INSTRUCTIONS, EV_TRUE, EV_FALSE),
        yes_or_no(
            "verdict-follows",
            FOLLOWS_INSTRUCTIONS,
            FOLLOWS_TRUE,
            FOLLOWS_FALSE,
        ),
    ]
}

/// What the three guard questions are about, by name: the claim's
/// sentence, the kind of check, the verdict and what it means, the reason,
/// and the cited code.
fn guard_context(
    claim: &Claim,
    verdict: Verdict,
    why: &str,
    evidence: &str,
) -> Vec<(String, String)> {
    let evidence = if evidence.is_empty() {
        "(nothing cited)"
    } else {
        evidence
    };
    vec![
        ("sentence".to_string(), claim.statement.clone()),
        ("check".to_string(), CHECK_WORDS.to_string()),
        ("verdict".to_string(), verdict_words(verdict).to_string()),
        ("why".to_string(), format!("Reason: {why}")),
        ("evidence".to_string(), evidence.to_string()),
    ]
}

const CHECK_WORDS: &str = "a statement checked against the project's code";

/// A verdict and what it means, as the guards are shown it.
pub fn verdict_words(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Right => "right: what the sentence states is true",
        Verdict::Wrong => "wrong: what the sentence states is false",
        Verdict::CannotTell => "cannot_tell",
    }
}

const WHY_INSTRUCTIONS: &str = "A checker read a software project and judged one sentence of the project's plan; `check` says what kind of check it was, `verdict` its verdict, `why` its reason (and answer), `evidence` what it cites. Does the checker's reason speak to what the sentence itself states, asks, or asks to be done?";
const WHY_TRUE: &str = "the reason addresses the very thing the sentence says, asks or asks for: the same thing, behaviour and condition, read the way the writer most likely meant it";
const WHY_FALSE: &str = "the reason addresses something else: another component or behaviour, a reframed or narrower question, or only part of what the sentence says while the verdict covers all of it";

const EV_INSTRUCTIONS: &str = "A checker read a software project and judged one sentence of the project's plan; `evidence` is the code it cites (each citation's path, unit and cited line, with the lines around that line). Is the cited code what the sentence is about: where what it states would be true or false, what it asks about, or what the step would change or has changed?";
const EV_TRUE: &str = "the cited code is where the behaviour, data or feature the sentence names lives, or plainly shows it (or, for a step not done yet, the code it would change)";
const EV_FALSE: &str = "the cited code is beside the point: another feature, a similarly named thing, a caller or test that does not show what the reason claims; or nothing is cited for a verdict that rests on code";

const FOLLOWS_INSTRUCTIONS: &str = "A checker judged one sentence of a software plan; `verdict` is its verdict, with what the verdict means, and `why` is its reason (and answer). Does the verdict follow from the checker's own reason?";
const FOLLOWS_TRUE: &str = "if the reason is true, it establishes this verdict and not another: e.g. a reason showing the statement false with the verdict wrong, a reason showing all of the step in the code with the verdict done, an answer the reason supports";
const FOLLOWS_FALSE: &str = "the reason supports another verdict, hedges, or contradicts the verdict: e.g. the reason says the code does part of it but the verdict is done, the reason finds the statement true but the verdict is wrong, or the answer says more than the reason shows";

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_fenced_block_wins_and_suffixes_are_read() {
        let r = parse_reply("```json\n{\"verdict\": \"wrong\", \"why\": \"a\", \"evidence\": [{\"path\": \"x:1\"}]}\n```\nthen\n```json\n{\"verdict\": \"Right\", \"why\": \" b \", \"evidence\": [{\"path\": \"src/a.txt:cancel\"}, {\"path\": \"src/b.txt:12\"}]}\n```").unwrap();
        assert_eq!((r.verdict, r.why.as_str()), (Verdict::Right, "b"));
        assert_eq!(
            r.citations[0],
            RawCitation {
                path: "src/a.txt".into(),
                unit: Some("cancel".into()),
                line: None
            }
        );
        assert_eq!(
            r.citations[1],
            RawCitation {
                path: "src/b.txt".into(),
                unit: None,
                line: Some(12)
            }
        );
    }

    #[test]
    fn a_bare_object_is_read() {
        let r = parse_reply("I think {\"verdict\": \"cannot tell\", \"why\": \"x\"} done").unwrap();
        assert_eq!(r.verdict, Verdict::CannotTell);
    }

    #[test]
    fn an_unknown_verdict_is_refused() {
        assert!(
            parse_reply("{\"verdict\": \"maybe\"}")
                .unwrap_err()
                .contains("maybe")
        );
    }
}
