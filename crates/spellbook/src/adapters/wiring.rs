//! The wiring: the real adapters for a project, put together once for a command.

use std::sync::Arc;

use sorcerer_core::Result;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::adapters::cached_grammars::CachedGrammars;
use crate::adapters::disk_workspace::DiskWorkspace;
use crate::adapters::generator_dir::GeneratorDirs;
use crate::adapters::json_holes::JsonHoles;
use crate::adapters::json_report::JsonReport;
use crate::adapters::jsonl_generation_store::JsonlGenerationStore;
use crate::adapters::query_anchors::QueryAnchors;
use crate::adapters::system_clock::SystemClock;
use crate::ports::clock::Clock;
use crate::usecases::apply_plan::ApplyDeps;
use crate::usecases::open_holes::HolesDeps;
use crate::usecases::plan_generation::PlanDeps;

/// Every adapter a command may use, for one project.
pub struct Wired {
    pub project: Arc<Project>,
    pub view: FilesView,
    pub generators: GeneratorDirs,
    pub grammars: CachedGrammars,
    /// Where an `insert` edit's phrase points, over the same grammars.
    pub anchors: QueryAnchors<CachedGrammars>,
    pub fences: Fences,
    pub workspace: DiskWorkspace,
    pub generations: JsonlGenerationStore,
    pub holes: JsonHoles,
    pub report: JsonReport,
    pub clock: Arc<dyn Clock>,
}

/// Builds the wiring for a project; the tests pass their own (vendored grammars, a fixed clock).
pub type Wire<'a> = &'a dyn Fn(Project) -> Result<Wired>;

/// The real wiring: the user's grammar cache and the system clock.
pub fn live(project: Project, env: &dyn Env) -> Result<Wired> {
    wired(project, GrammarCache::user(env), Arc::new(SystemClock))
}

/// The wiring over a grammar cache and a clock.
pub fn wired(project: Project, cache: GrammarCache, clock: Arc<dyn Clock>) -> Result<Wired> {
    let project = Arc::new(project);
    let fences = Fences::for_project(&project)?;
    Ok(Wired {
        view: FilesView::new(project.clone(), cache.clone()),
        generators: GeneratorDirs::new(project.clone()),
        anchors: QueryAnchors::new(CachedGrammars {
            languages: project.languages.clone(),
            cache: cache.clone(),
        }),
        grammars: CachedGrammars {
            languages: project.languages.clone(),
            cache,
        },
        workspace: DiskWorkspace {
            root: project.root.clone(),
            fences: fences.clone(),
        },
        generations: JsonlGenerationStore::for_project(&project)?,
        holes: JsonHoles::for_project(&project)?,
        report: JsonReport::for_project(&project)?,
        fences,
        clock,
        project,
    })
}

impl Wired {
    pub fn plan_deps(&self) -> PlanDeps<'_> {
        PlanDeps {
            view: &self.view,
            grammars: &self.grammars,
            anchors: &self.anchors,
            fences: &self.fences,
            clock: self.clock.as_ref(),
        }
    }

    pub fn apply_deps(&self) -> ApplyDeps<'_> {
        ApplyDeps {
            workspace: &self.workspace,
            generations: &self.generations,
            holes: &self.holes,
            clock: self.clock.as_ref(),
        }
    }

    pub fn holes_deps(&self) -> HolesDeps<'_> {
        HolesDeps {
            view: &self.view,
            holes: &self.holes,
        }
    }
}
