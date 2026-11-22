//! The first writer: one plain model call through the project's provider.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_models::domain::model::{Completion, Message, Reply};
use sorcerer_models::ports::provider::Provider;

use crate::domain::change_prompt::CHANGE_SYSTEM;
use crate::domain::config::PRODUCT;
use crate::ports::clock::Clock;
use crate::ports::writer::{Writer, Written};

/// The first call may write up to 8192 tokens.
const MAX_TOKENS: u32 = 8192;
/// The first call is given 120 seconds.
const TIMEOUT: Duration = Duration::from_secs(120);

/// What a call that failed after it was sent is counted as in the day's spending: a timed-out
/// call is usually billed, and the runner says nothing of its cost. A tenth of the day's
/// default budget, so a run of timeouts cannot go unnoticed.
pub const FAILED_CALL_ESTIMATE_USD: f64 = 0.02;

/// Whether the call reached the model: a missing key or an unknown alias never did.
fn was_sent(error: &sorcerer_models::Error) -> bool {
    use sorcerer_models::Error;
    !matches!(
        error,
        Error::NoKey { .. } | Error::UnknownAlias { .. } | Error::Setup { .. }
    )
}

fn failure_note(error: &sorcerer_models::Error) -> String {
    if was_sent(error) {
        format!("{error} (counted as ${FAILED_CALL_ESTIMATE_USD:.2}, an estimate)")
    } else {
        error.to_string()
    }
}

pub struct ProviderWriter {
    pub provider: Arc<dyn Provider>,
    /// The model alias, such as `haiku`.
    pub model: String,
    pub ledger: Arc<dyn Ledger>,
    pub clock: Arc<dyn Clock>,
}

impl Writer for ProviderWriter {
    /// Asks the model once with the situation as the only message, and records the call in the
    /// ledger as a `conjure` errand before returning, a failed call too.
    fn write(&self, situation: &str, cancel: &Cancel) -> sorcerer_models::Result<Written> {
        let started = Instant::now();
        let asked = self.ask(situation, cancel);
        let seconds = started.elapsed().as_secs_f64();
        let errand = self.errand_of(&asked, seconds);
        self.ledger.append(&errand)?;
        asked.map(|reply| Written {
            text: reply.text,
            errand,
        })
    }

    fn model(&self) -> &str {
        &self.model
    }
}

impl ProviderWriter {
    fn ask(&self, situation: &str, cancel: &Cancel) -> sorcerer_models::Result<Reply> {
        let completion = Completion {
            model: self.provider.resolve(&self.model)?,
            system: CHANGE_SYSTEM.to_string(),
            messages: vec![Message::user(situation)],
            tools: vec![],
            max_tokens: MAX_TOKENS,
            timeout: TIMEOUT,
        };
        self.provider.complete(completion, cancel)
    }

    fn errand_of(&self, asked: &sorcerer_models::Result<Reply>, seconds: f64) -> Errand {
        let (cost_usd, usage) = match asked {
            Ok(reply) => (reply.cost_usd, reply.usage),
            Err(e) if was_sent(e) => (Some(FAILED_CALL_ESTIMATE_USD), Default::default()),
            Err(_) => (None, Default::default()),
        };
        Errand {
            at: self.clock.now(),
            product: PRODUCT.into(),
            verb: "conjure".into(),
            model: self.model.clone(),
            provider: self.provider.name().to_string(),
            cost_usd,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            seconds: asked.as_ref().map_or(seconds, |reply| {
                if reply.seconds > 0.0 {
                    reply.seconds
                } else {
                    seconds
                }
            }),
            ok: asked.is_ok(),
            note: asked.as_ref().err().map(failure_note),
        }
    }
}
