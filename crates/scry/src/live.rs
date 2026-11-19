//! The live wiring of a pass: the project's view, the review call through a
//! researcher on the chosen provider, Jev, and the claim checker, each
//! recording its errands as scry's (`review`, `check`, `decide`).

use std::sync::Arc;

use anyhow::{Context, Result};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Languages;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::claude_code::ClaudeCodeResearcher;
use sorcerer_models::adapters::jev::Jev;
use sorcerer_models::adapters::providers::default_provider_for;
use sorcerer_models::adapters::tool_loop::LoopResearcher;
use sorcerer_models::domain::claim::{Checked, Claim};
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;
use sorcerer_models::usecases::check_claim::{CheckClaimDeps, check_claim};

use crate::pass::{PassDeps, Progress, SystemClock};
use crate::review::ResearchReviewer;

/// The product scry's errands are recorded under.
pub const PRODUCT: &str = "scry";

/// What a pass needs, however it was built: the live wiring or a test's doubles.
pub trait Wired {
    fn deps(&self) -> PassDeps<'_>;
    /// The cancel the claim checks watch; the pass is given the same.
    fn cancel(&self) -> Cancel {
        Cancel::default()
    }
}

type CheckFn = dyn Fn(&Claim) -> sorcerer_models::Result<Checked> + Send + Sync;

pub struct Live {
    view: Arc<FilesView>,
    reviewer: ResearchReviewer,
    jev: Jev,
    checker: Box<CheckFn>,
    clock: SystemClock,
    cancel: Cancel,
}

fn quiet(_: &Progress) {}

impl Live {
    /// The live wiring for PROJECT: Jev from `[decide]` (its key is
    /// required), the default provider (`claude` runs Claude Code as the
    /// researcher; any other provider runs sorcerer's tool loop over it),
    /// grammars from the user's cache.
    pub fn open(project: &Project, env: &dyn Env) -> Result<Live> {
        let jev = jev_for(project, env)?;
        let (provider, choice) = default_provider_for(project, env)?;
        let cfg = project
            .config
            .model
            .providers
            .get(&choice.name)
            .cloned()
            .unwrap_or_default();
        let kind = cfg.kind.clone().unwrap_or_else(|| choice.name.clone());
        let view = Arc::new(FilesView::new(
            Arc::new(project.clone()),
            GrammarCache::user(env),
        ));
        let fences = Fences::for_project(project)?;
        let researcher: Arc<dyn Researcher> = if kind == "claude" {
            Arc::new(ClaudeCodeResearcher::new(project, &cfg)?)
        } else {
            Arc::new(LoopResearcher::new(
                provider,
                view.clone() as Arc<dyn CodebaseView>,
                fences.clone(),
                Arc::new(JsonlLedger::for_project(project)?),
            ))
        };
        let cancel = Cancel::default();
        let checker = checker_over(
            researcher.clone(),
            Arc::new(jev.clone()),
            fences,
            view.clone(),
            project.languages.clone(),
            cancel.clone(),
        );
        Ok(Live {
            view,
            reviewer: ResearchReviewer { researcher },
            jev,
            checker,
            clock: SystemClock,
            cancel,
        })
    }

    pub fn deps(&self) -> PassDeps<'_> {
        PassDeps {
            view: &*self.view,
            reviewer: &self.reviewer,
            decider: &self.jev,
            checker: &*self.checker,
            clock: &self.clock,
            progress: &quiet,
            trace: &crate::log::quiet,
        }
    }
}

impl Wired for Live {
    fn deps(&self) -> PassDeps<'_> {
        Live::deps(self)
    }

    fn cancel(&self) -> Cancel {
        self.cancel.clone()
    }
}

/// Jev from `[decide]`, recording into the project's ledger as scry's
/// `decide` errands. No key is an error naming the variable to set.
pub fn jev_for(project: &Project, env: &dyn Env) -> Result<Jev> {
    let jev = Jev::from_env(
        &project.config.decide,
        env,
        Some(Arc::new(JsonlLedger::for_project(project)?)),
    )
    .context("scry does not run without Jev")?;
    Ok(jev.for_product(PRODUCT))
}

/// The claim checker as the pass calls it: `check` with the shipped
/// defaults, its errands recorded as scry's `check`, Jev guarding, a unit
/// named the way the project's LANGUAGES name the claim's files' units.
pub fn checker_over(
    researcher: Arc<dyn Researcher>,
    decider: Arc<dyn Decider>,
    fences: Fences,
    view: Arc<dyn CodebaseView>,
    languages: Languages,
    cancel: Cancel,
) -> Box<CheckFn> {
    Box::new(move |claim: &Claim| {
        let mut deps =
            CheckClaimDeps::defaults(&*researcher, Some(&*decider), &fences, &*view, PRODUCT);
        deps.languages = Some(&languages);
        check_claim(claim, &deps, &cancel)
    })
}
