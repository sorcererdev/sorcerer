//! The live wiring of a pass: the project's view, the find call through a
//! researcher on the chosen provider, Jev, and the claim checker, each
//! recording its errands as scry's (`find`, `check`, `decide`).

use std::sync::Arc;

use anyhow::{Context, Result};
use sorcerer_core::env::Env;
use sorcerer_core::fences::Fences;
use sorcerer_core::grammar::GrammarCache;
use sorcerer_core::ledger::Ledger;
use sorcerer_core::view::{CodebaseView, FilesView};
use sorcerer_core::{Cancel, Project};
use sorcerer_models::checker::{Checked, CheckerDeps, Claim, check};
use sorcerer_models::decide::Decider;
use sorcerer_models::decide::jev::Jev;
use sorcerer_models::providers::default_provider_for;
use sorcerer_models::research::Researcher;
use sorcerer_models::research::claude_code::ClaudeCodeResearcher;
use sorcerer_models::research::tool_loop::LoopResearcher;

use crate::find::{FindAsk, Finder, Found, ResearchFinder};
use crate::pass::{PassDeps, Progress, SystemClock};

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
    finder: LiveFinder,
    jev: Jev,
    checker: Box<CheckFn>,
    clock: SystemClock,
    cancel: Cancel,
}

/// The find call over a shared researcher.
struct LiveFinder {
    researcher: Arc<dyn Researcher>,
}

impl Finder for LiveFinder {
    fn find(
        &self,
        ask: &FindAsk,
        model: &str,
        cap_usd: f64,
        cancel: &Cancel,
    ) -> sorcerer_models::Result<Found> {
        ResearchFinder {
            researcher: &*self.researcher,
        }
        .find(ask, model, cap_usd, cancel)
    }
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
                Ledger::for_project(project)?,
            ))
        };
        let cancel = Cancel::default();
        let checker = checker_over(
            researcher.clone(),
            Arc::new(jev.clone()),
            fences,
            view.clone(),
            cancel.clone(),
        );
        Ok(Live {
            view,
            finder: LiveFinder { researcher },
            jev,
            checker,
            clock: SystemClock,
            cancel,
        })
    }

    pub fn deps(&self) -> PassDeps<'_> {
        PassDeps {
            view: &*self.view,
            finder: &self.finder,
            decider: &self.jev,
            checker: &*self.checker,
            clock: &self.clock,
            progress: &quiet,
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
        Some(Ledger::for_project(project)?),
    )
    .context("scry does not run without Jev")?;
    Ok(jev.for_product(PRODUCT))
}

/// The claim checker as the pass calls it: `check` with the shipped
/// defaults, its errands recorded as scry's `check`, Jev guarding.
pub fn checker_over(
    researcher: Arc<dyn Researcher>,
    decider: Arc<dyn Decider>,
    fences: Fences,
    view: Arc<dyn CodebaseView>,
    cancel: Cancel,
) -> Box<CheckFn> {
    Box::new(move |claim: &Claim| {
        let deps = CheckerDeps::defaults(&*researcher, Some(&*decider), &fences, &*view, PRODUCT);
        check(claim, &deps, &cancel)
    })
}
