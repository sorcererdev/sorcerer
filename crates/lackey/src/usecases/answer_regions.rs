//! On save: each question in the file answered by one read-only research call, each claim
//! checked by models' claim checker (haiku first, Jev's guards, sonnet on doubt). Both are kept
//! on the region, so a region is answered once; a call that failed is asked again at the next
//! save. The watch is told when each call begins and ends, so the editor can show it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sorcerer_core::Cancel;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::unit::Span;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::domain::claim::{Citation, Claim, ClaimKind, Verdict};
use sorcerer_models::domain::research::{Research, Tool};
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;
use sorcerer_models::usecases::check_claim::{CheckClaimDeps, check_claim};

use crate::domain::answer_prompt::{question_prompt, read_answer};
use crate::domain::budget::{day_allows, spent_today};
use crate::domain::config::{LackeyConfig, PRODUCT};
use crate::domain::region_record::{RegionRecord, Reply, ReplyVerdict};
use crate::domain::triage::Kind;
use crate::ports::clock::Clock;
use crate::ports::region_store::RegionStore;
use crate::ports::reply_watch::ReplyWatch;

/// The verb a question's research call is recorded under.
pub const ANSWER_VERB: &str = "answer";
/// The most one answer may spend, in dollars, and how long it may take. A sonnet answer that
/// reads a few files costs about $0.10 (measured 2026-11-27: $0.096), so the cap leaves room
/// for one that reads more; `claude -p` stops itself at the cap and the answer fails.
const ANSWER_CAP: f64 = 0.25;
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);
/// The model a question is answered on.
const ANSWER_MODEL: &str = "sonnet";

/// What answering runs with.
pub struct AnswerDeps {
    /// The researcher on the project's provider; none when there is no provider.
    pub researcher: Option<Arc<dyn Researcher>>,
    pub decider: Option<Arc<dyn Decider>>,
    pub fences: Fences,
    /// The project as on disk (a save has just written the file).
    pub view: Arc<dyn CodebaseView>,
    pub languages: Languages,
    pub regions: Arc<dyn RegionStore>,
    pub ledger: Arc<dyn Ledger>,
    pub clock: Arc<dyn Clock>,
    pub config: LackeyConfig,
    pub watch: Arc<dyn ReplyWatch>,
}

/// Answers the questions and checks the claims of a saved file that wait for a reply, while the
/// day's budget allows; gives back the records it replied to.
///
/// Example: a saved claim "Cancelling less than two hours before the slot should be refused."
/// that the code does not do comes back `does-not-hold` with the line it rests on, and so also
/// offers a task.
pub fn answer_saved(
    file: &str,
    text: &str,
    deps: &AnswerDeps,
    cancel: &Cancel,
) -> Vec<RegionRecord> {
    let Some(researcher) = &deps.researcher else {
        return Vec::new();
    };
    let waiting: Vec<RegionRecord> = deps
        .regions
        .of_file(file)
        .into_iter()
        .filter(|record| record.waits_for_a_reply() && record.region.found_in(text).is_some())
        .collect();
    let mut replied = Vec::new();
    for record in waiting {
        if cancel.is_cancelled() || !day_affordable(deps) {
            break;
        }
        if !deps.watch.begins(&record) {
            continue;
        }
        let reply = still_waiting(&record, deps).and_then(|record| match record.acting_kind() {
            Some(Kind::Question) => Some(answered(&record, researcher.as_ref(), cancel)),
            Some(Kind::Claim) => Some(checked(&record, researcher.as_ref(), deps, cancel)),
            _ => None,
        });
        let Some(reply) = reply else {
            deps.watch.ends(&record);
            continue;
        };
        let record = RegionRecord {
            reply: Some(reply),
            ..record
        };
        let _ = deps.regions.put(std::slice::from_ref(&record));
        deps.watch.ends(&record);
        replied.push(record);
    }
    replied
}

/// The region as stored now, if it still waits: a save that started while another was
/// answering found it waiting, and the other may have answered it since.
fn still_waiting(record: &RegionRecord, deps: &AnswerDeps) -> Option<RegionRecord> {
    deps.regions
        .get(&record.region.id)
        .filter(RegionRecord::waits_for_a_reply)
}

fn day_affordable(deps: &AnswerDeps) -> bool {
    let errands = deps
        .ledger
        .read()
        .map(|rows| rows.errands)
        .unwrap_or_default();
    day_allows(
        spent_today(&errands, deps.clock.now(), deps.clock.offset()),
        &deps.config,
    )
}

/// One research call answering the question.
fn answered(record: &RegionRecord, researcher: &dyn Researcher, cancel: &Cancel) -> Reply {
    let started = Instant::now();
    let research = Research {
        product: PRODUCT.into(),
        verb: ANSWER_VERB.into(),
        prompt: question_prompt(&record.region),
        model: ANSWER_MODEL.into(),
        tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
        cap_usd: ANSWER_CAP,
        timeout: ANSWER_TIMEOUT,
    };
    match researcher.research(research, cancel) {
        Ok(researched) => {
            let (text, long, citations) = read_answer(&researched.answer);
            Reply {
                verdict: ReplyVerdict::Answered,
                text,
                citations,
                cost_usd: researched.errand.cost_usd.unwrap_or(0.0),
                seconds: started.elapsed().as_secs_f64(),
                long,
            }
        }
        Err(why) => failed(why.to_string(), started),
    }
}

/// The claim checked by models' cascade, about the region's own lines.
fn checked(
    record: &RegionRecord,
    researcher: &dyn Researcher,
    deps: &AnswerDeps,
    cancel: &Cancel,
) -> Reply {
    let started = Instant::now();
    let claim = Claim {
        statement: record.region.text.trim().to_string(),
        about: vec![Citation {
            file: PathBuf::from(&record.region.file),
            lines: record.region.lines,
            print: None,
        }],
        kind: ClaimKind::Code,
    };
    let mut check = CheckClaimDeps::defaults(
        researcher,
        deps.decider.as_deref(),
        &deps.fences,
        deps.view.as_ref(),
        PRODUCT,
    );
    check.languages = Some(&deps.languages);
    match check_claim(&claim, &check, cancel) {
        Ok(checked) => Reply {
            verdict: match checked.verdict {
                Verdict::Right => ReplyVerdict::Holds,
                Verdict::Wrong => ReplyVerdict::DoesNotHold,
                Verdict::CannotTell => ReplyVerdict::CannotTell,
            },
            text: checked.why,
            citations: checked
                .citations
                .iter()
                .map(|citation| {
                    (
                        sorcerer_core::domain::paths::forward_slashes(&citation.file),
                        first_line(citation.lines),
                    )
                })
                .collect(),
            cost_usd: checked
                .errands
                .iter()
                .filter_map(|errand| errand.cost_usd)
                .sum(),
            seconds: started.elapsed().as_secs_f64(),
            long: false,
        },
        Err(why) => failed(why.to_string(), started),
    }
}

fn first_line(lines: Span) -> u32 {
    lines.start
}

fn failed(why: String, started: Instant) -> Reply {
    Reply {
        verdict: ReplyVerdict::Failed,
        text: why,
        citations: Vec::new(),
        cost_usd: 0.0,
        seconds: started.elapsed().as_secs_f64(),
        long: false,
    }
}
