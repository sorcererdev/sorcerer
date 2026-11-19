//! Each finding the review calls gave: placed, matched against the store
//! (Jev's same-problem question), merged, dropped or added, and asked
//! Jev's nitpick question.

use std::path::PathBuf;

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::unit::Span;
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};

use super::verify::citations;
use super::{Halt, Run};
use crate::domain::finding::{Evidence, Finding, FindingId, Place, Scores, Status, status_word};
use crate::domain::judging::{
    Match, SAME_THRESHOLD, asked_on, candidates, cited_code, confidence_of, finding_text,
    match_stored, nitpick, place, same_problem,
};
use crate::domain::log_text::{count, section, sub};
use crate::domain::review_prompt::RawFinding;
use crate::domain::scope::Scope;

impl Run<'_, '_> {
    /// Judges RAWS in order.
    pub(super) fn judge_all(
        &mut self,
        raws: Vec<RawFinding>,
        scope: &Scope,
    ) -> std::result::Result<(), Halt> {
        self.t(&section("Findings"));
        if raws.is_empty() {
            self.t("- none found\n");
        }
        for (n, raw) in raws.into_iter().enumerate() {
            self.t(&sub(&format!(
                "Finding {}: {} at {}:{}-{}",
                n + 1,
                raw.issue_type,
                raw.file.display(),
                raw.lines.start,
                raw.lines.end
            )));
            self.judge(raw, scope)?;
        }

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
        let q = nitpick(self.st, raw, code);
        let on = asked_on(unit_fp, rule_context(&q));
        let w = self.ask(std::slice::from_ref(&q))?;
        let w = w.into_iter().next().expect("one answer for one question");
        Ok((confidence_of(&w), on, w))
    }

    fn judge(&mut self, mut raw: RawFinding, scope: &Scope) -> std::result::Result<(), Halt> {
        let cited_rule = raw.rule.clone();
        raw.rule = raw
            .rule
            .as_deref()
            .and_then(|r| self.st.lookup(r))
            .map(String::from);
        let evidence: Vec<String> = raw
            .evidence
            .iter()
            .map(|(f, l)| format!("{}:{}-{}", f.display(), l.start, l.end))
            .collect();
        let rule = match (&cited_rule, &raw.rule) {
            (None, _) => "none cited".to_string(),
            (Some(c), Some(r)) => format!("cited \"{c}\" → {r}"),
            (Some(c), None) => {
                format!("cited \"{c}\": no rule has that id; dropped, the finding stays")
            }
        };
        self.t(&format!(
            "- statement: {}\n- rule: {rule}\n- evidence: {}\n",
            raw.statement,
            if evidence.is_empty() {
                "none".into()
            } else {
                evidence.join(", ")
            }
        ));
        let units_now = if self.fences.allows(&raw.file) {
            self.deps.view.units(&raw.file).unwrap_or_default()
        } else {
            Vec::new()
        };
        let (pl, status) = place(&raw, &scope.units, &units_now);
        let unit = pl
            .unit
            .as_deref()
            .map_or("no unit".into(), |u| format!("unit `{u}`"));
        if status == Status::Elsewhere {
            let id = self.add(&raw, pl, Status::Elsewhere, Scores::default());
            self.t(&format!(
                "- placed: elsewhere (its lines are in no unit in scope; {unit} now): stored with no question\n- id {}, elsewhere\n",
                id.0
            ));
            return Ok(());
        }
        self.t(&format!(
            "- placed: {unit} (fingerprint {})\n",
            pl.fingerprint.map_or("none".into(), |f| f.to_string())
        ));
        let code_a = cited_code(&self.text(&raw.file), raw.lines.start);
        let cands: Vec<Finding> = candidates(&self.mine, &raw).into_iter().cloned().collect();
        let mut hit = None;
        if !cands.is_empty() {
            let a = finding_text(
                &raw.issue_type,
                &raw.file,
                raw.lines.start,
                raw.rule.as_deref(),
                &raw.statement,
            );
            let mut qs = Vec::new();
            for c in &cands {
                let b = finding_text(
                    &c.issue_type,
                    &c.place.file,
                    c.place.lines.start,
                    c.rule.as_deref(),
                    &c.statement,
                );
                let code_b = cited_code(&self.text(&c.place.file), c.place.lines.start);
                qs.push(same_problem(&a, &b, &code_a, &code_b));
            }
            let ws = self.ask(&qs)?;
            let mut lines = String::new();
            let mut first = true;
            for (c, w) in cands.iter().zip(&ws) {
                let p = p_yes(w);
                let said = if p >= SAME_THRESHOLD && first {
                    first = false;
                    "the same: the first at or past the threshold wins"
                } else if p >= SAME_THRESHOLD {
                    "the same, but an earlier one won"
                } else {
                    "not the same"
                };
                lines.push_str(&format!(
                    "- same problem as {} ({}, {})? {} → {said}\n",
                    c.id.0,
                    status_word(c.status),
                    finding_text(
                        &c.issue_type,
                        &c.place.file,
                        c.place.lines.start,
                        c.rule.as_deref(),
                        &c.statement
                    ),
                    p_text(w),
                ));
            }
            self.t(&lines);
            hit = cands
                .into_iter()
                .zip(ws)
                .find(|(_, w)| p_yes(w) >= SAME_THRESHOLD);
        } else {
            self.t(
                "- same-problem question: no stored finding shares its file, rule or cited code\n",
            );
        }
        match hit.map(|(c, _)| (match_stored(&c, pl.unit.as_deref()), c)) {
            Some((Match::Drop, c)) => {
                self.t(&format!(
                    "- result: dropped ({} was {} here)\n",
                    c.id.0,
                    status_word(c.status)
                ));
                Ok(())
            }
            Some((Match::Merge, c)) => {
                self.t(&format!("- result: merge into {}\n", c.id.0));
                self.merge(&c.id, raw, pl)
            }
            other => {
                if let Some((_, c)) = other {
                    self.t(&format!(
                        "- result: new ({} is {}: a match starts afresh)\n",
                        c.id.0,
                        status_word(c.status)
                    ));
                } else {
                    self.t("- result: new\n");
                }
                let (confidence, on, w) =
                    self.confidence(&raw, pl.fingerprint.as_ref(), &code_a)?;
                let status = if confidence >= self.cfg.confidence_threshold {
                    Status::Waiting
                } else {
                    Status::Withheld
                };
                let scores = Scores {
                    confidence: Some(confidence),
                    asked_on: Some(on),
                };
                let id = self.add(&raw, pl, status, scores);
                self.t(&format!(
                    "- nitpick? {} → confidence {confidence:.2}, threshold {:.2} → {}\n- id {}, {}\n",
                    p_text(&w),
                    self.cfg.confidence_threshold,
                    status_word(status),
                    id.0,
                    status_word(status)
                ));
                Ok(())
            }
        }
    }

    fn add(&mut self, raw: &RawFinding, pl: Place, status: Status, scores: Scores) -> FindingId {
        let id = self
            .mine
            .new_id(self.now, &pl.file, pl.lines, &raw.statement);
        self.new.push(id.clone());
        self.mine.findings.push(Finding {
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

    /// RAW merges into the stored finding ID: its place, wording and evidence
    /// replace the stored ones; id, status and dismissals stay. A withheld one
    /// is asked again when what it was asked on changed; a refuted or unsure
    /// one waits again when its evidence prints moved.
    fn merge(
        &mut self,
        id: &FindingId,
        raw: RawFinding,
        pl: Place,
    ) -> std::result::Result<(), Halt> {
        let cited = citations(self.deps.view, self.fences, &raw.evidence);
        let old = self.mine.findings.iter().find(|f| f.id == *id).cloned();
        let Some(mut f) = old else { return Ok(()) };
        let old_prints: Vec<Option<String>> = f.evidence.iter().map(|e| e.print.clone()).collect();
        // a finding stored by an earlier pass takes this pass's place, wording
        // and evidence; one this pass found keeps the first of its duplicates'
        if !self.new.contains(&f.id) {
            f.place = pl;
            f.statement = raw.statement;
            f.evidence = cited.clone().unwrap_or_else(|_| bare(&raw.evidence));
        }
        f.seen += 1;
        f.last = self.now;
        match f.status {
            Status::Withheld => {
                let r = raw_of(&f);
                let code = cited_code(&self.text(&f.place.file), f.place.lines.start);
                let q = nitpick(self.st, &r, &code);
                let on = asked_on(f.place.fingerprint.as_ref(), rule_context(&q));
                if f.scores.asked_on.as_deref() != Some(on.as_str()) {
                    let w = self.ask(std::slice::from_ref(&q))?;
                    let confidence = confidence_of(&w[0]);
                    f.scores = Scores {
                        confidence: Some(confidence),
                        asked_on: Some(on),
                    };
                    if confidence >= self.cfg.confidence_threshold {
                        f.set_status(Status::Waiting, self.now);
                    }
                    self.t(&format!(
                        "- withheld, and what it was asked on changed: nitpick? {} → confidence {confidence:.2}, threshold {:.2} → {}\n",
                        p_text(&w[0]),
                        self.cfg.confidence_threshold,
                        status_word(f.status)
                    ));
                } else {
                    self.t("- withheld, asked on the same unit, rule and sides: not asked again\n");
                }
            }
            Status::Refuted | Status::Unsure => {
                if let Ok(new) = &cited {
                    let new_prints: Vec<Option<String>> =
                        new.iter().map(|e| e.print.clone()).collect();
                    if new_prints != old_prints {
                        f.set_status(Status::Waiting, self.now);
                        self.t("- its evidence moved: it waits to be checked again\n");
                    }
                }
            }
            _ => {}
        }
        self.t(&format!(
            "- id {} (merged; seen {}), {}\n",
            f.id.0,
            count(f.seen as usize, "time"),
            status_word(f.status)
        ));
        self.merged.insert(f.id.clone());
        if let Some(slot) = self.mine.findings.iter_mut().find(|g| g.id == f.id) {
            *slot = f;
        }
        Ok(())
    }
}

// `P(yes) 0.20`, or the answer when it carries no probability.
fn p_text(w: &Weighed) -> String {
    match w.answer.p_yes() {
        Some(p) => format!("P(yes) {p:.2}"),
        None => format!("{:?}", w.answer),
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
