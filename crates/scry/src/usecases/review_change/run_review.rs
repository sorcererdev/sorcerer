//! `review_change`: setting a review up, running one pass, and the run log's
//! opening text.

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::versioned::Snapshot;
use sorcerer_core::{Error, Result};

use super::request_and_report::{Progress, ReviewDeps, ReviewOutcome, ReviewRequest};
use super::review_pass::{Halt, Pass};
use super::scope::scope_of;
use super::spent_so_far::SpentSoFar;
use crate::domain::budget::Caps;
use crate::domain::config::ScryConfig;
use crate::domain::findings::Findings;
use crate::domain::log_text::{count, scope_text, section, usd};
use crate::ports::finding_store::FindingStore;

/// Reviews the change the request names and verifies what the reviewer
/// found:
///
/// 1. reads `[scry]`; when scry is turned off, it skips;
/// 2. works out what to review (the scope) and logs it;
/// 3. works out the spending limits, and reads the project's rules
///    (steering) and the files scry must not read (fences);
/// 4. locks the finding store, so two reviews never run at once;
/// 5. reads the stored findings and what scry spent today, and logs them;
/// 6. runs the pass (review, judge, remember, verify) and saves its result
///    over the store as it is then;
/// 7. tells the caller the store was written, and unlocks it.
///
/// When Jev cannot answer one of its questions the whole review is skipped
/// and nothing is written: scry does not run on a guess.
pub fn review_change(
    project: &Project,
    req: &ReviewRequest,
    deps: &ReviewDeps,
) -> Result<ReviewOutcome> {
    let cfg = deps.settings.scry_config()?;
    if !cfg.enabled {
        return Ok(ReviewOutcome::Skipped(SCRY_IS_OFF.into()));
    }
    let scope = scope_of(project, deps.view, req.scope.clone(), req.all, &cfg)?;
    deps.log.put(&scope_text(&scope));
    let caps = caps_for(req, &cfg);
    let steering = deps.settings.steering()?;
    let fences = Fences::for_project(project)?;

    let store_lock = deps.store.hold()?;
    let stored_before = findings_on_disk(deps.store)?;
    let spent = SpentSoFar::read(deps.ledger, deps.clock, &cfg)?;
    deps.log
        .put(&pass_text(&caps, &spent, &cfg.review_model, &stored_before));
    let pass = Pass::new(&cfg, &steering, &fences, deps, &caps, &spent, stored_before);
    let report = match pass.run(&scope, req.fresh) {
        Ok(report) => report,
        Err(Halt::JevDidNotAnswer) => return Ok(ReviewOutcome::Skipped(JEV_DID_NOT_ANSWER.into())),
        Err(Halt::Failed(e)) => return Err(e),
    };
    (deps.progress)(&Progress::Stored);
    drop(store_lock);
    Ok(ReviewOutcome::Ran(report))
}

/// The spending limits: the request's own when it gives them (the editor,
/// tests); else the command line's: no limit per pass, `[scry]
/// budget_per_day` for the day, and `--budget` when given.
fn caps_for(req: &ReviewRequest, cfg: &ScryConfig) -> Caps {
    req.caps.clone().unwrap_or(Caps {
        pass: None,
        day: cfg.budget_per_day,
        budget: req.budget,
    })
}

/// The stored findings: none when there is no store yet. A store scry
/// cannot read is an error, so the pass never writes over it.
fn findings_on_disk(store: &dyn FindingStore) -> Result<Findings> {
    match store.read()? {
        Snapshot::Absent => Ok(Findings::default()),
        Snapshot::Current(s) => Ok(s),
        Snapshot::Unreadable(why) => Err(Error::StoreUnreadable {
            path: store.path().to_path_buf(),
            why,
        }),
    }
}

// The run log's text, and the reasons a review is skipped.

const SCRY_IS_OFF: &str = "scry is off ([scry] enabled = false)";

const JEV_DID_NOT_ANSWER: &str = "Jev did not answer";

/// The `Pass` section: the caps, what scry spent earlier today, the
/// estimates and the store as the pass found it.
fn pass_text(caps: &Caps, spent: &SpentSoFar, review_model: &str, start: &Findings) -> String {
    format!(
        "{}- caps: pass {}, day {} (scry spent {} earlier today), budget {}\n\
         - estimates: review {} ({}), check {}\n\
         - store: {}, {} reviewed before\n",
        section("Pass"),
        caps.pass.map_or("none".into(), usd),
        usd(caps.day),
        usd(spent.today),
        caps.budget.map_or("none".into(), usd),
        usd(spent.review_estimate),
        review_model,
        usd(spent.check_estimate),
        count(start.findings.len(), "finding"),
        count(start.reviewed.len(), "file"),
    )
}
