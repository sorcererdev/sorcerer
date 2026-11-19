//! The verification rounds' policy: which findings need a claim check, the
//! claim a finding makes, and a verdict's word.

use sorcerer_models::domain::claim::{Citation, Claim, ClaimKind, Verdict};

use crate::domain::finding::Finding;
use crate::domain::issue_type::{Verify, issue_type};

pub fn needs_claim(f: &Finding) -> bool {
    issue_type(&f.issue_type).is_some_and(|k| k.verify == Verify::CitationsThenClaim)
}

/// The finding's statement, pointed at its cited code.
pub fn claim_of(f: &Finding) -> Claim {
    Claim {
        statement: f.statement.clone(),
        about: f
            .evidence
            .iter()
            .map(|e| Citation {
                file: e.file.clone(),
                lines: e.lines,
                print: e.print.clone(),
            })
            .collect(),
        kind: ClaimKind::Code,
    }
}

pub fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Right => "right",
        Verdict::Wrong => "wrong",
        Verdict::CannotTell => "cannot_tell",
    }
}
