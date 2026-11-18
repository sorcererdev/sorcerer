//! The claim checker: a statement about code ("`cancel` returns `None`
//! when the order is missing") checked by a researcher, the verdict
//! guarded by Jev. Ported from olifant's plans cascade (`Server/Cascade.hs`,
//! `Server/Checker.hs`), measured on the prose corpus at a 0.60 threshold:
//!
//! 1. With a decider, the first (cheap) model checks; three Jev guards ask
//!    whether its reason fits the claim, its evidence fits, and its verdict
//!    follows. A guard under the threshold, an unsure guard, a
//!    `cannot_tell`, or a failed first check sends it to the strong model.
//! 2. Without a decider, the strong model checks alone.
//! 3. Every citation is confirmed: inside the root, not fenced, the unit
//!    named found in the file, or the line inside it. One that is not makes
//!    the verdict `cannot_tell` ("cited code not found").

pub mod prompts;

use std::path::PathBuf;
use std::time::Duration;

use sorcerer_core::Cancel;
use sorcerer_core::fences::Fences;
use sorcerer_core::fingerprint::fingerprint;
use sorcerer_core::ledger::Errand;
use sorcerer_core::unit::{Span, Unit};
use sorcerer_core::view::CodebaseView;

use crate::decide::Decider;
use crate::research::{Research, Researcher, Tool};
use crate::{Error, Result};
use prompts::{RawCitation, claim_prompt, guard_questions, parse_reply};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub file: PathBuf,
    pub lines: Span,
    /// The fingerprint of the cited lines, once confirmed.
    pub print: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKind {
    Code,
    /// Checked against public web sources: a later milestone.
    Web,
}

#[derive(Debug, Clone)]
pub struct Claim {
    pub statement: String,
    /// Where the claim points, if anywhere (a hint for the checker).
    pub about: Vec<Citation>,
    pub kind: ClaimKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Right,
    Wrong,
    CannotTell,
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub verdict: Verdict,
    pub why: String,
    pub citations: Vec<Citation>,
    pub escalated: bool,
    pub errands: Vec<Errand>,
}

pub struct CheckerDeps<'a> {
    pub researcher: &'a dyn Researcher,
    pub decider: Option<&'a dyn Decider>,
    pub fences: &'a Fences,
    pub view: &'a dyn CodebaseView,
    /// The product the errands are recorded under.
    pub product: &'a str,
    pub first: &'a str,
    pub strong: &'a str,
    pub threshold: f64,
    pub cap_usd: f64,
    pub timeout: Duration,
}

/// What olifant measured: haiku first, sonnet on doubt, a 0.60 threshold.
pub const FIRST: &str = "haiku";
pub const STRONG: &str = "sonnet";
pub const THRESHOLD: f64 = 0.60;
pub const CITATION_MISSED: &str = "cited code not found";

impl<'a> CheckerDeps<'a> {
    pub fn defaults(
        researcher: &'a dyn Researcher,
        decider: Option<&'a dyn Decider>,
        fences: &'a Fences,
        view: &'a dyn CodebaseView,
        product: &'a str,
    ) -> CheckerDeps<'a> {
        CheckerDeps {
            researcher,
            decider,
            fences,
            view,
            product,
            first: FIRST,
            strong: STRONG,
            threshold: THRESHOLD,
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
}

pub fn check(claim: &Claim, deps: &CheckerDeps, cancel: &Cancel) -> Result<Checked> {
    if claim.kind == ClaimKind::Web {
        return Err(Error::Reply {
            provider: "checker".into(),
            why: "web claims wait for a later milestone".into(),
        });
    }
    let mut errands = Vec::new();
    let done = |a: Attempt, escalated: bool, errands: Vec<Errand>| Checked {
        verdict: a.verdict,
        why: a.why,
        citations: a.citations,
        escalated,
        errands,
    };
    let Some(decider) = deps.decider else {
        let a = attempt(claim, deps.strong, deps, cancel, &mut errands)?;
        return Ok(done(a, false, errands));
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
            return Ok(done(a, false, errands));
        }
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let a = attempt(claim, deps.strong, deps, cancel, &mut errands)?;
    Ok(done(a, true, errands))
}

fn attempt(
    claim: &Claim,
    model: &str,
    deps: &CheckerDeps,
    cancel: &Cancel,
    errands: &mut Vec<Errand>,
) -> Result<Attempt> {
    let r = Research {
        product: deps.product.into(),
        verb: "check".into(),
        prompt: claim_prompt(claim),
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
    Ok(match confirm(&reply.citations, deps) {
        Some(citations) => Attempt {
            verdict: reply.verdict,
            why: reply.why,
            citations,
            raw: reply.citations,
        },
        None => Attempt {
            verdict: Verdict::CannotTell,
            why: CITATION_MISSED.into(),
            citations: Vec::new(),
            raw: reply.citations,
        },
    })
}

/// Each citation confirmed, or None when any is not found.
fn confirm(raw: &[RawCitation], deps: &CheckerDeps) -> Option<Vec<Citation>> {
    raw.iter()
        .map(|c| confirm_one(c, deps.view, deps.fences))
        .collect()
}

fn confirm_one(c: &RawCitation, view: &dyn CodebaseView, fences: &Fences) -> Option<Citation> {
    let path = std::path::Path::new(&c.path);
    if !fences.allows(path) {
        return None;
    }
    let rel = fences.relative(path)?;
    let text = view.text(&rel).ok()?;
    let rows = text.lines().count().max(1) as u32;
    let lines = match (&c.unit, c.line) {
        (Some(u), _) => unit_named(&view.units(&rel).ok()?, u)?.span,
        (None, Some(l)) if (1..=rows).contains(&l) => Span { start: l, end: l },
        (None, Some(_)) => return None,
        (None, None) => Span {
            start: 1,
            end: rows,
        },
    };
    Some(Citation {
        print: Some(fingerprint(&text, lines).to_string()),
        file: rel,
        lines,
    })
}

/// The unit called NAME: the exact qualified name (the earliest of several),
/// or a bare name that is the last part of exactly one qualified name.
fn unit_named<'u>(units: &'u [Unit], name: &str) -> Option<&'u Unit> {
    if let Some(u) = units.iter().find(|u| u.name == name) {
        return Some(u);
    }
    let ends: Vec<&Unit> = units
        .iter()
        .filter(|u| u.name.ends_with(&format!(".{name}")) || u.name.ends_with(&format!("::{name}")))
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
