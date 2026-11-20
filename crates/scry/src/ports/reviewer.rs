//! The reviewer: a model that reads a batch of changed units and says what
//! they get wrong.

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;

use crate::domain::review_prompt::ReviewAsk;

/// A review call's result: the model's reply, and the ledger row that
/// records what the call cost.
#[derive(Debug, Clone)]
pub struct Reviewed {
    pub reply: String,
    pub errand: Errand,
}

/// Makes a review call on `model`, spending at most `cap_usd`. The eval
/// stands in recorded replies here.
pub trait Reviewer: Send + Sync {
    fn review(
        &self,
        ask: &ReviewAsk,
        model: &str,
        cap_usd: f64,
        cancel: &Cancel,
    ) -> sorcerer_models::Result<Reviewed>;
}
