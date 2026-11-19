//! The claim checker, as a review calls it: is this statement about the
//! code right?

use sorcerer_models::domain::claim::{Checked, Claim};

pub trait ClaimChecker: Sync {
    fn check(&self, claim: &Claim) -> sorcerer_models::Result<Checked>;
}

/// A closure is a checker (the tests' doubles).
impl<F> ClaimChecker for F
where
    F: Fn(&Claim) -> sorcerer_models::Result<Checked> + Sync,
{
    fn check(&self, claim: &Claim) -> sorcerer_models::Result<Checked> {
        self(claim)
    }
}
