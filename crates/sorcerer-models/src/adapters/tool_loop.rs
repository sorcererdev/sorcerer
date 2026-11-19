//! Core's own tool loop, so any provider that supports tools can research:
//! send the prompt; while the model asks for tools, run them under fences
//! and send the results back; stop at the turn cap or the spend cap. One
//! errand for the whole research, its usage and cost summed over the turns.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;

use crate::adapters::tools;
use crate::domain::model::{Completion, Content, Message, Role, StopReason, Usage};
use crate::domain::research::{Research, Researched};
use crate::ports::provider::Provider;
use crate::ports::researcher::Researcher;
use crate::{Error, Result};

const FENCE_PROMPT: &str = include_str!("../../prompts/fence.md");
pub const MAX_TURNS: u32 = 12;
const MAX_TOKENS: u32 = 4096;

pub struct LoopResearcher {
    provider: Arc<dyn Provider>,
    view: Arc<dyn CodebaseView>,
    fences: Fences,
    ledger: Arc<dyn Ledger>,
    max_turns: u32,
}

impl LoopResearcher {
    pub fn new(
        provider: Arc<dyn Provider>,
        view: Arc<dyn CodebaseView>,
        fences: Fences,
        ledger: Arc<dyn Ledger>,
    ) -> LoopResearcher {
        LoopResearcher {
            provider,
            view,
            fences,
            ledger,
            max_turns: MAX_TURNS,
        }
    }
}

impl Researcher for LoopResearcher {
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched> {
        let started = Instant::now();
        let deadline = started + r.timeout;
        let mut usage = Usage::default();
        let mut cost: Option<f64> = None;
        let mut reads: Vec<PathBuf> = Vec::new();
        let errand = |usage: Usage, cost: Option<f64>, ok: bool, note: Option<String>| Errand {
            at: chrono::Utc::now(),
            product: r.product.clone(),
            verb: r.verb.clone(),
            model: r.model.clone(),
            provider: self.provider.name().into(),
            cost_usd: cost,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            seconds: started.elapsed().as_secs_f64(),
            ok,
            note,
        };
        let fail = |e: Error, usage: Usage, cost: Option<f64>| -> Result<Researched> {
            let charged = if matches!(e, Error::Timeout { .. }) {
                Some(r.cap_usd)
            } else {
                cost
            };
            self.ledger
                .append(&errand(usage, charged, false, Some(e.to_string())))?;
            Err(e)
        };
        let model = match self.provider.resolve(&r.model) {
            Ok(m) => m,
            Err(e) => return fail(e, usage, cost),
        };
        let system = FENCE_PROMPT
            .trim()
            .replace("{fences}", &self.fences.patterns().join(", "));
        let specs = tools::specs(&r.tools);
        let mut messages = vec![Message::user(&r.prompt)];
        let mut turn = 0;
        let (answer, ok, note) = loop {
            turn += 1;
            let now = Instant::now();
            if now >= deadline {
                return fail(
                    Error::Timeout {
                        provider: self.provider.name().into(),
                        seconds: r.timeout.as_secs(),
                    },
                    usage,
                    cost,
                );
            }
            let req = Completion {
                model: model.clone(),
                system: system.clone(),
                messages: messages.clone(),
                tools: specs.clone(),
                max_tokens: MAX_TOKENS,
                timeout: deadline - now,
            };
            let reply = match self.provider.complete(req, cancel) {
                Ok(reply) => reply,
                Err(e) => return fail(e, usage, cost),
            };
            usage.input_tokens += reply.usage.input_tokens;
            usage.output_tokens += reply.usage.output_tokens;
            if let Some(c) = reply.cost_usd {
                cost = Some(cost.unwrap_or(0.0) + c);
            }
            if reply.stop != StopReason::ToolUse || reply.tool_calls.is_empty() {
                break (reply.text, true, None);
            }
            if turn >= self.max_turns {
                break (
                    reply.text,
                    false,
                    Some(format!("stopped at the turn cap ({})", self.max_turns)),
                );
            }
            if cost.unwrap_or(0.0) >= r.cap_usd {
                break (
                    reply.text,
                    false,
                    Some(format!("stopped at the spend cap (${:.2})", r.cap_usd)),
                );
            }
            if cancel.is_cancelled() {
                return fail(Error::Cancelled, usage, cost);
            }
            let mut said: Vec<Content> = Vec::new();
            if !reply.text.is_empty() {
                said.push(Content::Text(reply.text.clone()));
            }
            let mut results: Vec<Content> = Vec::new();
            for call in &reply.tool_calls {
                said.push(Content::ToolCall(call.clone()));
                let (text, read) = tools::run(call, self.view.as_ref(), &self.fences);
                if let Some(p) = read
                    && !reads.contains(&p)
                {
                    reads.push(p);
                }
                results.push(Content::ToolResult {
                    id: call.id.clone(),
                    text,
                });
            }
            messages.push(Message {
                role: Role::Assistant,
                content: said,
            });
            messages.push(Message {
                role: Role::User,
                content: results,
            });
        };
        let e = errand(usage, cost, ok, note);
        self.ledger.append(&e)?;
        Ok(Researched {
            answer,
            errand: e,
            reads,
        })
    }
}
