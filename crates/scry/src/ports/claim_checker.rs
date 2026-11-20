//! The claim checker, as a review calls it: is this statement about the
//! code right?

use sorcerer_models::domain::claim::{Checked, Claim};

pub trait ClaimChecker: Sync {
    /// Whether the claim is right, wrong, or cannot be told, with what the
    /// check cited.
    fn check(&self, claim: &Claim) -> sorcerer_models::Result<Checked>;
}

/// A closure can stand in as a checker, as the tests' doubles do.
impl<F> ClaimChecker for F
where
    F: Fn(&Claim) -> sorcerer_models::Result<Checked> + Sync,
{
    fn check(&self, claim: &Claim) -> sorcerer_models::Result<Checked> {
        self(claim)
    }
}
