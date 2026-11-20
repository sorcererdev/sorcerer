//! The reviewer: a review call made as research, by Claude Code or by
//! sorcerer's own tool loop over another provider (the wiring chooses).
//! The researcher may read, grep and glob the project to back a finding.

use std::sync::Arc;

use sorcerer_core::Cancel;
use sorcerer_models::domain::research::{Research, Tool};
use sorcerer_models::ports::researcher::Researcher;

use crate::domain::review_prompt::{REVIEW_TIMEOUT, ReviewAsk, review_prompt};
use crate::ports::reviewer::{Reviewed, Reviewer};

/// Makes review calls through a researcher.
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
        let researched = self.researcher.research(
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
            reply: researched.answer,
            errand: researched.errand,
        })
    }
}
