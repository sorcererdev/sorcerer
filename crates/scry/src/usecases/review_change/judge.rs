//! Judging each finding the reviewer raised: where it is, whether it is a
//! problem scry already knows, and whether it is worth checking.
//!
//! `judge` takes one finding through the steps (its doc comment lists them
//! with an example); `merge` folds a finding into a stored one that Jev
//! says is the same problem. Jev is asked two questions here: "is this the
//! same problem as that one?" and "is this a nitpick?". Any question Jev
//! cannot answer stops the whole pass (`Halt::JevDidNotAnswer`): scry does not guess.
//!
//! The run log's text is built last in the file.

use std::path::{Path, PathBuf};

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::{Span, Unit};
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};

use super::verify::citations;
use super::{Halt, Pass};
use crate::domain::finding::{Evidence, Finding, FindingId, Place, Scores, Status, status_word};
use crate::domain::judging::{
    Match, Placement, SAME_THRESHOLD, asked_on, candidates, cited_code, confidence_of,
    finding_text, match_stored, nitpick, place, same_problem,
};
use crate::domain::log_text::{count, section, sub};
use crate::domain::review_prompt::RawFinding;
use crate::domain::scope::Scope;

/// What to do with a new finding, given what Jev said about the stored ones.
enum WhatToDo {
    /// Store it as new. `same_as` is a stored finding Jev called the same
    /// problem that cannot take it in (resolved, gone, or dismissed in
    /// another unit), for the log.
    AddAsNew { same_as: Option<Finding> },
    /// Fold it into this stored finding.
    MergeInto(Finding),
    /// Throw it away: the user dismissed this stored finding.
    Drop(Finding),
}

impl WhatToDo {
    /// The decision for a new finding in the unit `unit`, given the stored
    /// finding Jev called the same problem (if any).
    fn given(same: Option<Finding>, unit: Option<&str>) -> WhatToDo {
        let Some(stored) = same else {
            return WhatToDo::AddAsNew { same_as: None };
        };
        match match_stored(&stored, unit) {
            Match::AddAsNew => WhatToDo::AddAsNew {
                same_as: Some(stored),
            },
            Match::MergeInto => WhatToDo::MergeInto(stored),
            Match::Drop => WhatToDo::Drop(stored),
        }
    }
}

/// Jev's answer to "is this a nitpick?".
struct NitpickAnswer {
    /// How sure scry is that the finding is worth showing: 1 minus Jev's
    /// probability that it is a nitpick. P(nitpick) 0.2 is confidence 0.8.
    confidence: f64,
    /// A key for what the question was asked on (the unit's fingerprint
    /// and the rule text), so a withheld finding is asked again only when
    /// one of them changed.
    asked_on: String,
    /// The answer itself, for the log.
    answer: Weighed,
}

impl Pass<'_, '_> {
    /// Judges the reviewer's findings one by one, in the order it gave them.
    pub(super) fn judge_all(
        &mut self,
        raw_findings: Vec<RawFinding>,
        scope: &Scope,
    ) -> std::result::Result<(), Halt> {
        self.log(&section("Findings"));
        if raw_findings.is_empty() {
            self.log(NONE_FOUND_TEXT);
        }
        for (n, raw) in raw_findings.into_iter().enumerate() {
            self.log(&finding_heading_text(n, &raw));
            self.judge(raw, scope)?;
        }
        Ok(())
    }

    /// Judges one finding the reviewer raised:
    ///
    /// 1. looks up the project rule it cites by id (an id the steering does
    ///    not have is dropped, the finding kept);
    /// 2. finds the unit its lines are in; one outside every unit this run
    ///    reviewed is stored as "elsewhere", with no question to Jev;
    /// 3. asks Jev whether it is the same problem as a stored finding (one
    ///    in the same file, citing the same rule or the same lines);
    /// 4. drops it when the user dismissed that problem, merges it into a
    ///    stored finding that is still live, or else adds it as new: Jev
    ///    is asked "nitpick?", and it is stored unverified when it is
    ///    probably real, withheld when it is probably a nitpick.
    ///
    /// Example: the reviewer says "cancel() returns None" at `orders.py:2`.
    /// It is in `cancel`, which this run reviewed. A stored finding says
    /// "cancel() gives back None"; Jev says same problem (P 0.9), and the
    /// stored one is open, so the new wording is merged into it.
    fn judge(&mut self, raw: RawFinding, scope: &Scope) -> std::result::Result<(), Halt> {
        let (raw, cited_rule) = self.with_cited_rule_looked_up(raw);
        self.log(&found_text(&raw, cited_rule.as_deref()));
        let placed = match place(&raw, &scope.units, &self.units_now(&raw.file)) {
            Placement::Elsewhere(place) => {
                self.add_elsewhere(&raw, place);
                return Ok(());
            }
            Placement::InReviewedUnit(place) => place,
        };
        self.log(&placed_text(&placed));
        let code = cited_code(&self.text(&raw.file), raw.lines.start);
        let same = self.same_problem_as_stored(&raw, &code)?;
        match WhatToDo::given(same, placed.unit.as_deref()) {
            WhatToDo::Drop(stored) => {
                self.log(&dropped_text(&stored));
                Ok(())
            }
            WhatToDo::MergeInto(stored) => {
                self.log(&merge_into_text(&stored));
                self.merge(&stored.id, raw, placed)
            }
            WhatToDo::AddAsNew { same_as } => {
                self.log(&added_as_new_text(same_as.as_ref()));
                self.add_unverified_or_withheld(&raw, placed, &code)
            }
        }
    }

    /// The finding with its cited rule id replaced by the rule as the
    /// steering has it (none when the steering has no rule with that id),
    /// and the id as the reviewer cited it, for the log.
    fn with_cited_rule_looked_up(&self, raw: RawFinding) -> (RawFinding, Option<String>) {
        let cited = raw.rule.clone();
        let rule = cited
            .as_deref()
            .and_then(|r| self.steering.lookup(r))
            .map(String::from);
        (RawFinding { rule, ..raw }, cited)
    }

    /// The file's units as they are now; none when the file is fenced off.
    fn units_now(&self, file: &Path) -> Vec<Unit> {
        if self.fences.allows(file) {
            self.deps.view.units(file).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Stores a finding that is in no reviewed unit as "elsewhere". Jev is
    /// not asked: scry does not show it, it only remembers it.
    fn add_elsewhere(&mut self, raw: &RawFinding, placed: Place) {
        let unit = placed.unit.clone();
        let id = self.add(raw, placed, Status::Elsewhere, Scores::default());
        self.log(&elsewhere_text(unit.as_deref(), &id));
    }

    /// The stored finding Jev says is the same problem as the new one, if
    /// any. Jev is asked about every stored finding that shares the new
    /// one's file, rule or cited lines, all in one batch; the first it puts
    /// at or past `SAME_THRESHOLD` wins. Every answer is logged.
    fn same_problem_as_stored(
        &mut self,
        raw: &RawFinding,
        code: &str,
    ) -> std::result::Result<Option<Finding>, Halt> {
        let stored: Vec<Finding> = candidates(&self.working_copy, raw)
            .into_iter()
            .cloned()
            .collect();
        if stored.is_empty() {
            self.log(NO_CANDIDATES_TEXT);
            return Ok(None);
        }
        let questions: Vec<Question> = stored
            .iter()
            .map(|s| {
                let code_b = cited_code(&self.text(&s.place.file), s.place.lines.start);
                same_problem(
                    &raw_finding_text(raw),
                    &stored_finding_text(s),
                    code,
                    &code_b,
                )
            })
            .collect();
        let answers = self.ask(&questions)?;
        self.log(&same_problem_text(&stored, &answers));
        Ok(stored
            .into_iter()
            .zip(answers)
            .find(|(_, w)| p_yes(w) >= SAME_THRESHOLD)
            .map(|(s, _)| s))
    }

    /// Asks Jev whether the new finding is a nitpick and stores it:
    /// unverified (to be checked) when its confidence reaches
    /// `[scry] confidence_threshold`, else withheld (kept, never shown).
    ///
    /// Example: with the threshold at 0.7, Jev's P(nitpick) 0.2 gives
    /// confidence 0.8: unverified. P(nitpick) 0.9 gives 0.1: withheld.
    fn add_unverified_or_withheld(
        &mut self,
        raw: &RawFinding,
        placed: Place,
        code: &str,
    ) -> std::result::Result<(), Halt> {
        let nitpick = self.ask_nitpick(raw, placed.fingerprint.as_ref(), code)?;
        let threshold = self.cfg.confidence_threshold;
        let status = if nitpick.confidence >= threshold {
            Status::Unverified
        } else {
            Status::Withheld
        };
        let scores = Scores {
            confidence: Some(nitpick.confidence),
            asked_on: Some(nitpick.asked_on),
        };
        let id = self.add(raw, placed, status, scores);
        self.log(&nitpick_text(
            &nitpick.answer,
            nitpick.confidence,
            threshold,
            status,
            &id,
        ));
        Ok(())
    }

    /// Asks Jev a batch of questions. Any question Jev cannot answer (an
    /// error, or "unsure") stops the pass: scry does not run on a guess.
    fn ask(&self, qs: &[Question]) -> std::result::Result<Vec<Weighed>, Halt> {
        let ws = self
            .deps
            .decider
            .ask(qs, self.deps.cancel)
            .map_err(|_| Halt::JevDidNotAnswer)?;
        if ws.len() != qs.len() || ws.iter().any(|w| w.answer == Answer::Unsure) {
            return Err(Halt::JevDidNotAnswer);
        }
        Ok(ws)
    }

    /// Asks Jev "is this a nitpick?" about the finding, seen in the unit
    /// with fingerprint `unit_fingerprint` and the code it cites.
    fn ask_nitpick(
        &self,
        raw: &RawFinding,
        unit_fingerprint: Option<&Fingerprint>,
        code: &str,
    ) -> std::result::Result<NitpickAnswer, Halt> {
        let q = nitpick(self.steering, raw, code);
        let asked_on = asked_on(unit_fingerprint, rule_context(&q));
        let answer = self
            .ask(std::slice::from_ref(&q))?
            .into_iter()
            .next()
            .expect("one answer for one question");
        Ok(NitpickAnswer {
            confidence: confidence_of(&answer),
            asked_on,
            answer,
        })
    }

    /// Adds a new finding to the working copy with this status and scores,
    /// seen once, now, and remembers it as this run's. Returns its id, which
    /// is made from the time, place and statement (and made unique).
    fn add(&mut self, raw: &RawFinding, pl: Place, status: Status, scores: Scores) -> FindingId {
        let id = self
            .working_copy
            .new_id(self.now, &pl.file, pl.lines, &raw.statement);
        self.new.push(id.clone());
        self.working_copy.findings.push(Finding {
            id,
            issue_type: raw.issue_type.clone(),
            rule: raw.rule.clone(),
            statement: raw.statement.clone(),
            place: pl,
            evidence: bare(&raw.evidence),
            scores,
            verdict: None,
            status,
            seen: 1,
            first: self.now,
            last: self.now,
            history: vec![(self.now, status)],
            model: self.cfg.review_model.clone(),
            cost_usd: 0.0,
        });
        self.new.last().cloned().expect("just pushed")
    }

    /// Folds a new finding into the stored finding `id`, which Jev called
    /// the same problem:
    ///
    /// 1. the stored finding takes the new wording, place and evidence
    ///    (unless this run found it too: then its first wording stays);
    /// 2. it is counted as seen once more, now;
    /// 3. a withheld one is asked "nitpick?" again if what it was asked on
    ///    changed; a refuted or unsure one is unverified again if the code
    ///    it cites changed; any other keeps its status.
    ///
    /// Its id and any dismissal stay.
    ///
    /// Example: "zeta overflows" was refuted. The reviewer raises "zeta
    /// overflows on big input" and the cited line now has a different
    /// fingerprint: the stored finding takes the new wording and is
    /// unverified again, so the rounds check it again.
    fn merge(
        &mut self,
        id: &FindingId,
        raw: RawFinding,
        placed: Place,
    ) -> std::result::Result<(), Halt> {
        let cited = citations(self.deps.view, self.fences, &raw.evidence);
        let Some(stored) = self.stored(id) else {
            return Ok(());
        };
        let old_prints = prints(&stored.evidence);
        let f = self
            .with_this_runs_version(stored, raw, placed, &cited)
            .seen_again(self.now);
        let f = match f.status {
            Status::Withheld => self.asked_again_if_changed(f)?,
            Status::Refuted | Status::Unsure => {
                self.unverified_again_if_evidence_moved(f, &cited, &old_prints)
            }
            _ => f,
        };
        self.log(&merged_text(&f));
        self.merged.insert(f.id.clone());
        self.replace_stored(f);
        Ok(())
    }

    /// The stored finding `id`, as the working copy has it now.
    fn stored(&self, id: &FindingId) -> Option<Finding> {
        self.working_copy
            .findings
            .iter()
            .find(|f| f.id == *id)
            .cloned()
    }

    /// The stored finding with the new one's place, wording and evidence. A
    /// finding this run itself added keeps its own: two findings of one run
    /// that are the same problem keep the first one's words.
    fn with_this_runs_version(
        &self,
        stored: Finding,
        raw: RawFinding,
        placed: Place,
        cited: &Result<Vec<Evidence>, String>,
    ) -> Finding {
        if self.new.contains(&stored.id) {
            return stored;
        }
        Finding {
            place: placed,
            evidence: cited.clone().unwrap_or_else(|_| bare(&raw.evidence)),
            statement: raw.statement,
            ..stored
        }
    }

    /// The withheld finding, asked "nitpick?" again when what it was asked
    /// on (its unit's code or the rule text) changed: unverified when its
    /// new confidence reaches the threshold, else still withheld. Not
    /// asked again when nothing changed.
    fn asked_again_if_changed(&mut self, f: Finding) -> std::result::Result<Finding, Halt> {
        let code = cited_code(&self.text(&f.place.file), f.place.lines.start);
        let q = nitpick(self.steering, &raw_of(&f), &code);
        let asked_on = asked_on(f.place.fingerprint.as_ref(), rule_context(&q));
        if f.scores.asked_on.as_deref() == Some(asked_on.as_str()) {
            self.log(NOT_ASKED_AGAIN_TEXT);
            return Ok(f);
        }
        let answer = self.ask(std::slice::from_ref(&q))?;
        let confidence = confidence_of(&answer[0]);
        let threshold = self.cfg.confidence_threshold;
        let mut f = Finding {
            scores: Scores {
                confidence: Some(confidence),
                asked_on: Some(asked_on),
            },
            ..f
        };
        if confidence >= threshold {
            f.set_status(Status::Unverified, self.now);
        }
        self.log(&asked_again_text(
            &answer[0], confidence, threshold, f.status,
        ));
        Ok(f)
    }

    /// The refuted or unsure finding, unverified again (so the rounds
    /// check it again) when the code it cites changed: the fingerprints of
    /// its cited lines now are not the ones it was checked against.
    fn unverified_again_if_evidence_moved(
        &self,
        mut f: Finding,
        cited: &Result<Vec<Evidence>, String>,
        old_prints: &[Option<String>],
    ) -> Finding {
        if let Ok(evidence) = cited
            && prints(evidence) != old_prints
        {
            f.set_status(Status::Unverified, self.now);
            self.log(EVIDENCE_MOVED_TEXT);
        }
        f
    }

    /// Puts the finding back in the working copy, in place of the one with
    /// its id.
    fn replace_stored(&mut self, f: Finding) {
        if let Some(slot) = self.working_copy.findings.iter_mut().find(|g| g.id == f.id) {
            *slot = f;
        }
    }
}

fn p_yes(w: &Weighed) -> f64 {
    match w.answer {
        Answer::Yes(Confidence::Known(c)) => c,
        Answer::No(Confidence::Known(c)) => 1.0 - c,
        _ => 0.0,
    }
}

/// The rule text a nitpick question carried: the rule, or the Rules section.
fn rule_context(q: &Question) -> &str {
    q.context
        .iter()
        .find(|(n, _)| n == "project rule" || n == "project rules")
        .map_or("", |(_, v)| v.as_str())
}

/// RAW as Jev is shown it.
fn raw_finding_text(raw: &RawFinding) -> String {
    finding_text(
        &raw.issue_type,
        &raw.file,
        raw.lines.start,
        raw.rule.as_deref(),
        &raw.statement,
    )
}

/// The stored finding F as Jev is shown it.
fn stored_finding_text(f: &Finding) -> String {
    finding_text(
        &f.issue_type,
        &f.place.file,
        f.place.lines.start,
        f.rule.as_deref(),
        &f.statement,
    )
}

fn raw_of(f: &Finding) -> RawFinding {
    RawFinding {
        issue_type: f.issue_type.clone(),
        file: f.place.file.clone(),
        lines: f.place.lines,
        statement: f.statement.clone(),
        rule: f.rule.clone(),
        evidence: f
            .evidence
            .iter()
            .map(|e| (e.file.clone(), e.lines))
            .collect(),
    }
}

fn bare(evidence: &[(PathBuf, Span)]) -> Vec<Evidence> {
    evidence
        .iter()
        .map(|(file, lines)| Evidence {
            file: file.clone(),
            lines: *lines,
            print: None,
        })
        .collect()
}

fn prints(evidence: &[Evidence]) -> Vec<Option<String>> {
    evidence.iter().map(|e| e.print.clone()).collect()
}

// The run log's text for judging.

const NONE_FOUND_TEXT: &str = "- none found\n";

const NO_CANDIDATES_TEXT: &str =
    "- same-problem question: no stored finding shares its file, rule or cited code\n";

const NOT_ASKED_AGAIN_TEXT: &str =
    "- withheld, asked on the same unit, rule and sides: not asked again\n";

const EVIDENCE_MOVED_TEXT: &str = "- its evidence moved: it is unverified again\n";

/// `### Finding 3: bug at orders.py:2-2`.
fn finding_heading_text(n: usize, raw: &RawFinding) -> String {
    sub(&format!(
        "Finding {}: {} at {}:{}-{}",
        n + 1,
        raw.issue_type,
        forward_slashes(&raw.file),
        raw.lines.start,
        raw.lines.end
    ))
}

/// What the call found: its statement, the rule it cited (CITED_RULE, as
/// cited) and its evidence.
fn found_text(raw: &RawFinding, cited_rule: Option<&str>) -> String {
    let rule = match (cited_rule, &raw.rule) {
        (None, _) => "none cited".to_string(),
        (Some(c), Some(r)) => format!("cited \"{c}\" → {r}"),
        (Some(c), None) => {
            format!("cited \"{c}\": no rule has that id; dropped, the finding stays")
        }
    };
    let evidence: Vec<String> = raw
        .evidence
        .iter()
        .map(|(f, l)| format!("{}:{}-{}", forward_slashes(f), l.start, l.end))
        .collect();
    format!(
        "- statement: {}\n- rule: {rule}\n- evidence: {}\n",
        raw.statement,
        if evidence.is_empty() {
            "none".into()
        } else {
            evidence.join(", ")
        }
    )
}

/// "unit `cancel`", or "no unit".
fn unit_text(unit: Option<&str>) -> String {
    unit.map_or("no unit".into(), |u| format!("unit `{u}`"))
}

fn elsewhere_text(unit: Option<&str>, id: &FindingId) -> String {
    format!(
        "- placed: elsewhere (its lines are in no unit in scope; {} now): stored with no question\n- id {}, elsewhere\n",
        unit_text(unit),
        id.0
    )
}

fn placed_text(placed: &Place) -> String {
    format!(
        "- placed: {} (fingerprint {})\n",
        unit_text(placed.unit.as_deref()),
        placed.fingerprint.map_or("none".into(), |f| f.to_string())
    )
}

/// Jev's answer for each stored finding: the first at or past the
/// threshold wins.
fn same_problem_text(stored: &[Finding], answers: &[Weighed]) -> String {
    let mut text = String::new();
    let mut won = false;
    for (s, w) in stored.iter().zip(answers) {
        let same = p_yes(w) >= SAME_THRESHOLD;
        let said = if same && !won {
            won = true;
            "the same: the first at or past the threshold wins"
        } else if same {
            "the same, but an earlier one won"
        } else {
            "not the same"
        };
        text.push_str(&format!(
            "- same problem as {} ({}, {})? {} → {said}\n",
            s.id.0,
            status_word(s.status),
            stored_finding_text(s),
            p_text(w),
        ));
    }
    text
}

fn dropped_text(stored: &Finding) -> String {
    format!(
        "- result: dropped ({} was {} here)\n",
        stored.id.0,
        status_word(stored.status)
    )
}

fn merge_into_text(stored: &Finding) -> String {
    format!("- result: merge into {}\n", stored.id.0)
}

/// `- result: new`, or why a stored finding Jev called the same problem
/// could not take it in.
fn added_as_new_text(same_as: Option<&Finding>) -> String {
    match same_as {
        None => "- result: new\n".into(),
        Some(stored) => format!(
            "- result: new ({} is {}: a match starts afresh)\n",
            stored.id.0,
            status_word(stored.status)
        ),
    }
}

fn nitpick_text(
    w: &Weighed,
    confidence: f64,
    threshold: f64,
    status: Status,
    id: &FindingId,
) -> String {
    format!(
        "- nitpick? {} → confidence {confidence:.2}, threshold {threshold:.2} → {}\n- id {}, {}\n",
        p_text(w),
        status_word(status),
        id.0,
        status_word(status)
    )
}

fn asked_again_text(w: &Weighed, confidence: f64, threshold: f64, status: Status) -> String {
    format!(
        "- withheld, and what it was asked on changed: nitpick? {} → confidence {confidence:.2}, threshold {threshold:.2} → {}\n",
        p_text(w),
        status_word(status)
    )
}

fn merged_text(f: &Finding) -> String {
    format!(
        "- id {} (merged; seen {}), {}\n",
        f.id.0,
        count(f.seen as usize, "time"),
        status_word(f.status)
    )
}

/// `P(yes) 0.20`, or the answer when it carries no probability.
fn p_text(w: &Weighed) -> String {
    match w.answer.p_yes() {
        Some(p) => format!("P(yes) {p:.2}"),
        None => format!("{:?}", w.answer),
    }
}
