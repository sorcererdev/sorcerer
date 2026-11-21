//! The claim checker: checks a statement about code, such as "`cancel`
//! returns `None` when the order is missing", by having a researcher read
//! the project and give a verdict with the code it rests on.
//!
//! The check is a cascade, measured on a prose corpus:
//!
//! 1. With a decider (Jev), the cheap model (haiku) checks first. Jev then
//!    asks three yes/no questions about its verdict (does the reason fit
//!    the claim, does the cited code fit, does the verdict follow). When
//!    all three are yes with at least 0.60 probability, that verdict
//!    stands. Otherwise (a doubtful or unsure guard, a `cannot_tell`, or a
//!    first check that failed) the strong model (sonnet) checks again and
//!    its verdict stands.
//! 2. Without a decider, the strong model checks alone.
//! 3. Every place a verdict cites is looked for in the project (it must be
//!    inside the project and not fenced). One that is not found turns the
//!    verdict into `cannot_tell` ("cited code not found").
//!
//! The prompt names a unit the way the language of the claim's files does
//! (`CheckClaimDeps::languages`, each language's `unit_words`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fingerprint::fingerprint;
use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::{Span, Unit};
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::claim::{
    CITATION_MISSED, Checked, Citation, CitationCheck, Cited, Claim, ClaimKind, Escalation,
    FIRST_MODEL, GUARD_THRESHOLD, RawCitation, STRONG_MODEL, Verdict,
};
use crate::domain::claim_prompt::{ParsedReply, claim_prompt, guard_questions, parse_reply};
use crate::domain::research::{Research, Tool};
use crate::ports::decider::Decider;
use crate::ports::researcher::Researcher;
use crate::{Error, Result};

/// What a claim check runs with: who researches and who guards, what may
/// be read, and the models, threshold and limits of the cascade.
pub struct CheckClaimDeps<'a> {
    pub researcher: &'a dyn Researcher,
    /// Jev, or none: then the strong model checks alone.
    pub decider: Option<&'a dyn Decider>,
    pub fences: &'a Fences,
    pub view: &'a dyn CodebaseView,
    /// The project's languages, so the prompt names a unit the way the
    /// claim's files do; without them, in neutral words.
    pub languages: Option<&'a Languages>,
    /// The product the errands are recorded under.
    pub product: &'a str,
    /// The cheap model that checks first (`haiku`).
    pub first: &'a str,
    /// The model that checks when the first is not trusted (`sonnet`).
    pub strong: &'a str,
    /// How sure each guard must be for the first verdict to stand (0.60).
    pub threshold: f64,
    /// The most one model check may spend, in dollars.
    pub cap_usd: f64,
    pub timeout: Duration,
}

impl<'a> CheckClaimDeps<'a> {
    /// The measured cascade: haiku, then sonnet, a 0.60 threshold, at most
    /// $0.10 and 180 seconds a model check, no languages.
    pub fn defaults(
        researcher: &'a dyn Researcher,
        decider: Option<&'a dyn Decider>,
        fences: &'a Fences,
        view: &'a dyn CodebaseView,
        product: &'a str,
    ) -> CheckClaimDeps<'a> {
        CheckClaimDeps {
            researcher,
            decider,
            fences,
            view,
            languages: None,
            product,
            first: FIRST_MODEL,
            strong: STRONG_MODEL,
            threshold: GUARD_THRESHOLD,
            cap_usd: 0.10,
            timeout: Duration::from_secs(180),
        }
    }
}

/// Checks one claim with the cascade the module comment describes. It
/// fails when the claim is about the web, when the strong model's check
/// fails, when Jev fails, or when it is cancelled.
pub fn check_claim(claim: &Claim, deps: &CheckClaimDeps, cancel: &Cancel) -> Result<Checked> {
    refuse_web_claims(claim)?;
    let check = ClaimCheck {
        claim,
        deps,
        cancel,
    };
    let mut errands = Vec::new();
    let Some(decider) = deps.decider else {
        let strong = check.by_model(deps.strong, &mut errands)?;
        return Ok(strong.checked(Escalation::No, errands));
    };
    if let Ok(first) = check.by_model(deps.first, &mut errands)
        && check.guards_trust(decider, &first)?
    {
        return Ok(first.checked(Escalation::No, errands));
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let strong = check.by_model(deps.strong, &mut errands)?;
    Ok(strong.checked(Escalation::ToTheStrongModel, errands))
}

fn refuse_web_claims(claim: &Claim) -> Result<()> {
    match claim.kind {
        ClaimKind::Code => Ok(()),
        ClaimKind::Web => Err(Error::Reply {
            provider: "checker".into(),
            why: "web claims wait for a later milestone".into(),
        }),
    }
}

/// The claim, what the check runs with, and its cancel, which every step
/// of one check shares.
struct ClaimCheck<'a> {
    claim: &'a Claim,
    deps: &'a CheckClaimDeps<'a>,
    cancel: &'a Cancel,
}

/// One model's verdict, with every place it cited looked for.
struct ModelVerdict {
    verdict: Verdict,
    why: String,
    /// The places the verdict rests on; empty when any was not found.
    citations: Vec<Citation>,
    /// Every place the reply cited, as it gave them, for the guards.
    raw: Vec<RawCitation>,
    cited: Vec<Cited>,
}

impl ModelVerdict {
    fn checked(self, escalation: Escalation, errands: Vec<Errand>) -> Checked {
        Checked {
            verdict: self.verdict,
            why: self.why,
            citations: self.citations,
            cited: self.cited,
            escalation,
            errands,
        }
    }
}

impl ClaimCheck<'_> {
    /// One model checks the claim: the researcher reads the project and
    /// replies, its call is added to `errands` (even when the reply cannot
    /// be read), and each place the reply cites is looked for. A place
    /// not found turns the verdict into `cannot_tell`.
    fn by_model(&self, model: &str, errands: &mut Vec<Errand>) -> Result<ModelVerdict> {
        let researched = self
            .deps
            .researcher
            .research(self.research_for(model), self.cancel)?;
        errands.push(researched.errand);
        let reply = parse_reply(&researched.answer).map_err(|why| Error::Reply {
            provider: model.into(),
            why,
        })?;
        Ok(self.verdict_with_citations_looked_for(reply))
    }

    fn research_for(&self, model: &str) -> Research {
        Research {
            product: self.deps.product.into(),
            verb: "check".into(),
            prompt: claim_prompt(self.claim, self.deps.languages),
            model: model.into(),
            tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
            cap_usd: self.deps.cap_usd,
            timeout: self.deps.timeout,
        }
    }

    fn verdict_with_citations_looked_for(&self, reply: ParsedReply) -> ModelVerdict {
        let found: Vec<Option<(Citation, CitationCheck)>> = reply
            .citations
            .iter()
            .map(|raw| self.confirm(raw))
            .collect();
        let cited = reply
            .citations
            .iter()
            .zip(&found)
            .map(|(raw, found)| Cited {
                raw: raw.clone(),
                check: found
                    .as_ref()
                    .map_or(CitationCheck::NotConfirmed, |(_, check)| *check),
            })
            .collect();
        let all_found: Option<Vec<Citation>> = found
            .into_iter()
            .map(|found| found.map(|(citation, _)| citation))
            .collect();
        match all_found {
            Some(citations) => ModelVerdict {
                verdict: reply.verdict,
                why: reply.why,
                citations,
                raw: reply.citations,
                cited,
            },
            None => ModelVerdict {
                verdict: Verdict::CannotTell,
                why: CITATION_MISSED.into(),
                citations: Vec::new(),
                raw: reply.citations,
                cited,
            },
        }
    }

    /// Looks for one cited place in the project. The lines it stands on
    /// are the named unit's; when the unit is not in the file (or none is
    /// named), the cited line, if the file has it; with neither a unit nor
    /// a line, the whole file. None when the file cannot be read (outside
    /// the project, fenced, missing) or neither the unit nor the line is
    /// found.
    ///
    /// Example: `orders.py`, unit `Order.refund`, line 52, where the file
    /// has no `refund` but has 80 lines: found on line 52 only.
    fn confirm(&self, raw: &RawCitation) -> Option<(Citation, CitationCheck)> {
        let file = CitedFile::open(raw, self.deps.view, self.deps.fences)?;
        let unit_span = raw
            .unit
            .as_deref()
            .and_then(|name| file.unit_named(name, self.deps.view))
            .map(|unit| unit.span);
        let (lines, check) = match (unit_span, &raw.unit, raw.line) {
            (Some(span), _, _) => (span, CitationCheck::Confirmed),
            (None, Some(_), _) => (
                file.one_line(raw.line)?,
                CitationCheck::ConfirmedOnTheLineOnly,
            ),
            (None, None, Some(_)) => (file.one_line(raw.line)?, CitationCheck::Confirmed),
            (None, None, None) => (file.every_line(), CitationCheck::Confirmed),
        };
        let citation = Citation {
            print: Some(fingerprint(&file.text, lines).to_string()),
            file: file.path,
            lines,
        };
        Some((citation, check))
    }

    /// Whether Jev trusts the first model's verdict: it is not
    /// `cannot_tell`, and all three guards say yes with at least the
    /// threshold's probability. An unsure guard counts as no.
    fn guards_trust(&self, decider: &dyn Decider, first: &ModelVerdict) -> Result<bool> {
        if first.verdict == Verdict::CannotTell {
            return Ok(false);
        }
        let evidence = guard_evidence(&first.raw, self.deps.view, self.deps.fences);
        let answers = decider.ask(
            &guard_questions(self.claim, first.verdict, &first.why, &evidence),
            self.cancel,
        )?;
        Ok(answers.len() == 3
            && answers
                .iter()
                .all(|weighed| weighed.answer.p_yes().unwrap_or(0.0) >= self.deps.threshold))
    }
}

/// A cited file the checker may read: its path from the project root and
/// its text.
struct CitedFile {
    path: PathBuf,
    text: String,
}

impl CitedFile {
    /// The file a citation names, when it is inside the project, not
    /// fenced, and readable.
    fn open(raw: &RawCitation, view: &dyn CodebaseView, fences: &Fences) -> Option<CitedFile> {
        let written = Path::new(&raw.path);
        if !fences.allows(written) {
            return None;
        }
        let path = fences.relative(written)?;
        let text = view.text(&path).ok()?;
        Some(CitedFile { path, text })
    }

    fn line_count(&self) -> u32 {
        self.text.lines().count() as u32
    }

    fn unit_named(&self, name: &str, view: &dyn CodebaseView) -> Option<Unit> {
        let units = view.units(&self.path).ok()?;
        unit_named(&units, name).cloned()
    }

    /// The cited line as a span, when the file has that line.
    fn one_line(&self, line: Option<u32>) -> Option<Span> {
        line.filter(|line| (1..=self.line_count().max(1)).contains(line))
            .map(|line| Span {
                start: line,
                end: line,
            })
    }

    fn every_line(&self) -> Span {
        Span {
            start: 1,
            end: self.line_count().max(1),
        }
    }
}

/// The unit called `name`: the exact qualified name (the earliest of
/// several), or a bare name that is the last part of exactly one qualified
/// name. A model may join a method with `.` or `::` whatever its language
/// does, so the two separators are read alike.
///
/// Example: with units `Jev::new` and `Jev::ask_raw`, the names
/// `Jev.ask_raw`, `Jev::ask_raw` and `ask_raw` all find `Jev::ask_raw`.
fn unit_named<'u>(units: &'u [Unit], name: &str) -> Option<&'u Unit> {
    let plain = |name: &str| name.replace("::", ".");
    let name = plain(name);
    if let Some(exact) = units.iter().find(|unit| plain(&unit.name) == name) {
        return Some(exact);
    }
    let ending_in_name: Vec<&Unit> = units
        .iter()
        .filter(|unit| plain(&unit.name).ends_with(&format!(".{name}")))
        .collect();
    match ending_in_name.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

/// What the guards see as evidence: the first three citations, each
/// labelled `path:Unit line N` and followed by 12 lines of its code.
///
/// The 12 lines start 4 before the cited line when the file has it, else
/// at the named unit's start, else at the top of the file. Example: a
/// citation of `orders.py:cancel line 41` shows lines 37 to 48.
fn guard_evidence(raw: &[RawCitation], view: &dyn CodebaseView, fences: &Fences) -> String {
    raw.iter()
        .take(3)
        .filter_map(|cited| {
            let file = CitedFile::open(cited, view, fences)?;
            let start = evidence_start(cited, &file, view);
            Some(format!(
                "{}\n{}",
                evidence_label(cited, &file),
                twelve_lines_from(&file.text, start)
            ))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn evidence_start(cited: &RawCitation, file: &CitedFile, view: &dyn CodebaseView) -> u32 {
    let unit_start = cited
        .unit
        .as_deref()
        .and_then(|name| file.unit_named(name, view))
        .map(|unit| unit.span.start);
    match (cited.line, unit_start) {
        (Some(line), _) if line >= 1 && line <= file.line_count() => line.saturating_sub(4).max(1),
        (_, Some(start)) => start,
        _ => 1,
    }
}

fn evidence_label(cited: &RawCitation, file: &CitedFile) -> String {
    let unit = cited
        .unit
        .as_deref()
        .map(|unit| format!(":{unit}"))
        .unwrap_or_default();
    let line = cited
        .line
        .map(|line| format!(" line {line}"))
        .unwrap_or_default();
    format!("{}{unit}{line}", forward_slashes(&file.path))
}

fn twelve_lines_from(text: &str, start: u32) -> String {
    text.lines()
        .skip(start as usize - 1)
        .take(12)
        .map(|line| format!("{line}\n"))
        .collect()
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::UnitKind;

    fn unit(name: &str, start: u32) -> Unit {
        Unit {
            name: name.into(),
            kind: UnitKind::Method,
            span: Span {
                start,
                end: start + 5,
            },
            body: None,
            fingerprint: Fingerprint::parse("0000000").unwrap(),
        }
    }

    #[test]
    fn a_cited_method_matches_whatever_separator_the_language_uses() {
        // the prompt asks for Class.method; a language may name it Class::method
        let units = [unit("Jev::new", 1), unit("Jev::ask_raw", 10)];
        assert_eq!(
            unit_named(&units, "Jev.ask_raw").map(|u| u.span.start),
            Some(10)
        );
        assert_eq!(
            unit_named(&units, "Jev::ask_raw").map(|u| u.span.start),
            Some(10)
        );
        assert_eq!(
            unit_named(&units, "ask_raw").map(|u| u.span.start),
            Some(10)
        );
        let dotted = [unit("Order.cancel", 3)];
        assert_eq!(
            unit_named(&dotted, "Order::cancel").map(|u| u.span.start),
            Some(3)
        );
        assert!(unit_named(&units, "Other.ask_raw").is_none());
    }
}
