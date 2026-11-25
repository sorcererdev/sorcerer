//! Everything a plan reads, for one project on the vendored grammars.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use spellbook::adapters::cached_grammars::CachedGrammars;
use spellbook::adapters::disk_workspace::DiskWorkspace;
use spellbook::adapters::generator_dir::generator_named;
use spellbook::adapters::json_holes::JsonHoles;
use spellbook::adapters::jsonl_generation_store::JsonlGenerationStore;
use spellbook::adapters::query_anchors::QueryAnchors;
use spellbook::domain::generator::Generator;
use spellbook::domain::inputs::Given;
use spellbook::domain::plan::{Plan, PlanStop};
use spellbook::usecases::apply_plan::{AppliedGeneration, ApplyDeps, ChangesLanded, apply_plan};
use spellbook::usecases::plan_generation::{PlanDeps, plan_generation};

use super::{FixedClock, project_with_vendored_grammars, test_cache};

pub struct PlanRig {
    pub project: Arc<Project>,
    pub view: FilesView,
    pub grammars: CachedGrammars,
    pub anchors: QueryAnchors<CachedGrammars>,
    pub fences: Fences,
    pub clock: FixedClock,
}

impl PlanRig {
    pub fn new(dir: &Path) -> PlanRig {
        let (project, view) = project_with_vendored_grammars(dir);
        PlanRig {
            grammars: CachedGrammars {
                languages: project.languages.clone(),
                cache: test_cache(),
            },
            anchors: QueryAnchors::new(CachedGrammars {
                languages: project.languages.clone(),
                cache: test_cache(),
            }),
            fences: Fences::for_project(&project).unwrap(),
            clock: FixedClock::at_noon(),
            project,
            view,
        }
    }

    pub fn deps(&self) -> PlanDeps<'_> {
        PlanDeps {
            view: &self.view,
            grammars: &self.grammars,
            anchors: &self.anchors,
            fences: &self.fences,
            clock: &self.clock,
        }
    }

    pub fn generator(&self, id: &str) -> Generator {
        generator_named(&self.project, id).unwrap().unwrap()
    }

    /// Plans a generator with `name=value` inputs.
    pub fn plan(&self, id: &str, inputs: &[(&str, &str)]) -> Result<Plan, PlanStop> {
        let given = Given(
            inputs
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        );
        plan_generation(&self.deps(), &self.generator(id), &given)
    }
}

/// Everything applying writes to, for the same project.
pub struct ApplyRig {
    pub workspace: DiskWorkspace,
    pub generations: JsonlGenerationStore,
    pub holes: JsonHoles,
}

impl PlanRig {
    pub fn apply_rig(&self) -> ApplyRig {
        ApplyRig {
            workspace: DiskWorkspace {
                root: self.project.root.clone(),
                fences: self.fences.clone(),
            },
            generations: JsonlGenerationStore::for_project(&self.project).unwrap(),
            holes: JsonHoles::for_project(&self.project).unwrap(),
        }
    }

    /// Applies a plan.
    pub fn apply(&self, plan: Plan) -> AppliedGeneration {
        let rig = self.apply_rig();
        let deps = ApplyDeps {
            workspace: &rig.workspace,
            generations: &rig.generations,
            holes: &rig.holes,
            clock: &self.clock,
        };
        apply_plan(&deps, plan, ChangesLanded::NotYet).unwrap()
    }
}
