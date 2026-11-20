//! What scry decides about each finding the reviewer returns, before it is
//! stored: where in the code it sits, which stored findings it might repeat,
//! what happens when it does repeat one, and whether Jev thinks it is only a
//! nitpick.

use std::path::Path;

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::unit::{Span, Unit, unit_at};
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Shape, UnsureAnswer, Weighed};

use crate::domain::finding::{Finding, Place, Status};
use crate::domain::findings::Findings;
use crate::domain::review_prompt::RawFinding;
use crate::domain::scope::ScopedUnit;
use crate::domain::steering::Steering;

/// How sure Jev must be that two findings are the same problem before they
/// are merged: at least 0.5, so an even chance is enough.
pub const SAME_THRESHOLD: f64 = 0.5;

/// Where a new finding sits: in a unit this run reviewed, or elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    /// Its lines are in a unit this run sent to the reviewer.
    InReviewedUnit(Place),
    /// Its lines are in no reviewed unit: it is stored as elsewhere,
    /// placed in whichever unit of the file holds its first line now (if
    /// any).
    Elsewhere(Place),
}

/// Where a new finding sits. Among the reviewed units of its file whose
/// lines overlap the finding's, it takes the one holding its first line,
/// else the narrowest. With none, it is elsewhere.
///
/// Example: a finding on lines 12-14 of `orders.py`, where the reviewed
/// units are the class `Order` (lines 1-30) and its method `Order.cancel`
/// (lines 10-20), is in `Order.cancel`: both hold line 12, and the method
/// is narrower. A finding on line 25, when only `Order.cancel` was
/// reviewed, is elsewhere, placed in `Order`.
pub fn place(raw: &RawFinding, reviewed: &[ScopedUnit], units_now: &[Unit]) -> Placement {
    match best_reviewed_unit(raw, reviewed) {
        Some(unit) => Placement::InReviewedUnit(place_in(raw, Some(unit))),
        None => {
            let unit_now = unit_at(units_now, raw.lines.start);
            Placement::Elsewhere(place_in(raw, unit_now))
        }
    }
}

/// The reviewed unit that best holds the finding: of those in its file
/// that overlap its lines, one holding its first line wins, and among
/// equals the one with the fewest lines.
fn best_reviewed_unit<'a>(raw: &RawFinding, reviewed: &'a [ScopedUnit]) -> Option<&'a Unit> {
    let first_line = raw.lines.start;
    reviewed
        .iter()
        .filter(|scoped| scoped.file == raw.file)
        .map(|scoped| &scoped.unit)
        .filter(|unit| overlaps(unit.span, raw.lines))
        .min_by_key(|unit| (!unit.span.holds(first_line), lines_spanned(unit)))
}

fn lines_spanned(unit: &Unit) -> u32 {
    unit.span.end - unit.span.start
}

/// The finding's place: its file and lines, and the unit it is in, if any.
fn place_in(raw: &RawFinding, unit: Option<&Unit>) -> Place {
    Place {
        file: raw.file.clone(),
        unit: unit.map(|u| u.name.clone()),
        lines: raw.lines,
        unit_start: unit.map(|u| u.span.start),
        fingerprint: unit.map(|u| u.fingerprint),
    }
}

fn overlaps(a: Span, b: Span) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// The stored findings that a new finding might repeat, so worth asking Jev
/// about, in the order the store holds them. A stored finding qualifies when
/// it is still live (not resolved, not gone) and it shares something with the
/// new one: the same file, the same project rule, or evidence on overlapping
/// lines of the same file. The new finding's rule is already a rule's id.
///
/// Example: a new finding in `orders.py` citing rule `log-text-apart` is
/// checked against every live finding in `orders.py`, and against live
/// findings in other files that cite `log-text-apart` too. A resolved
/// finding in `orders.py` is not checked.
pub fn candidates<'a>(store: &'a Findings, raw: &RawFinding) -> Vec<&'a Finding> {
    store
        .findings
        .iter()
        .filter(|stored| is_live(stored) && shares_file_rule_or_evidence(stored, raw))
        .collect()
}

fn is_live(stored: &Finding) -> bool {
    !matches!(stored.status, Status::Resolved | Status::Gone)
}

fn shares_file_rule_or_evidence(stored: &Finding, raw: &RawFinding) -> bool {
    let same_file = stored.place.file == raw.file;
    let same_rule = raw.rule.is_some() && stored.rule == raw.rule;
    let overlapping_evidence = raw.evidence.iter().any(|(file, lines)| {
        stored
            .evidence
            .iter()
            .any(|e| e.file == *file && overlaps(*lines, e.lines))
    });
    same_file || same_rule || overlapping_evidence
}

/// One finding as a single line for Jev to read.
///
/// Example: `bug at orders.py:41; rule: none; cancel() returns None`.
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

/// The question Jev is asked to tell whether two findings are the same
/// problem. It shows both findings (as [`finding_text`] writes them) and the
/// code each one cites. The wording is the one the eval measured; changing
/// it means measuring again.
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
        unsure: UnsureAnswer::NotAllowed,
    }
}

/// What happens to a new finding that Jev says is the same problem as a
/// stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// Store it as a new finding all the same.
    AddAsNew,
    /// Fold it into the stored finding.
    MergeInto,
    /// Throw it away: the user already dismissed this problem.
    Drop,
}

/// What happens to a new finding in the unit `new_unit` that Jev says is
/// the same problem as `stored`. A stored finding that is still live
/// (open, unverified, withheld, refuted, unsure, elsewhere) takes it in. A
/// dismissal drops it: everywhere, or only in the unit it was dismissed in.
/// A resolved or gone one starts afresh.
///
/// Example: the user dismissed "cancel() returns None" in `cancel` only.
/// The reviewer raises it again in `cancel`: dropped. It raises it in
/// `refund`: added as new.
pub fn match_stored(stored: &Finding, new_unit: Option<&str>) -> Match {
    match stored.status {
        Status::Open
        | Status::Unverified
        | Status::Withheld
        | Status::Refuted
        | Status::Unsure
        | Status::Elsewhere => Match::MergeInto,
        Status::DismissedHere if dismissed_in(stored, new_unit) => Match::Drop,
        Status::DismissedHere => Match::AddAsNew,
        Status::DismissedEverywhere => Match::Drop,
        Status::Resolved | Status::Gone => Match::AddAsNew,
    }
}

/// Whether the new finding is in the very unit the stored one was dismissed
/// in. A finding in no unit never is.
fn dismissed_in(stored: &Finding, new_unit: Option<&str>) -> bool {
    new_unit.is_some() && stored.place.unit.as_deref() == new_unit
}

/// What counts as a nitpick, as Jev is told it.
pub const NITPICK_IS: &str = "it is about taste or polish: naming, wording, formatting, style, a small optional refactor, or speculative future-proofing; nothing would go wrong if it were ignored";
/// What counts as worth saying, as Jev is told it.
pub const WORTH_SAYING_IS: &str = "ignoring it leaves wrong behaviour, a failure, lost data, a security hole, or a real maintenance cost";

/// The question Jev is asked to tell whether a finding is only a nitpick.
/// It shows the finding, where it is, the code around it (`cited`, as
/// [`cited_code`] writes it) and the project's rules: the one rule the
/// finding cites, or all of them when it cites none (or a rule the project
/// does not have).
pub fn nitpick(steering: &Steering, raw: &RawFinding, cited: &str) -> Question {
    let place = format!("{}:{}", raw.file.display(), raw.lines.start);
    Question {
        kind: "nitpick".into(),
        instructions: "Is this code review finding a nitpick?".into(),
        shape: Shape::Noul {
            yes: NITPICK_IS.into(),
            no: WORTH_SAYING_IS.into(),
        },
        context: vec![
            ("issue type".into(), raw.issue_type.clone()),
            ("finding".into(), raw.statement.clone()),
            ("place".into(), place),
            ("cited code".into(), cited.to_string()),
            rules_for(steering, raw),
        ],
        unsure: UnsureAnswer::NotAllowed,
    }
}

/// The rules part of the nitpick question: the cited rule's text, or all
/// the rules.
fn rules_for(steering: &Steering, raw: &RawFinding) -> (String, String) {
    match raw.rule.as_deref().and_then(|id| steering.rule(id)) {
        Some(rule) => ("project rule".into(), rule.text.trim().to_string()),
        None => ("project rules".into(), steering.rules_text()),
    }
}

/// How sure Jev is that a finding is worth saying, from 0 to 1: one minus
/// its chance that the finding is a nitpick. When Jev could not say how
/// sure it is, a "nitpick" answer scores 0 and anything else scores 1: a
/// finding is kept unless Jev called it a nitpick, and whether that was
/// right is checked afterwards.
///
/// Example: "nitpick, 0.8 sure" scores 0.2; "not a nitpick, 0.9 sure"
/// scores 0.9.
pub fn confidence_of(weighed: &Weighed) -> f64 {
    match weighed.answer {
        Answer::Yes(Confidence::Known(p_nitpick)) => 1.0 - p_nitpick,
        Answer::No(Confidence::Known(p_worth_saying)) => p_worth_saying,
        Answer::Yes(Confidence::Unknown) => 0.0,
        Answer::No(Confidence::Unknown)
        | Answer::Unsure
        | Answer::Chosen(..)
        | Answer::Level(..) => 1.0,
    }
}

/// A short stamp of what the nitpick question was asked about: the unit's
/// fingerprint, the rule text it carried, and the two definitions above.
/// A withheld finding is asked again only when its stamp changes, that is
/// when its unit's code, the project's rules or the definitions change.
///
/// Example: `"3fa9c01"` (the first seven hex digits of an FNV-1a hash of
/// the four parts).
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

/// How many lines of code Jev sees either side of a finding's first line.
const CITED_LINES_EITHER_SIDE: u32 = 8;

/// The code around a line, numbered: up to 8 lines either side of it, each
/// as its line number right-aligned in four columns, a space, and the text.
/// Near the top or bottom of the file there are fewer lines.
///
/// Example: around line 3 of a 30-line file, lines 1 to 11:
/// ```text
///    1 import os
///    2
///    3 def cancel(order):
/// ```
/// and so on to line 11.
pub fn cited_code(text: &str, line: u32) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let first = line.saturating_sub(CITED_LINES_EITHER_SIDE).max(1) as usize;
    let last = line as usize + CITED_LINES_EITHER_SIDE as usize;
    (first..=last.min(lines.len()))
        .filter_map(|n| lines.get(n - 1).map(|code| format!("{n:>4} {code}")))
        .collect::<Vec<_>>()
        .join("\n")
}
