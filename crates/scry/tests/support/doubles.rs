//! Every double a review needs, wired together.

use std::path::Path;

use super::{FixedClock, Ports, StubDecider, StubReviewer, project_with_vendored_grammars};
use scry::adapters::wiring::Wiring;
use scry::ports::claim_checker::ClaimChecker;
use scry::ports::run_log::Quiet;
use scry::usecases::review_change::{Progress, ReviewDeps};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_models::domain::claim::{Checked, Claim, Escalation, Verdict};

/// The pass's doubles: the vendored grammars' view, a stub reviewer and
/// decider, a fixed clock (2026-11-18 23:00 UTC), a claim checker that says
/// right, and one cancel for everything.
pub struct Doubles {
    pub view: FilesView,
    pub reviewer: StubReviewer,
    pub decider: StubDecider,
    pub clock: FixedClock,
    pub checker: Box<dyn ClaimChecker + Send>,
    pub ports: Ports,
    pub cancel: Cancel,
}

fn quiet(_: &Progress) {}

impl Doubles {
    pub fn new(dir: &Path, reviewer: StubReviewer) -> Doubles {
        let (project, view) = project_with_vendored_grammars(dir);
        Doubles {
            view,
            ports: Ports::of(&project),
            cancel: Cancel::default(),
            reviewer,
            decider: StubDecider::new(),
            clock: FixedClock::at("2026-11-18T23:00:00Z".parse().unwrap()),
            checker: Box::new(|_: &Claim| {
                Ok(Checked {
                    verdict: Verdict::Right,
                    why: "it does".into(),
                    citations: Vec::new(),
                    cited: Vec::new(),
                    escalation: Escalation::No,
                    errands: Vec::new(),
                })
            }),
        }
    }
}

impl Wiring for Doubles {
    fn deps(&self) -> ReviewDeps<'_> {
        ReviewDeps {
            view: &self.view,
            reviewer: &self.reviewer,
            decider: &self.decider,
            checker: &*self.checker,
            store: &self.ports.store,
            ledger: &self.ports.ledger,
            settings: &self.ports.settings,
            clock: &self.clock,
            log: &Quiet,
            progress: &quiet,
            cancel: &self.cancel,
            review_cancel: &self.cancel,
        }
    }
}

/// The command line's tests hand `main_with` a borrowed `Doubles`.
impl Wiring for &Doubles {
    fn deps(&self) -> ReviewDeps<'_> {
        (**self).deps()
    }
}
