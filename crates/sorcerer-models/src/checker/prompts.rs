//! The claim checker's texts, ported word for word from olifant at commit
//! 127570e: the claim prompt (`claimPrompt` through `checkPrompt` in the
//! `CheckClaim` mode, `evidenceShape`) and its reply parser
//! (`parseCheckReply`) from `Server/Checker.hs`; the three guards'
//! instructions and sides (`guardQuestions`, `checkWords`, `verdictWords`)
//! from `Server/Cascade.hs`. They are what the corpus measured: change
//! them only against the record.

use serde_json::Value;

use super::{Claim, Verdict};
use crate::decide::{Question, Shape};

const EVIDENCE_SHAPE: &str = r#"{"path": "<path relative to the current directory>", "unit": "<function or class name, or null>", "line": <line number, or null>}"#;

/// The prompt a researcher is given to check CLAIM against the code.
pub fn claim_prompt(claim: &Claim) -> String {
    let mut lines: Vec<String> = vec![
        "Check one sentence of a software plan against this repository (the current directory)."
            .into(),
        String::new(),
        "Section: (none)".into(),
        format!("Sentence: {}", claim.statement),
    ];
    if let Some(c) = claim.about.first() {
        lines.push(format!(
            "Hint: the writer pointed at `{}:{}`: a place to start, not the answer.",
            c.file.display(),
            c.lines.start
        ));
    }
    lines.extend(
        [
            "",
            "Use Read, Grep and Glob to find the code the sentence is about, and read it.",
            "Behaviour can live in any layer: server code, templates, and front-end scripts and styles. Before you conclude that something does not exist, search for it across the whole repository.",
            "Rule: judge what the sentence says is true of the code now; if it also wishes or asks for something, ignore that part.",
            "If the code cannot settle it (it is about people, documents or outside services), the verdict is cannot_tell.",
            "",
            "Answer with one fenced ```json block holding one object, after any notes:",
            "```json",
        ]
        .map(String::from),
    );
    lines.push(format!(r#"{{"verdict": "right | wrong | cannot_tell", "why": "<one sentence>", "evidence": [{EVIDENCE_SHAPE}]}}"#));
    lines.extend(
        [
            "```",
            "For right or wrong, give at least one evidence entry: the code the verdict rests on.",
            r#"In evidence, "unit" is the exact name of one function, class or method as the file defines it (Class.method for a method), or null. For anything else (a template, a heading, part of a function, several names) give null and the line. A unit the file does not define voids the verdict."#,
        ]
        .map(String::from),
    );
    lines.iter().map(|l| format!("{l}\n")).collect()
}

/// A citation as the reply gave it, before it is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCitation {
    pub path: String,
    pub unit: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedReply {
    pub verdict: Verdict,
    pub why: String,
    pub citations: Vec<RawCitation>,
}

/// The last ```json block, else the first `{` from which the rest of the
/// reply decodes as an object. An unknown verdict is refused, and so is
/// right or wrong with no evidence.
pub fn parse_reply(text: &str) -> Result<ParsedReply, String> {
    let v = fenced(text)
        .or_else(|| bare(text))
        .ok_or("no JSON object in the reply")?;
    let o = v.as_object().ok_or("the reply's JSON is not an object")?;
    let w = o
        .get("verdict")
        .and_then(Value::as_str)
        .ok_or("no verdict")?
        .trim()
        .replace(' ', "_")
        .to_lowercase();
    let verdict = match w.as_str() {
        "right" => Verdict::Right,
        "wrong" => Verdict::Wrong,
        "cannot_tell" => Verdict::CannotTell,
        _ => return Err(format!("unknown verdict `{w}`")),
    };
    let why = o
        .get("why")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let citations: Vec<RawCitation> = o
        .get("evidence")
        .and_then(Value::as_array)
        .map(|xs| xs.iter().filter_map(citation).collect())
        .unwrap_or_default();
    if verdict != Verdict::CannotTell && citations.is_empty() {
        return Err("a verdict with no evidence".into());
    }
    Ok(ParsedReply {
        verdict,
        why,
        citations,
    })
}

fn fenced(text: &str) -> Option<Value> {
    let last = text.split("```json").skip(1).last()?;
    serde_json::from_str(last.split("```").next()?).ok()
}

fn bare(text: &str) -> Option<Value> {
    text.char_indices()
        .filter(|(_, c)| *c == '{')
        .find_map(|(i, _)| {
            let rest = &text[i..];
            let end = rest.rfind('}')?;
            serde_json::from_str::<Value>(&rest[..=end])
                .ok()
                .filter(Value::is_object)
        })
}

/// "path", with "path:42" or "path:Unit" read as a line or a unit.
fn citation(e: &Value) -> Option<RawCitation> {
    let p0 = e.get("path")?.as_str()?.trim();
    let (path, suffix) = match p0.rsplit_once(':') {
        Some((before, after)) if before.len() > 1 && !after.is_empty() && !after.contains('/') => {
            (before, Some(after))
        }
        _ => (p0, None),
    };
    if path.is_empty() {
        return None;
    }
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    let unit = match e.get("unit").and_then(Value::as_str).map(str::trim) {
        Some(u) if !u.is_empty() => Some(u.to_string()),
        _ => suffix.filter(|s| !digits(s)).map(String::from),
    };
    let line = match e.get("line").and_then(Value::as_f64) {
        Some(n) => Some(n.round() as u32),
        None => suffix.filter(|s| digits(s)).and_then(|s| s.parse().ok()),
    };
    Some(RawCitation {
        path: path.into(),
        unit,
        line,
    })
}

const CHECK_WORDS: &str = "a statement checked against the project's code";

pub fn verdict_words(v: Verdict) -> &'static str {
    match v {
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

/// The three guards, sharing one state (one Jev request).
pub fn guard_questions(
    claim: &Claim,
    verdict: Verdict,
    why: &str,
    evidence: &str,
) -> Vec<Question> {
    let ctx = vec![
        ("sentence".to_string(), claim.statement.clone()),
        ("check".to_string(), CHECK_WORDS.to_string()),
        ("verdict".to_string(), verdict_words(verdict).to_string()),
        ("why".to_string(), format!("Reason: {why}")),
        (
            "evidence".to_string(),
            if evidence.is_empty() {
                "(nothing cited)".to_string()
            } else {
                evidence.to_string()
            },
        ),
    ];
    let q = |kind: &str, instructions: &str, yes: &str, no: &str| Question {
        kind: kind.into(),
        instructions: instructions.into(),
        shape: Shape::Noul {
            yes: yes.into(),
            no: no.into(),
        },
        context: ctx.clone(),
        unsure_allowed: false,
    };
    vec![
        q("why-fits", WHY_INSTRUCTIONS, WHY_TRUE, WHY_FALSE),
        q("evidence-fits", EV_INSTRUCTIONS, EV_TRUE, EV_FALSE),
        q(
            "verdict-follows",
            FOLLOWS_INSTRUCTIONS,
            FOLLOWS_TRUE,
            FOLLOWS_FALSE,
        ),
    ]
}

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
