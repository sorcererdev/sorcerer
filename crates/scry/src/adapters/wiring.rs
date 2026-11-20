//! The live wiring: everything a review needs, built for real. The
//! project's files, the reviewer on the chosen model provider, Jev, the
//! claim checker, the findings store, the ledger, the settings and the
//! system clock. Each model call is recorded in the ledger as one of
//! scry's errands: `review`, `check` or `decide`.

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
use sorcerer_models::domain::claim::Claim;
use sorcerer_models::ports::decider::Decider;
use sorcerer_models::ports::researcher::Researcher;
use sorcerer_models::usecases::check_claim::{CheckClaimDeps, check_claim};

use crate::adapters::json_finding_store::JsonFindingStore;
use crate::adapters::research_reviewer::ResearchReviewer;
use crate::adapters::scry_toml::ScryToml;
use crate::adapters::system_clock::SystemClock;
use crate::ports::claim_checker::ClaimChecker;
use crate::ports::run_log::Quiet;
use crate::usecases::review_change::{Progress, ReviewDeps};

/// The product scry's errands are recorded under.
pub const PRODUCT: &str = "scry";

/// What a review needs, however it was built: the live wiring or a test's
/// doubles.
pub trait Wiring {
    fn deps(&self) -> ReviewDeps<'_>;
}

/// What a review needs, built from the real project, models and clock.
pub struct LiveWiring {
    view: Arc<FilesView>,
    reviewer: ResearchReviewer,
    jev: Jev,
    checker: Box<dyn ClaimChecker>,
    store: JsonFindingStore,
    ledger: JsonlLedger,
    settings: ScryToml,
    clock: SystemClock,
    review_cancel: Cancel,
    cancel: Cancel,
}

/// The live wiring reports no progress; the editor's server sets its own.
fn quiet(_: &Progress) {}

impl LiveWiring {
    /// The live wiring for the project. Jev comes from `[decide]`, and its
    /// key must be set. The model provider is the project's default:
    /// `claude` runs Claude Code; any other provider runs sorcerer's own
    /// tool loop. Grammars come from the user's cache. One `Cancel` stops
    /// everything, as the command line wants.
    pub fn open(project: &Project, env: &dyn Env) -> Result<LiveWiring> {
        let cancel = Cancel::default();
        LiveWiring::open_with(project, env, cancel.clone(), cancel)
    }

    /// The live wiring with two cancels: `review` stops the review calls
    /// only (an editor's save that overtakes them), `rest` stops Jev and the
    /// claim checks too (the editor shutting down).
    pub fn open_with(
        project: &Project,
        env: &dyn Env,
        review: Cancel,
        rest: Cancel,
    ) -> Result<LiveWiring> {
        let jev = jev_for(project, env)?;
        let view = Arc::new(FilesView::new(
            Arc::new(project.clone()),
            GrammarCache::user(env),
        ));
        let fences = Fences::for_project(project)?;
        let researcher = researcher_for(project, env, view.clone())?;
        let checker = claim_checker(
            researcher.clone(),
            Arc::new(jev.clone()),
            fences,
            view.clone(),
            project.languages.clone(),
            rest.clone(),
        );
        Ok(LiveWiring {
            view,
            reviewer: ResearchReviewer { researcher },
            jev,
            checker,
            store: JsonFindingStore::for_project(project)?,
            ledger: JsonlLedger::for_project(project)?,
            settings: ScryToml::for_project(project),
            clock: SystemClock,
            review_cancel: review,
            cancel: rest,
        })
    }
}

impl Wiring for LiveWiring {
    fn deps(&self) -> ReviewDeps<'_> {
        ReviewDeps {
            view: &*self.view,
            reviewer: &self.reviewer,
            decider: &self.jev,
            checker: &*self.checker,
            store: &self.store,
            ledger: &self.ledger,
            settings: &self.settings,
            clock: &self.clock,
            log: &Quiet,
            progress: &quiet,
            cancel: &self.cancel,
            review_cancel: &self.review_cancel,
        }
    }
}

/// The researcher on the project's default model provider. A provider of
/// kind `claude` runs Claude Code; any other runs sorcerer's tool loop over
/// the provider, reading the project's files through `view`, within its
/// fences, and recording each call in its ledger.
pub fn researcher_for(
    project: &Project,
    env: &dyn Env,
    view: Arc<dyn CodebaseView>,
) -> Result<Arc<dyn Researcher>> {
    let (provider, choice) = default_provider_for(project, env)?;
    let settings = project
        .config
        .model
        .providers
        .get(&choice.name)
        .cloned()
        .unwrap_or_default();
    // a provider is of the kind its name says unless `kind` says otherwise
    let kind = settings.kind.clone().unwrap_or_else(|| choice.name.clone());
    Ok(if kind == "claude" {
        Arc::new(ClaudeCodeResearcher::new(project, &settings)?)
    } else {
        Arc::new(LoopResearcher::new(
            provider,
            view,
            Fences::for_project(project)?,
            Arc::new(JsonlLedger::for_project(project)?),
        ))
    })
}

/// Jev, set up from `[decide]`, recording each question in the project's
/// ledger as one of scry's `decide` errands. With no key set, the error
/// names the variable to set.
pub fn jev_for(project: &Project, env: &dyn Env) -> Result<Jev> {
    let jev = Jev::from_env(
        &project.config.decide,
        env,
        Some(Arc::new(JsonlLedger::for_project(project)?)),
    )
    .context("scry does not run without Jev")?;
    Ok(jev.for_product(PRODUCT))
}

/// The claim checker a review calls: sorcerer's `check_claim` with its
/// shipped defaults, each call recorded as one of scry's `check` errands,
/// Jev asked whether a cheap model's verdict holds up (if not, a strong
/// model checks again), and units named the way the project's languages
/// name them.
pub fn claim_checker(
    researcher: Arc<dyn Researcher>,
    decider: Arc<dyn Decider>,
    fences: Fences,
    view: Arc<dyn CodebaseView>,
    languages: Languages,
    cancel: Cancel,
) -> Box<dyn ClaimChecker> {
    Box::new(move |claim: &Claim| {
        let deps = CheckClaimDeps {
            languages: Some(&languages),
            ..CheckClaimDeps::defaults(&*researcher, Some(&*decider), &fences, &*view, PRODUCT)
        };
        check_claim(claim, &deps, &cancel)
    })
}
