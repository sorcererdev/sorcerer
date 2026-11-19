//! Judging a finding after the review call:
//! place it (in a unit in scope, or elsewhere), pick the stored findings
//! worth asking Jev the same-problem question about, say what a match does
//! (the merge table), and ask Jev whether it is a nitpick.

use std::path::Path;

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::unit::{Span, Unit, unit_at};
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Shape, Weighed};

use crate::domain::finding::{Finding, Place, Status};
use crate::domain::findings::Findings;
use crate::domain::review_prompt::RawFinding;
use crate::domain::scope::ScopedUnit;
use crate::domain::steering::Steering;

/// P(same problem) at or past this merges.
pub const SAME_THRESHOLD: f64 = 0.5;

fn width(u: &Unit) -> u32 {
    u.span.end - u.span.start
}

/// Where RAW is, and its first status. Among the scoped units of its file
/// that overlap its lines, the one holding its first line comes first, then
/// the narrowest; that is `Open`. With none overlapping it is `Elsewhere`,
/// placed in the narrowest of UNITS_NOW (the file's units now) holding its
/// first line, if any.
pub fn place(raw: &RawFinding, scoped: &[ScopedUnit], units_now: &[Unit]) -> (Place, Status) {
    let first = raw.lines.start;
    let overlapping = scoped
        .iter()
        .filter(|s| s.file == raw.file)
        .map(|s| &s.unit)
        .filter(|u| u.span.start <= raw.lines.end && first <= u.span.end);
    let in_scope = overlapping.min_by_key(|u| (!u.span.holds(first), width(u)));
    let status = if in_scope.is_some() {
        Status::Open
    } else {
        Status::Elsewhere
    };
    let unit = in_scope.or_else(|| unit_at(units_now, first));
    let place = Place {
        file: raw.file.clone(),
        unit: unit.map(|u| u.name.clone()),
        lines: raw.lines,
        unit_start: unit.map(|u| u.span.start),
        fingerprint: unit.map(|u| u.fingerprint),
    };
    (place, status)
}

fn overlaps(a: Span, b: Span) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// The stored findings worth asking Jev about, in store order: not
/// resolved or gone, and sharing RAW's file, its rule, or a cited range.
/// RAW's rule is a rule's id (the pass looks it up first).
pub fn candidates<'a>(store: &'a Findings, raw: &RawFinding) -> Vec<&'a Finding> {
    store
        .findings
        .iter()
        .filter(|g| !matches!(g.status, Status::Resolved | Status::Gone))
        .filter(|g| {
            g.place.file == raw.file
                || (raw.rule.is_some() && g.rule == raw.rule)
                || raw.evidence.iter().any(|(p, a)| {
                    g.evidence
                        .iter()
                        .any(|e| e.file == *p && overlaps(*a, e.lines))
                })
        })
        .collect()
}

/// A finding as Jev is shown it: `issue_type at file:line; rule: X; statement`.
pub fn finding_text(
    issue_type: &str,
    file: &Path,
    line: u32,
    rule: Option<&str>,
    statement: &str,
) -> String {
    format!(
        "{issue_type} at {}:{line}; rule: {}; {statement}",
        file.display(),
        rule.unwrap_or("none")
    )
}

/// Jev's same-problem question, in the wording the eval measured.
pub fn same_problem(a: &str, b: &str, code_a: &str, code_b: &str) -> Question {
    Question {
        kind: "same".into(),
        instructions: "Do these two code review findings describe the same problem?".into(),
        shape: Shape::Noul {
            yes: "one fix would fix both: the same defect or rule break at the same code, however it is worded".into(),
            no: "they need separate fixes: different defects, or the same kind of defect at different code".into(),
        },
        context: vec![
            ("finding A".into(), a.into()),
            ("finding B".into(), b.into()),
            ("code at A".into(), code_a.into()),
            ("code at B".into(), code_b.into()),
        ],
        unsure_allowed: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    New,
    Merge,
    Drop,
}

/// What a new finding does when Jev says it is the same problem as STORED;
/// NEW_UNIT is the new finding's unit. Dismissed here drops only in the
/// unit it was dismissed in; resolved and gone start afresh.
pub fn match_stored(stored: &Finding, new_unit: Option<&str>) -> Match {
    match stored.status {
        Status::Open
        | Status::Waiting
        | Status::Withheld
        | Status::Refuted
        | Status::Unsure
        | Status::Elsewhere => Match::Merge,
        Status::DismissedHere => {
            if new_unit.is_some() && stored.place.unit.as_deref() == new_unit {
                Match::Drop
            } else {
                Match::New
            }
        }
        Status::DismissedEverywhere => Match::Drop,
        Status::Resolved | Status::Gone => Match::New,
    }
}

/// The nitpick question's two sides.
pub const NITPICK_IS: &str = "it is about taste or polish: naming, wording, formatting, style, a small optional refactor, or speculative future-proofing; nothing would go wrong if it were ignored";
pub const WORTH_SAYING_IS: &str = "ignoring it leaves wrong behaviour, a failure, lost data, a security hole, or a real maintenance cost";

/// Jev's nitpick question, with the text of the rule RAW cites (its id,
/// looked up already), or all the rules when it cites none. CITED is
/// [`cited_code`] at the finding's first line.
pub fn nitpick(st: &Steering, raw: &RawFinding, cited: &str) -> Question {
    let mut context = vec![
        ("issue type".to_string(), raw.issue_type.clone()),
        ("finding".to_string(), raw.statement.clone()),
        (
            "place".to_string(),
            format!("{}:{}", raw.file.display(), raw.lines.start),
        ),
        ("cited code".to_string(), cited.to_string()),
    ];
    context.push(match raw.rule.as_deref().and_then(|id| st.rule(id)) {
        Some(r) => ("project rule".into(), r.text.trim().to_string()),
        None => ("project rules".into(), st.rules_text()),
    });
    Question {
        kind: "nitpick".into(),
        instructions: "Is this code review finding a nitpick?".into(),
        shape: Shape::Noul {
            yes: NITPICK_IS.into(),
            no: WORTH_SAYING_IS.into(),
        },
        context,
        unsure_allowed: false,
    }
}

/// 1 − P(nitpick); an unsure answer keeps the finding (truth is checked after).
pub fn confidence_of(w: &Weighed) -> f64 {
    match w.answer {
        Answer::Yes(Confidence::Known(c)) => 1.0 - c,
        Answer::No(Confidence::Known(c)) => c,
        Answer::Yes(Confidence::Unknown) => 0.0,
        _ => 1.0,
    }
}

/// What the nitpick question was asked on: the unit's fingerprint, the rule
/// text it carried (the rule, or all the rules), and the two sides. Seven
/// hex digits of their FNV-1a; a withheld finding is asked again only when
/// this changes.
pub fn asked_on(unit_fp: Option<&Fingerprint>, rule_text: &str) -> String {
    let joined = [
        unit_fp.map_or("", |f| f.as_str()),
        rule_text,
        NITPICK_IS,
        WORTH_SAYING_IS,
    ]
    .join("\0");
    fnv64_hex(joined.as_bytes())[..7].to_string()
}

/// Eight lines either side of LINE, each `{:>4} {text}`.
pub fn cited_code(text: &str, line: u32) -> String {
    let ls: Vec<&str> = text.lines().collect();
    let lo = line.saturating_sub(8).max(1);
    let hi = (line as usize + 8).min(ls.len());
    (lo as usize..=hi)
        .filter_map(|n| ls.get(n - 1).map(|l| format!("{n:>4} {l}")))
        .collect::<Vec<_>>()
        .join("\n")
}
