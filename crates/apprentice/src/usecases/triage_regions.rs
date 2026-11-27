//! Triage on idle: the person's changes to a file since the last look, cut into regions, each
//! new region classified (by a free rule, else by Jev in one batch), and kept.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::Cancel;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_models::ports::decider::Decider;

use crate::domain::budget::{spent_today, triage_allows, triage_spent_today};
use crate::domain::config::ApprenticeConfig;
use crate::domain::cutting::{persons_changed_lines, regions_of};
use crate::domain::region::Region;
use crate::domain::region_record::RegionRecord;
use crate::domain::triage::{free_kind, questions_for, verdict_of};
use crate::ports::clock::Clock;
use crate::ports::region_store::RegionStore;
use crate::ports::syntax::Syntax;

/// What triage runs with.
pub struct TriageDeps {
    pub syntax: Arc<dyn Syntax>,
    pub regions: Arc<dyn RegionStore>,
    /// Jev; none when its key is not set, and then only the free rules classify.
    pub decider: Option<Arc<dyn Decider>>,
    pub ledger: Arc<dyn Ledger>,
    pub clock: Arc<dyn Clock>,
    pub config: ApprenticeConfig,
}

/// A file's text at the last look and now, and the lines tools landed in it a moment ago.
pub struct Look<'a> {
    /// Relative to the root, with `/`.
    pub file: &'a str,
    pub before: &'a str,
    pub after: &'a str,
    /// Trimmed lines Sorcerer's servers landed in the file lately, in order: not the person's.
    pub landed_by_tools: &'a [String],
}

/// Classifies the regions the change makes and keeps them; gives back the records of the
/// regions now in the file that apprentice acts on, old and new.
///
/// 1. Cut: the lines the person changed (not a tool's, not reflowed or moved:
///    `persons_changed_lines`) over the file's shape (`regions_of`).
/// 2. A region already kept (same file, same text) is not asked again.
/// 3. A free rule classifies what it can (a stub is an instruction).
/// 4. The rest goes to Jev as one batch, when the triage cap and the day's cap allow; when they
///    do not, or there is no Jev, the rest stays unclassified (and is asked at its next change).
///
/// Example: a new sentence in a docstring is a hunk region; Jev says claim 0.91, addressed 0.82,
/// and it is kept as a claim to check on save.
pub fn triage_change(look: &Look, deps: &TriageDeps, cancel: &Cancel) -> Vec<RegionRecord> {
    let shape = deps.syntax.shape(Path::new(look.file), look.after);
    let changed = persons_changed_lines(look.before, look.after, look.landed_by_tools);
    let regions = regions_of(look.file, look.after, &changed, &shape);
    let unseen: Vec<Region> = regions
        .into_iter()
        .filter(|region| deps.regions.get(&region.id).is_none())
        .collect();
    let (by_rule, for_jev): (Vec<Region>, Vec<Region>) = unseen
        .into_iter()
        .partition(|region| free_kind(region).is_some());
    let now = deps.clock.now();
    let record = |region: Region, triaged| RegionRecord {
        region,
        triaged,
        reply: None,
        dismissed: false,
        at: now,
    };
    let mut records: Vec<RegionRecord> = by_rule
        .into_iter()
        .map(|region| {
            let triaged = free_kind(&region).expect("partitioned by the rule");
            record(region, triaged)
        })
        .collect();
    records.extend(
        asked_of_jev(&for_jev, deps, cancel)
            .into_iter()
            .map(|(region, triaged)| record(region, triaged)),
    );
    let _ = deps.regions.put(&records);
    acted_on_in(look.file, look.after, deps)
}

/// The regions Jev classified, each with its verdict; none when the caps or Jev say no.
fn asked_of_jev(
    regions: &[Region],
    deps: &TriageDeps,
    cancel: &Cancel,
) -> Vec<(Region, crate::domain::triage::Triaged)> {
    let Some(decider) = &deps.decider else {
        return Vec::new();
    };
    if regions.is_empty() || !triage_affordable(deps) {
        return Vec::new();
    }
    let questions: Vec<_> = regions.iter().flat_map(questions_for).collect();
    let Ok(answers) = decider.ask(&questions, cancel) else {
        return Vec::new();
    };
    regions
        .iter()
        .zip(answers.chunks(4))
        .map(|(region, four)| (region.clone(), verdict_of(four)))
        .collect()
}

fn triage_affordable(deps: &TriageDeps) -> bool {
    let errands = deps
        .ledger
        .read()
        .map(|rows| rows.errands)
        .unwrap_or_default();
    let (now, offset) = (deps.clock.now(), deps.clock.offset());
    triage_allows(
        triage_spent_today(&errands, now, offset),
        spent_today(&errands, now, offset),
        &deps.config,
    )
}

/// The kept records of the file that apprentice acts on and whose text is still in it.
pub fn acted_on_in(file: &str, text: &str, deps: &TriageDeps) -> Vec<RegionRecord> {
    deps.regions
        .of_file(file)
        .into_iter()
        .filter(|record| record.acting_kind().is_some() && record.region.found_in(text).is_some())
        .collect()
}
