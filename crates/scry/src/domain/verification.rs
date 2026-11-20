//! What verifying a finding needs: whether it gets a claim check on top of
//! its citations, the claim the checker is handed, and the verdict's word
//! as the store keeps it.

use sorcerer_models::domain::claim::{Citation, Claim, ClaimKind, Verdict};

use crate::domain::finding::Finding;
use crate::domain::issue_type::{Verify, issue_type};

/// Whether a finding needs the claim checker's own call after its
/// citations check out: bugs and assumptions do; conventions and design
/// findings stand on their citations alone.
pub fn needs_claim(finding: &Finding) -> bool {
    issue_type(&finding.issue_type).is_some_and(|known| known.verify == Verify::CitationsThenClaim)
}

/// What the claim checker is asked to judge: the finding's statement,
/// about the code its evidence cites.
pub fn claim_of(finding: &Finding) -> Claim {
    let cited_code = finding
        .evidence
        .iter()
        .map(|evidence| Citation {
            file: evidence.file.clone(),
            lines: evidence.lines,
            print: evidence.print.clone(),
        })
        .collect();
    Claim {
        statement: finding.statement.clone(),
        about: cited_code,
        kind: ClaimKind::Code,
    }
}

/// A verdict as the store writes it: `right`, `wrong` or `cannot_tell`.
pub fn verdict_word(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Right => "right",
        Verdict::Wrong => "wrong",
        Verdict::CannotTell => "cannot_tell",
    }
}
