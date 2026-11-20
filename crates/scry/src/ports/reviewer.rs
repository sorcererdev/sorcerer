//! The reviewer: a model that reads a batch of changed units and says what
//! they get wrong.

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;

use crate::domain::review_prompt::ReviewAsk;

#[derive(Debug, Clone)]
pub struct Reviewed {
    pub reply: String,
    pub errand: Errand,
}

/// The seam the eval swaps for recorded replies.
pub trait Reviewer: Send + Sync {
    fn review(
        &self,
        ask: &ReviewAsk,
        model: &str,
        cap_usd: f64,
        cancel: &Cancel,
    ) -> sorcerer_models::Result<Reviewed>;
}
