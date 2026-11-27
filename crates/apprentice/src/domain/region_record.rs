//! What apprentice knows of each region it looked at (`regions.json`): Jev's answers, the
//! answer or the verdict once it is given, and whether the person dismissed it. A region at the
//! same text is never asked about twice.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sorcerer_core::domain::versioned::Versioned;

use crate::domain::region::Region;
use crate::domain::triage::{Kind, Triaged};

/// One region and what became of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionRecord {
    pub region: Region,
    pub triaged: Triaged,
    pub reply: Option<Reply>,
    pub dismissed: bool,
    pub at: DateTime<Utc>,
}

/// The answer to a question, or the verdict on a claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub verdict: ReplyVerdict,
    /// The answer, or why the claim does (not) hold.
    pub text: String,
    /// The code it rests on: a file relative to the root and a line, from 1.
    pub citations: Vec<(String, u32)>,
    pub cost_usd: f64,
    pub seconds: f64,
    /// Whether the answer was long, and so also written as a note.
    pub long: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplyVerdict {
    Answered,
    Holds,
    DoesNotHold,
    CannotTell,
    /// The call failed: the line says why, and the next save asks again.
    Failed,
}

impl RegionRecord {
    /// The kind apprentice acts on, unless the person dismissed it.
    pub fn acting_kind(&self) -> Option<Kind> {
        (!self.dismissed).then_some(self.triaged.kind).flatten()
    }

    /// Whether the region is something to do: an instruction, or a claim about intended
    /// behaviour that does not hold (an instruction in disguise).
    pub fn offers_a_task(&self) -> bool {
        match self.acting_kind() {
            Some(Kind::Instruction) => true,
            Some(Kind::Claim) => self
                .reply
                .as_ref()
                .is_some_and(|reply| reply.verdict == ReplyVerdict::DoesNotHold),
            _ => false,
        }
    }

    /// Whether it waits for an answer on save: a question or a claim with none yet, or whose
    /// last call failed.
    pub fn waits_for_a_reply(&self) -> bool {
        let unanswered = self
            .reply
            .as_ref()
            .is_none_or(|reply| reply.verdict == ReplyVerdict::Failed);
        matches!(self.acting_kind(), Some(Kind::Question | Kind::Claim)) && unanswered
    }
}

/// Every region record, by id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegionRecords {
    pub regions: BTreeMap<String, RegionRecord>,
}

impl Versioned for RegionRecords {
    const VERSION: u32 = 1;
}

/// One line of `labels.jsonl`: what Jev said a region was, and what the person says it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub region: Region,
    pub jev: Option<(Kind, f64)>,
    pub acted_as: Option<Kind>,
    /// The person's answer: a kind, or `None` for "nothing".
    pub person: Option<Kind>,
    /// `label` or `dismiss`.
    pub how: String,
    pub at: DateTime<Utc>,
}
