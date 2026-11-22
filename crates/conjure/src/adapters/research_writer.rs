//! The Try harder writer: a stronger model that may look around the project before it writes.

use std::sync::Arc;
use std::time::Duration;

use sorcerer_core::Cancel;
use sorcerer_models::domain::research::{Research, Tool};
use sorcerer_models::ports::researcher::Researcher;

use crate::domain::budget::TRY_HARDER_CAP_USD;
use crate::domain::change_prompt::CHANGE_SYSTEM;
use crate::domain::config::PRODUCT;
use crate::ports::writer::{Writer, Written};

/// Try harder is given 300 seconds.
const TIMEOUT: Duration = Duration::from_secs(300);

pub struct ResearchWriter {
    pub researcher: Arc<dyn Researcher>,
    /// The model alias, such as `sonnet`.
    pub model: String,
}

impl Writer for ResearchWriter {
    /// Asks the model with the system words and the situation as one prompt, and the tools
    /// Read, Grep and Glob. The researcher records its own call in the ledger as a
    /// `try-harder` errand. Nothing it does writes a file.
    fn write(&self, situation: &str, cancel: &Cancel) -> sorcerer_models::Result<Written> {
        let researched = self.researcher.research(
            Research {
                product: PRODUCT.into(),
                verb: "try-harder".into(),
                prompt: format!("{CHANGE_SYSTEM}\n\n{situation}"),
                model: self.model.clone(),
                tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
                cap_usd: TRY_HARDER_CAP_USD,
                timeout: TIMEOUT,
            },
            cancel,
        )?;
        Ok(Written {
            text: researched.answer,
            errand: researched.errand,
        })
    }

    fn model(&self) -> &str {
        &self.model
    }
}
