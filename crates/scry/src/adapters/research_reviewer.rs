//! The reviewer over a researcher: Claude Code, or sorcerer's tool loop
//! over any provider (the wiring chooses).

use std::sync::Arc;

use sorcerer_core::Cancel;
use sorcerer_models::domain::research::{Research, Tool};
use sorcerer_models::ports::researcher::Researcher;

use crate::domain::review_prompt::{REVIEW_TIMEOUT, ReviewAsk, review_prompt};
use crate::ports::reviewer::{Reviewed, Reviewer};

/// The review call over a researcher (Claude Code, or the tool loop over a provider).
pub struct ResearchReviewer {
    pub researcher: Arc<dyn Researcher>,
}

impl Reviewer for ResearchReviewer {
    fn review(
        &self,
        ask: &ReviewAsk,
        model: &str,
        cap_usd: f64,
        cancel: &Cancel,
    ) -> sorcerer_models::Result<Reviewed> {
        let r = self.researcher.research(
            Research {
                product: "scry".into(),
                verb: "review".into(),
                prompt: review_prompt(ask),
                model: model.into(),
                tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
                cap_usd,
                timeout: REVIEW_TIMEOUT,
            },
            cancel,
        )?;
        Ok(Reviewed {
            reply: r.answer,
            errand: r.errand,
        })
    }
}
