//! The review call as data: the pack of changed units it is asked about, with
//! the project's steering and the open findings to answer; its prompt and
//! the parse of its reply. The prompt's words are the eval's: changing one
//! changes what the corpus measured (`tests/golden/review-prompt.txt`).

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use sorcerer_core::domain::unit::Span;

use crate::domain::finding::Finding;
use crate::domain::issue_type::ISSUE_TYPES;

/// Seconds a review call may take.
pub const REVIEW_TIMEOUT: Duration = Duration::from_secs(420);

/// Numbered lines a pack may hold (a longer unit goes alone).
const PACK_LINES: u32 = 600;

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

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAnswer {
    pub id: String,
    pub holds: bool,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReviewReply {
    pub findings: Vec<RawFinding>,
    pub open: Vec<OpenAnswer>,
}

fn numbered(u: &ReviewUnit) -> String {
    let mut head = format!(
        "--- {}:{}-{} {}",
        u.file.display(),
        u.lines.start,
        u.lines.end,
        u.name
    );
    if let Some(h) = u.header.as_deref().filter(|h| !h.is_empty()) {
        head.push_str(&format!(" (in `{h}`)"));
    }
    if u.in_progress {
        head.push_str(" (in progress)");
    }
    let mut rows = vec![head];
    for (i, l) in u.text.lines().enumerate() {
        rows.push(format!("{:>5}  {}", u.lines.start as usize + i, l));
    }
    rows.join("\n")
}

fn open_line(f: &Finding) -> String {
    format!(
        "- id {}: [{}] {}:{}: {}",
        f.id.0,
        f.issue_type,
        f.place.file.display(),
        f.place.lines.start,
        f.statement
    )
}

pub fn review_prompt(a: &ReviewAsk) -> String {
    let open_ask = "Answer every open finding in \"open\": \"still holds\" when the code still has that problem, \"no longer holds\" when the code no longer has it, with one line why. Do not raise an open finding again in \"findings\".\n";
    let mut s = String::new();
    s.push_str("A developer has just saved a change in this repository (the current directory is the tree after the change).\n\n");
    s.push_str("The changed units (functions and methods this change adds or changes), with line numbers as in the file after the change:\n");
    if a.units.is_empty() {
        s.push_str("(none)");
    } else {
        s.push_str(
            &a.units
                .iter()
                .map(numbered)
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
    }
    s.push_str("\n\nThe change as a diff (tests included):\n");
    s.push_str(a.diff.trim_end());
    s.push_str(
        "\n\nThe project's steering, which holds the rules this project keeps:\n<steering>\n",
    );
    s.push_str(a.steering.trim_end());
    s.push_str("\n</steering>\n\nFindings already reported on these files, which you must not raise again:\n");
    if a.reported.is_empty() {
        s.push_str("none");
    } else {
        s.push_str(
            &a.reported
                .iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    s.push_str("\n\nOpen findings from earlier saves on these units:\n");
    if a.open.is_empty() {
        s.push_str("none");
    } else {
        s.push_str(&a.open.iter().map(open_line).collect::<Vec<_>>().join("\n"));
    }
    s.push('\n');
    if !a.open.is_empty() {
        s.push('\n');
        s.push_str(open_ask);
    }
    s.push('\n');
    if a.units.iter().any(|u| u.in_progress) {
        s.push_str("Units marked (in progress) are still being written: report only rule breaks and bugs in the code that is there, never that something is missing or not done yet.\n");
    }
    s.push_str("Review the changed units. List problems this change has or introduces: nothing else. An empty list is a normal and often the right answer; do not look for something to say. Do not speculate: a finding needs a named code path (the line, the call and what happens), found with Read, Grep and Glob. Do not report style or taste unless it breaks a rule in the steering file.\n\n");
    s.push_str("Issue types:\n");
    s.push_str("- bug: the code does the wrong thing.\n");
    s.push_str("- assumption: the code relies on something about other code that is not so.\n");
    s.push_str("- convention: the code breaks a rule in the steering file.\n");
    s.push_str("- design: readability, maintainability, testability or architecture with no rule behind it.\n\n");
    s.push_str("End your answer with one JSON object and nothing after it:\n");
    s.push_str("{\"findings\": [{\"issue_type\": \"bug|assumption|convention|design\",\n");
    s.push_str("   \"file\": \"path/relative/to/repo\", \"lines\": [first line, last line] (in the file after the change),\n");
    s.push_str("   \"statement\": \"two to three sentences: what is wrong and why it matters\",\n");
    s.push_str("   \"rule\": \"the id of the rule it breaks, exactly as listed under Rules in the steering, or empty\",\n");
    s.push_str("   \"evidence\": [{\"file\": \"path/relative/to/repo\", \"lines\": [first line, last line]}]}],\n");
    s.push_str(" \"open\": [{\"id\": \"the open finding's id\", \"holds\": \"still holds|no longer holds\", \"why\": \"one line\"}]}\n");
    s
}

/// The reply's JSON: fenced chunks last-first, then first `{` to last `}`,
/// then first `[` to last `]` (the array first when it opens first); the first that decodes wins.
pub fn json_in(t: &str) -> Option<Value> {
    let decode = |s: &str| serde_json::from_str::<Value>(s.trim()).ok();
    if t.contains("```") {
        for chunk in t.split("```").collect::<Vec<_>>().into_iter().rev() {
            let p = chunk.trim();
            let p = p.strip_prefix("json").unwrap_or(p);
            if let Some(v) = decode(p) {
                return Some(v);
            }
        }
    }
    // Whichever opens first goes first, so a bare array of objects is the
    // array, not its first object (trying braces first loses a bare array).
    let pairs = match (t.find('{'), t.find('[')) {
        (Some(b), Some(a)) if a < b => [('[', ']'), ('{', '}')],
        _ => [('{', '}'), ('[', ']')],
    };
    for (o, c) in pairs {
        if let (Some(i), Some(j)) = (t.find(o), t.rfind(c))
            && i <= j
            && let Some(v) = decode(&t[i..=j])
        {
            return Some(v);
        }
    }
    None
}

fn int(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|x| u32::try_from(x).ok()).or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && *f >= 0.0 && *f <= u32::MAX as f64)
                .map(|f| f as u32)
        }),
        _ => None,
    }
}

fn pair_of(v: &Value) -> Option<Span> {
    let xs = v.as_array()?;
    let start = int(xs.first()?)?;
    let end = int(xs.last()?)?;
    Some(Span { start, end })
}

fn text_of(o: &serde_json::Map<String, Value>, k: &str) -> Option<String> {
    match o.get(k) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

fn list<'a>(o: &'a serde_json::Map<String, Value>, k: &str) -> &'a [Value] {
    match o.get(k) {
        Some(Value::Array(xs)) => xs,
        _ => &[],
    }
}

fn finding_of(v: &Value) -> Option<RawFinding> {
    let o = v.as_object()?;
    let kind = text_of(o, "issue_type")?;
    if !ISSUE_TYPES.iter().any(|k| k.name == kind) {
        return None;
    }
    let file = text_of(o, "file")?;
    let lines = match o.get("lines") {
        Some(l) => pair_of(l)?,
        None => {
            let l = int(o.get("line")?)?;
            Span { start: l, end: l }
        }
    };
    let statement = text_of(o, "statement")?;
    let evidence = list(o, "evidence")
        .iter()
        .filter_map(|e| {
            let e = e.as_object()?;
            Some((
                PathBuf::from(text_of(e, "file")?),
                pair_of(e.get("lines")?)?,
            ))
        })
        .collect();
    Some(RawFinding {
        issue_type: kind,
        file: PathBuf::from(file),
        lines,
        statement,
        rule: text_of(o, "rule"),
        evidence,
    })
}

fn open_of(v: &Value) -> Option<OpenAnswer> {
    let o = v.as_object()?;
    let id = match o.get("id")? {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => n.as_f64()?.to_string(),
        },
        _ => return None,
    };
    let holds = match o.get("holds")?.as_str()? {
        "still holds" => true,
        "no longer holds" => false,
        _ => return None,
    };
    Some(OpenAnswer {
        id,
        holds,
        why: text_of(o, "why").unwrap_or_default(),
    })
}

/// Err only when no JSON is there at all.
pub fn parse_review(text: &str) -> Result<ReviewReply, String> {
    match json_in(text) {
        None => Err("no JSON in the review reply".into()),
        Some(Value::Array(xs)) => Ok(ReviewReply {
            findings: xs.iter().filter_map(finding_of).collect(),
            open: vec![],
        }),
        Some(Value::Object(o)) => Ok(ReviewReply {
            findings: list(&o, "findings").iter().filter_map(finding_of).collect(),
            open: list(&o, "open").iter().filter_map(open_of).collect(),
        }),
        Some(_) => Ok(ReviewReply::default()),
    }
}

/// Units in order, packed into calls of at most 600 numbered lines; a unit
/// longer than that goes alone.
pub fn pack_units(units: Vec<ReviewUnit>) -> Vec<Vec<ReviewUnit>> {
    let size = |u: &ReviewUnit| u.lines.end.saturating_sub(u.lines.start) + 1;
    let mut packs: Vec<Vec<ReviewUnit>> = vec![];
    let mut n = 0;
    for u in units {
        let s = size(&u);
        match packs.last_mut() {
            Some(p) if n + s <= PACK_LINES => {
                p.push(u);
                n += s;
            }
            _ => {
                packs.push(vec![u]);
                n = s;
            }
        }
    }
    packs
}
