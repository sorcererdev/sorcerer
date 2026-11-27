//! Core's own tool loop, so any provider that supports tools can research:
//! send the prompt; while the model asks for tools, run them under fences
//! and send the results back; stop at the turn cap or the spend cap. One
//! errand for the whole research, its usage and cost summed over the turns.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sorcerer_core::Cancel;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;

use crate::adapters::tools;
use crate::domain::model::{
    Completion, Content, Message, ModelId, Reply, Role, StopReason, ToolSpec, Usage,
};
use crate::domain::research::{Research, Researched};
use crate::ports::provider::Provider;
use crate::ports::researcher::Researcher;
use crate::{Error, Result};

const FENCE_PROMPT: &str = include_str!("../../prompts/fence.md");
/// The most turns one research may take: each turn is one call to the model.
pub const MAX_TURNS: u32 = 12;
const MAX_TOKENS: u32 = 4096;

pub struct LoopResearcher {
    provider: Arc<dyn Provider>,
    view: Arc<dyn CodebaseView>,
    fences: Fences,
    ledger: Arc<dyn Ledger>,
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
        }
    }
}

impl Researcher for LoopResearcher {
    /// Lets the model read the project until it answers, then records the whole research
    /// as one line in the ledger, whether it worked or not.
    ///
    /// Each turn sends the conversation so far and the time left. When the model asks for
    /// tools, they run under the fences and their results go back on the next turn. The
    /// research stops with the model's last text at 12 turns or once it has spent its cap;
    /// a timeout, a cancel or a failed call stops it with an error.
    ///
    /// Example: the model greps, then reads two files, then answers: three turns, one
    /// ledger line with the three calls' tokens and cost added up, and the two files
    /// listed as read.
    fn research(&self, r: Research, cancel: &Cancel) -> Result<Researched> {
        let run = ResearchRun {
            researcher: self,
            research: &r,
            cancel,
            started: Instant::now(),
        };
        match run.take_turns() {
            Ok(ended) => {
                let errand =
                    run.errand(&ended.so_far.spent, ended.how.note(&r), ended.how.outcome());
                self.ledger.append(&errand)?;
                Ok(Researched {
                    answer: ended.answer,
                    errand,
                    reads: ended.so_far.reads,
                })
            }
            Err(failed) => {
                let charged = run.charged_for_failure(&failed);
                let errand = run.errand(&charged, Some(failed.error.to_string()), Outcome::Failed);
                self.ledger.append(&errand)?;
                Err(failed.error)
            }
        }
    }
}

/// One research in progress: who runs it, what was asked, and when it started.
struct ResearchRun<'a> {
    researcher: &'a LoopResearcher,
    research: &'a Research,
    cancel: &'a Cancel,
    started: Instant,
}

/// What every turn sends besides the conversation: the model, the fence prompt as the
/// system prompt, and the tools the model may ask for.
struct SentEveryTurn {
    model: ModelId,
    system: String,
    tools: Vec<ToolSpec>,
}

/// What the research has said, spent and read so far.
struct SoFar {
    conversation: Vec<Message>,
    spent: Spent,
    reads: Vec<PathBuf>,
}

/// The tokens and dollars spent so far, added up over the turns. The cost stays unknown
/// until a turn reports one.
#[derive(Clone, Copy, Default)]
struct Spent {
    usage: Usage,
    cost: Option<f64>,
}

/// A research that stopped without an error: the model's last text and why it stopped.
struct Ended {
    answer: String,
    how: HowItEnded,
    so_far: SoFar,
}

enum HowItEnded {
    /// The model answered without asking for a tool.
    Answered,
    TurnCapReached,
    SpendCapReached,
}

/// A research that stopped with an error, and what it had spent by then.
struct Failed {
    error: Error,
    spent: Spent,
}

/// Whether the ledger records the research as having worked.
#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    Worked,
    Failed,
}

impl ResearchRun<'_> {
    fn take_turns(&self) -> std::result::Result<Ended, Failed> {
        let provider = &self.researcher.provider;
        let model = provider
            .resolve(&self.research.model)
            .map_err(|error| Failed {
                error,
                spent: Spent::default(),
            })?;
        let every_turn = SentEveryTurn {
            model,
            system: FENCE_PROMPT
                .trim()
                .replace("{fences}", &self.researcher.fences.described()),
            tools: tools::specs(&self.research.tools),
        };
        let so_far = SoFar {
            conversation: vec![Message::user(&self.research.prompt)],
            spent: Spent::default(),
            reads: Vec::new(),
        };
        self.turn(1, &every_turn, so_far)
    }

    /// One turn: ask the model, then stop, or run the tools it asked for and take the next.
    fn turn(
        &self,
        number: u32,
        every_turn: &SentEveryTurn,
        so_far: SoFar,
    ) -> std::result::Result<Ended, Failed> {
        let provider = &self.researcher.provider;
        let Some(time_left) = self.time_left() else {
            return Err(so_far.failed(Error::Timeout {
                provider: provider.name().into(),
                seconds: self.research.timeout.as_secs(),
            }));
        };
        let request = Completion {
            model: every_turn.model.clone(),
            system: every_turn.system.clone(),
            messages: so_far.conversation.clone(),
            tools: every_turn.tools.clone(),
            max_tokens: MAX_TOKENS,
            timeout: time_left,
        };
        let reply = match provider.complete(request, self.cancel) {
            Ok(reply) => reply,
            Err(error) => return Err(so_far.failed(error)),
        };
        let so_far = so_far.charged_for(&reply);
        if reply.stop != StopReason::ToolUse || reply.tool_calls.is_empty() {
            return Ok(so_far.ended(reply.text, HowItEnded::Answered));
        }
        if number >= MAX_TURNS {
            return Ok(so_far.ended(reply.text, HowItEnded::TurnCapReached));
        }
        if so_far.spent.dollars() >= self.research.cap_usd {
            return Ok(so_far.ended(reply.text, HowItEnded::SpendCapReached));
        }
        if self.cancel.is_cancelled() {
            return Err(so_far.failed(Error::Cancelled));
        }
        let so_far = self.after_running_tools(&reply, so_far);
        self.turn(number + 1, every_turn, so_far)
    }

    fn time_left(&self) -> Option<Duration> {
        (self.started + self.research.timeout)
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
    }

    /// Runs each tool the model asked for, then adds the model's turn (its text and its
    /// calls) and the tools' results to the conversation. A file read twice is listed once.
    fn after_running_tools(&self, reply: &Reply, so_far: SoFar) -> SoFar {
        let (results, reads) = reply.tool_calls.iter().fold(
            (Vec::new(), so_far.reads),
            |(mut results, mut reads), call| {
                let output =
                    tools::run(call, self.researcher.view.as_ref(), &self.researcher.fences);
                if let Some(path) = output.read
                    && !reads.contains(&path)
                {
                    reads.push(path);
                }
                results.push(Content::ToolResult {
                    id: call.id.clone(),
                    text: output.text,
                });
                (results, reads)
            },
        );
        let text = (!reply.text.is_empty()).then(|| Content::Text(reply.text.clone()));
        let calls = reply.tool_calls.iter().cloned().map(Content::ToolCall);
        let model_turn = Message {
            role: Role::Assistant,
            content: text.into_iter().chain(calls).collect(),
        };
        let tool_results = Message {
            role: Role::User,
            content: results,
        };
        SoFar {
            conversation: so_far
                .conversation
                .into_iter()
                .chain([model_turn, tool_results])
                .collect(),
            spent: so_far.spent,
            reads,
        }
    }

    /// A research that ran out of time was probably paid for, so it is charged its whole
    /// cap; any other failure is charged what its turns reported.
    fn charged_for_failure(&self, failed: &Failed) -> Spent {
        match failed.error {
            Error::Timeout { .. } => Spent {
                cost: Some(self.research.cap_usd),
                ..failed.spent
            },
            _ => failed.spent,
        }
    }

    fn errand(&self, spent: &Spent, note: Option<String>, outcome: Outcome) -> Errand {
        Errand {
            at: chrono::Utc::now(),
            product: self.research.product.clone(),
            verb: self.research.verb.clone(),
            model: self.research.model.clone(),
            provider: self.researcher.provider.name().into(),
            cost_usd: spent.cost,
            input_tokens: spent.usage.input_tokens,
            output_tokens: spent.usage.output_tokens,
            seconds: self.started.elapsed().as_secs_f64(),
            ok: outcome == Outcome::Worked,
            note,
        }
    }
}

impl SoFar {
    fn charged_for(self, reply: &Reply) -> SoFar {
        SoFar {
            spent: self.spent.plus(reply),
            ..self
        }
    }

    fn ended(self, answer: String, how: HowItEnded) -> Ended {
        Ended {
            answer,
            how,
            so_far: self,
        }
    }

    fn failed(self, error: Error) -> Failed {
        Failed {
            error,
            spent: self.spent,
        }
    }
}

impl Spent {
    fn plus(self, reply: &Reply) -> Spent {
        Spent {
            usage: Usage {
                input_tokens: self.usage.input_tokens + reply.usage.input_tokens,
                output_tokens: self.usage.output_tokens + reply.usage.output_tokens,
            },
            cost: match (self.cost, reply.cost_usd) {
                (cost, None) => cost,
                (cost, Some(this_turn)) => Some(cost.unwrap_or(0.0) + this_turn),
            },
        }
    }

    fn dollars(&self) -> f64 {
        self.cost.unwrap_or(0.0)
    }
}

impl HowItEnded {
    fn outcome(&self) -> Outcome {
        match self {
            HowItEnded::Answered => Outcome::Worked,
            HowItEnded::TurnCapReached | HowItEnded::SpendCapReached => Outcome::Failed,
        }
    }

    /// What the ledger says about a research that stopped short.
    fn note(&self, r: &Research) -> Option<String> {
        match self {
            HowItEnded::Answered => None,
            HowItEnded::TurnCapReached => Some(format!("stopped at the turn cap ({MAX_TURNS})")),
            HowItEnded::SpendCapReached => {
                Some(format!("stopped at the spend cap (${:.2})", r.cap_usd))
            }
        }
    }
}
