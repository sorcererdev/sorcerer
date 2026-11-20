//! The claim checker: a statement about code ("`cancel` returns `None`
//! when the order is missing") checked by a researcher, the verdict
//! guarded by Jev, a cascade measured on a prose corpus at a 0.60 threshold:
//!
//! 1. With a decider, the first (cheap) model checks; three Jev guards ask
//!    whether its reason fits the claim, its evidence fits, and its verdict
//!    follows. A guard under the threshold, an unsure guard, a
//!    `cannot_tell`, or a failed first check sends it to the strong model.
//! 2. Without a decider, the strong model checks alone.
//! 3. Every citation is confirmed: inside the root, not fenced, and the
//!    unit named found in the file, or, when the name does not resolve or
//!    none is given, the line inside it (a citation that stood on its line
//!    although its unit was not found is marked `unit_missed`). One that is
//!    not confirmed makes the verdict `cannot_tell` ("cited code not found").
//!
//! The prompt names a unit the way the language of the claim's files does
//! (`CheckClaimDeps::languages`, each language's `unit_words`).

use std::time::Duration;

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fingerprint::fingerprint;
use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::unit::{Span, Unit};
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::claim::{
    CITATION_MISSED, Checked, Citation, CitationCheck, Cited, Claim, ClaimKind, Escalation,
    FIRST_MODEL, GUARD_THRESHOLD, RawCitation, STRONG_MODEL, Verdict,
};
use crate::domain::claim_prompt::{claim_prompt, guard_questions, parse_reply};
use crate::domain::research::{Research, Tool};
use crate::ports::decider::Decider;
use crate::ports::researcher::Researcher;
use crate::{Error, Result};

pub struct CheckClaimDeps<'a> {
    pub researcher: &'a dyn Researcher,
    pub decider: Option<&'a dyn Decider>,
    pub fences: &'a Fences,
    pub view: &'a dyn CodebaseView,
    /// The project's languages, so the prompt names a unit the way the
    /// claim's files do; without them, in neutral words.
    pub languages: Option<&'a Languages>,
    /// The product the errands are recorded under.
    pub product: &'a str,
    pub first: &'a str,
    pub strong: &'a str,
    pub threshold: f64,
    pub cap_usd: f64,
    pub timeout: Duration,
}

impl<'a> CheckClaimDeps<'a> {
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

/// One checked reply: its verdict, reason and confirmed citations, with
/// the raw citations for the guards' evidence.
struct Attempt {
    verdict: Verdict,
    why: String,
    citations: Vec<Citation>,
    raw: Vec<RawCitation>,
    cited: Vec<Cited>,
}

pub fn check_claim(claim: &Claim, deps: &CheckClaimDeps, cancel: &Cancel) -> Result<Checked> {
    if claim.kind == ClaimKind::Web {
        return Err(Error::Reply {
            provider: "checker".into(),
            why: "web claims wait for a later milestone".into(),
        });
    }
    let mut errands = Vec::new();
    let done = |a: Attempt, escalation: Escalation, errands: Vec<Errand>| Checked {
        verdict: a.verdict,
        why: a.why,
        citations: a.citations,
        cited: a.cited,
        escalation,
        errands,
    };
    let Some(decider) = deps.decider else {
        let a = attempt(claim, deps.strong, deps, cancel, &mut errands)?;
        return Ok(done(a, Escalation::No, errands));
    };
    if let Ok(a) = attempt(claim, deps.first, deps, cancel, &mut errands)
        && a.verdict != Verdict::CannotTell
    {
        let evidence = guard_evidence(&a.raw, deps.view, deps.fences);
        let ws = decider.ask(
            &guard_questions(claim, a.verdict, &a.why, &evidence),
            cancel,
        )?;
        if ws.len() == 3
            && ws
                .iter()
                .all(|w| w.answer.p_yes().unwrap_or(0.0) >= deps.threshold)
        {
            return Ok(done(a, Escalation::No, errands));
        }
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let a = attempt(claim, deps.strong, deps, cancel, &mut errands)?;
    Ok(done(a, Escalation::ToTheStrongModel, errands))
}

fn attempt(
    claim: &Claim,
    model: &str,
    deps: &CheckClaimDeps,
    cancel: &Cancel,
    errands: &mut Vec<Errand>,
) -> Result<Attempt> {
    let r = Research {
        product: deps.product.into(),
        verb: "check".into(),
        prompt: claim_prompt(claim, deps.languages),
        model: model.into(),
        tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
        cap_usd: deps.cap_usd,
        timeout: deps.timeout,
    };
    let got = deps.researcher.research(r, cancel)?;
    errands.push(got.errand);
    let reply = parse_reply(&got.answer).map_err(|why| Error::Reply {
        provider: model.into(),
        why,
    })?;
    let each: Vec<Option<Confirmed>> = reply
        .citations
        .iter()
        .map(|c| confirm_one(c, deps.view, deps.fences))
        .collect();
    let cited: Vec<Cited> = reply
        .citations
        .iter()
        .zip(&each)
        .map(|(c, ok)| Cited {
            raw: c.clone(),
            check: match ok {
                None => CitationCheck::NotConfirmed,
                Some(k) if k.unit_missed => CitationCheck::ConfirmedOnTheLineOnly,
                Some(_) => CitationCheck::Confirmed,
            },
        })
        .collect();
    let each = each.into_iter().map(|k| k.map(|k| k.citation));
    // every citation confirmed, or the verdict is cannot_tell
    Ok(match each.collect::<Option<Vec<Citation>>>() {
        Some(citations) => Attempt {
            verdict: reply.verdict,
            why: reply.why,
            citations,
            raw: reply.citations,
            cited,
        },
        None => Attempt {
            verdict: Verdict::CannotTell,
            why: CITATION_MISSED.into(),
            citations: Vec::new(),
            raw: reply.citations,
            cited,
        },
    })
}

/// A confirmed citation, and whether it stood on its line because the unit
/// it named was not found.
struct Confirmed {
    citation: Citation,
    unit_missed: bool,
}

/// C confirmed: inside the root, not fenced, readable; then the named
/// unit's span, else (no unit, or one the file does not define) the cited
/// line if it is inside the file, else, with neither unit nor line, the
/// whole file. A unit not found and no usable line fails.
fn confirm_one(c: &RawCitation, view: &dyn CodebaseView, fences: &Fences) -> Option<Confirmed> {
    let path = std::path::Path::new(&c.path);
    if !fences.allows(path) {
        return None;
    }
    let rel = fences.relative(path)?;
    let text = view.text(&rel).ok()?;
    let rows = text.lines().count().max(1) as u32;
    let named = c.unit.as_deref().and_then(|u| {
        view.units(&rel)
            .ok()
            .and_then(|us| unit_named(&us, u).map(|x| x.span))
    });
    let on_line = c
        .line
        .filter(|l| (1..=rows).contains(l))
        .map(|l| Span { start: l, end: l });
    let (lines, unit_missed) = match (named, &c.unit, c.line) {
        (Some(span), _, _) => (span, false),
        (None, Some(_), _) => (on_line?, true),
        (None, None, Some(_)) => (on_line?, false),
        (None, None, None) => (
            Span {
                start: 1,
                end: rows,
            },
            false,
        ),
    };
    Some(Confirmed {
        citation: Citation {
            print: Some(fingerprint(&text, lines).to_string()),
            file: rel,
            lines,
        },
        unit_missed,
    })
}

/// The unit called NAME: the exact qualified name (the earliest of several),
/// or a bare name that is the last part of exactly one qualified name. A
/// model may join a method with `.` or `::` whatever its language does, so
/// the two separators are read alike.
fn unit_named<'u>(units: &'u [Unit], name: &str) -> Option<&'u Unit> {
    let plain = |s: &str| s.replace("::", ".");
    let name = plain(name);
    if let Some(u) = units.iter().find(|u| plain(&u.name) == name) {
        return Some(u);
    }
    let ends: Vec<&Unit> = units
        .iter()
        .filter(|u| plain(&u.name).ends_with(&format!(".{name}")))
        .collect();
    if ends.len() == 1 { Some(ends[0]) } else { None }
}

/// What the guards see as evidence: up to three citations, each labelled
/// `path:Unit line N`, with 12 lines from 4 before the cited line, else the
/// unit's first 12, else the file's first 12.
fn guard_evidence(raw: &[RawCitation], view: &dyn CodebaseView, fences: &Fences) -> String {
    raw.iter()
        .take(3)
        .filter_map(|c| {
            let path = std::path::Path::new(&c.path);
            if !fences.allows(path) {
                return None;
            }
            let rel = fences.relative(path)?;
            let text = view.text(&rel).ok()?;
            let rows = text.lines().count() as u32;
            let named = c.unit.as_deref().and_then(|u| {
                view.units(&rel)
                    .ok()
                    .and_then(|us| unit_named(&us, u).map(|x| x.span.start))
            });
            let start = match (c.line, named) {
                (Some(l), _) if l >= 1 && l <= rows => l.saturating_sub(4).max(1),
                (_, Some(s)) => s,
                _ => 1,
            };
            let label = format!(
                "{}{}{}",
                rel.display(),
                c.unit
                    .as_deref()
                    .map(|u| format!(":{u}"))
                    .unwrap_or_default(),
                c.line.map(|l| format!(" line {l}")).unwrap_or_default()
            );
            let body: String = text
                .lines()
                .skip(start as usize - 1)
                .take(12)
                .map(|l| format!("{l}\n"))
                .collect();
            Some(format!("{label}\n{body}"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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
