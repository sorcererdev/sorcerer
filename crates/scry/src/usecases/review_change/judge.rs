//! Each finding the review calls gave. `judge`:
//! 1. resolves the rule it cites (`resolve_cited_rule`) and logs what was
//!    found;
//! 2. places it (`place`, over `units_now`); one in no unit in scope is
//!    stored as elsewhere with no question (`add_elsewhere`);
//! 3. asks Jev whether a stored finding is the same problem
//!    (`same_problem_as_stored`);
//! 4. drops it, merges it into that finding (`merge`), or adds it,
//!    unverified or, when Jev calls it a nitpick, withheld
//!    (`add_unverified_or_withheld`).
//!
//! The run log's text is built last in the file.

use std::path::{Path, PathBuf};

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::unit::{Span, Unit};
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};

use super::verify::citations;
use super::{Halt, Pass};
use crate::domain::finding::{Evidence, Finding, FindingId, Place, Scores, Status, status_word};
use crate::domain::judging::{
    Match, SAME_THRESHOLD, asked_on, candidates, cited_code, confidence_of, finding_text,
    match_stored, nitpick, place, same_problem,
};
use crate::domain::log_text::{count, section, sub};
use crate::domain::review_prompt::RawFinding;
use crate::domain::scope::Scope;

impl Pass<'_, '_> {
    /// Judges RAW_FINDINGS in order.
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

    /// One finding: its cited rule resolved and what was found logged;
    /// placed; one in no unit in scope stored as elsewhere with no
    /// question; else matched against the stored findings, then dropped,
    /// merged, or added (withheld when a nitpick).
    fn judge(&mut self, mut raw: RawFinding, scope: &Scope) -> std::result::Result<(), Halt> {
        // 1. The reviewer may cite a project rule by its id. Look it up; an id
        //    the steering does not have is dropped, but the finding is kept.
        let cited_rule = self.resolve_cited_rule(&mut raw);
        // 2. Log what the reviewer found.
        self.log(&found_text(&raw, cited_rule.as_deref()));
        // 3. Find the unit (function, method) its lines are in. A finding
        //    outside every unit this run reviewed is stored as "elsewhere",
        //    without asking Jev anything.
        let units_now = self.units_now(&raw.file);
        let (placed, status) = place(&raw, &scope.units, &units_now);
        if status == Status::Elsewhere {
            self.add_elsewhere(&raw, placed);
            return Ok(());
        }
        self.log(&placed_text(&placed));
        // 4. Ask Jev whether it is the same problem as a finding already
        //    stored (one in the same file, citing the same rule or the same
        //    lines). The first one Jev is sure enough about wins.
        let code = cited_code(&self.text(&raw.file), raw.lines.start);
        let same = self.same_problem_as_stored(&raw, &code)?;
        // 5. Decide by what the stored one is:
        //    - dismissed by the user: drop the new one, it was rejected;
        //    - still live (open, unverified, withheld, refuted, unsure,
        //      elsewhere): merge the new one into it;
        //    - dismissed in another unit, or resolved: a new finding.
        // 6. A new finding: ask Jev whether it is a nitpick; store it as
        //    unverified when it is probably real, else withheld.
        match same.map(|stored| (match_stored(&stored, placed.unit.as_deref()), stored)) {
            Some((Match::Drop, stored)) => {
                self.log(&dropped_text(&stored));
                Ok(())
            }
            Some((Match::Merge, stored)) => {
                self.log(&merge_into_text(&stored));
                self.merge(&stored.id, raw, placed)
            }
            Some((Match::New, stored)) => {
                self.log(&new_afresh_text(&stored));
                self.add_unverified_or_withheld(&raw, placed, &code)
            }
            None => {
                self.log(NEW_TEXT);
                self.add_unverified_or_withheld(&raw, placed, &code)
            }
        }
    }

    /// RAW's rule as the steering has it (none when no rule has the cited
    /// id); returns the rule as RAW cited it.
    fn resolve_cited_rule(&self, raw: &mut RawFinding) -> Option<String> {
        let cited = raw.rule.take();
        raw.rule = cited
            .as_deref()
            .and_then(|r| self.steering.lookup(r))
            .map(String::from);
        cited
    }

    /// FILE's units as they are now; none when FILE is fenced.
    fn units_now(&self, file: &Path) -> Vec<Unit> {
        if self.fences.allows(file) {
            self.deps.view.units(file).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Stores RAW, placed in no unit in scope, as elsewhere: Jev is not asked.
    fn add_elsewhere(&mut self, raw: &RawFinding, placed: Place) {
        let unit = placed.unit.clone();
        let id = self.add(raw, placed, Status::Elsewhere, Scores::default());
        self.log(&elsewhere_text(unit.as_deref(), &id));
    }

    /// The stored finding Jev says is the same problem as RAW: the first
    /// candidate at or past `SAME_THRESHOLD`, if any. Every answer is logged.
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
        let mut questions = Vec::new();
        for s in &stored {
            let code_b = cited_code(&self.text(&s.place.file), s.place.lines.start);
            questions.push(same_problem(
                &raw_finding_text(raw),
                &stored_finding_text(s),
                code,
                &code_b,
            ));
        }
        let answers = self.ask(&questions)?;
        self.log(&same_problem_text(&stored, &answers));
        Ok(stored
            .into_iter()
            .zip(answers)
            .find(|(_, w)| p_yes(w) >= SAME_THRESHOLD)
            .map(|(s, _)| s))
    }

    /// Asks Jev whether RAW is a nitpick and stores it: unverified when its
    /// confidence reaches the threshold, else withheld.
    fn add_unverified_or_withheld(
        &mut self,
        raw: &RawFinding,
        placed: Place,
        code: &str,
    ) -> std::result::Result<(), Halt> {
        let (confidence, on, w) = self.confidence(raw, placed.fingerprint.as_ref(), code)?;
        let threshold = self.cfg.confidence_threshold;
        let status = if confidence >= threshold {
            Status::Unverified
        } else {
            Status::Withheld
        };
        let scores = Scores {
            confidence: Some(confidence),
            asked_on: Some(on),
        };
        let id = self.add(raw, placed, status, scores);
        self.log(&nitpick_text(&w, confidence, threshold, status, &id));
        Ok(())
    }

    /// One batch to Jev; any answer it could not give stops the pass.
    fn ask(&self, qs: &[Question]) -> std::result::Result<Vec<Weighed>, Halt> {
        let ws = self
            .deps
            .decider
            .ask(qs, self.cancel)
            .map_err(|_| Halt::Silent)?;
        if ws.len() != qs.len() || ws.iter().any(|w| w.answer == Answer::Unsure) {
            return Err(Halt::Silent);
        }
        Ok(ws)
    }

    /// Jev's nitpick question on RAW: its confidence, and what it was asked on.
    fn confidence(
        &self,
        raw: &RawFinding,
        unit_fp: Option<&Fingerprint>,
        code: &str,
    ) -> std::result::Result<(f64, String, Weighed), Halt> {
        let q = nitpick(self.steering, raw, code);
        let on = asked_on(unit_fp, rule_context(&q));
        let w = self.ask(std::slice::from_ref(&q))?;
        let w = w.into_iter().next().expect("one answer for one question");
        Ok((confidence_of(&w), on, w))
    }

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

    /// RAW merges into the stored finding ID: it takes this pass's place,
    /// wording and evidence; it is counted seen again; a withheld one is
    /// asked again when what it was asked on changed; a refuted or unsure
    /// one is unverified again when its evidence moved. Id, status and dismissals
    /// stay.
    fn merge(
        &mut self,
        id: &FindingId,
        raw: RawFinding,
        placed: Place,
    ) -> std::result::Result<(), Halt> {
        // 1. Check the new finding's cited lines (to compare them below).
        let cited = citations(self.deps.view, self.fences, &raw.evidence);
        let Some(mut f) = self.stored(id) else {
            return Ok(());
        };
        let old_prints = prints(&f.evidence);
        // 2. The stored finding takes the new wording, place and evidence
        //    (unless this same run found it: then the first wording stays).
        self.take_this_passs_version(&mut f, raw, placed, &cited);
        // 3. It has been seen once more, now.
        f.seen += 1;
        f.last = self.now;
        // 4. A withheld one is asked "nitpick?" again if its code, rule or
        //    sides changed. A refuted or unsure one is unverified again if
        //    the code it cites changed. Others keep their status.
        match f.status {
            Status::Withheld => self.ask_withheld_again_if_changed(&mut f)?,
            Status::Refuted | Status::Unsure => {
                self.wait_again_if_evidence_moved(&mut f, &cited, &old_prints);
            }
            _ => {}
        }
        // 5. Log it, remember it was merged into, and store it.
        self.log(&merged_text(&f));
        self.merged.insert(f.id.clone());
        self.replace_stored(f);
        Ok(())
    }

    /// The finding ID as the working copy has it.
    fn stored(&self, id: &FindingId) -> Option<Finding> {
        self.working_copy
            .findings
            .iter()
            .find(|f| f.id == *id)
            .cloned()
    }

    /// A finding stored by an earlier pass takes this pass's place, wording
    /// and evidence; one this pass found keeps the first of its duplicates'.
    fn take_this_passs_version(
        &self,
        f: &mut Finding,
        raw: RawFinding,
        placed: Place,
        cited: &Result<Vec<Evidence>, String>,
    ) {
        if self.new.contains(&f.id) {
            return;
        }
        f.place = placed;
        f.statement = raw.statement;
        f.evidence = cited.clone().unwrap_or_else(|_| bare(&raw.evidence));
    }

    /// A withheld finding is asked Jev's nitpick question again when what it
    /// was asked on (its unit, rule and sides) changed, and is unverified again when its
    /// confidence now reaches the threshold.
    fn ask_withheld_again_if_changed(&mut self, f: &mut Finding) -> std::result::Result<(), Halt> {
        let code = cited_code(&self.text(&f.place.file), f.place.lines.start);
        let q = nitpick(self.steering, &raw_of(f), &code);
        let on = asked_on(f.place.fingerprint.as_ref(), rule_context(&q));
        if f.scores.asked_on.as_deref() == Some(on.as_str()) {
            self.log(NOT_ASKED_AGAIN_TEXT);
            return Ok(());
        }
        let w = self.ask(std::slice::from_ref(&q))?;
        let confidence = confidence_of(&w[0]);
        f.scores = Scores {
            confidence: Some(confidence),
            asked_on: Some(on),
        };
        let threshold = self.cfg.confidence_threshold;
        if confidence >= threshold {
            f.set_status(Status::Unverified, self.now);
        }
        self.log(&asked_again_text(&w[0], confidence, threshold, f.status));
        Ok(())
    }

    /// A refuted or unsure finding is unverified again (to be checked again) when the prints
    /// of its evidence, as cited now, are not OLD_PRINTS.
    fn wait_again_if_evidence_moved(
        &self,
        f: &mut Finding,
        cited: &Result<Vec<Evidence>, String>,
        old_prints: &[Option<String>],
    ) {
        let Ok(evidence) = cited else { return };
        if prints(evidence) != old_prints {
            f.set_status(Status::Unverified, self.now);
            self.log(EVIDENCE_MOVED_TEXT);
        }
    }

    /// F in the working copy, in place of the finding with its id.
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

const NEW_TEXT: &str = "- result: new\n";

const NOT_ASKED_AGAIN_TEXT: &str =
    "- withheld, asked on the same unit, rule and sides: not asked again\n";

const EVIDENCE_MOVED_TEXT: &str = "- its evidence moved: it is unverified again\n";

/// `### Finding 3: bug at orders.py:2-2`.
fn finding_heading_text(n: usize, raw: &RawFinding) -> String {
    sub(&format!(
        "Finding {}: {} at {}:{}-{}",
        n + 1,
        raw.issue_type,
        raw.file.display(),
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
        .map(|(f, l)| format!("{}:{}-{}", f.display(), l.start, l.end))
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

fn new_afresh_text(stored: &Finding) -> String {
    format!(
        "- result: new ({} is {}: a match starts afresh)\n",
        stored.id.0,
        status_word(stored.status)
    )
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
