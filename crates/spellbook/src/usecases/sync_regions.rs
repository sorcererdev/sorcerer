//! Owned regions kept: every region laid on the files it matches, each block worked out again
//! from the files on disk (or in a plan), and the difference planned (`spellbook sync`), found
//! (`check`) or added to a generation's plan (`new`, `fix`). Nothing is written here;
//! `apply_plan` writes a sync like any plan.

use std::collections::BTreeMap;
use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};
use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::anchor::{Anchor, Place, Resolution, resolution_text};
use crate::domain::finding::{
    Finding, REGION_BROKEN, REGION_NOT_SET_UP, REGION_STALE, RegionDrift, fingerprint,
};
use crate::domain::plan::{FileStatus, Plan, PlanStop, PlannedFile, plan_id};
use crate::domain::region::{
    Block, Markers, Region, RegionInstance, adopt, find_block, splice, with_markers_at,
};
use crate::domain::rule::Severity;
use crate::ports::anchors::Anchors;
use crate::ports::grammars::Grammars;
use crate::usecases::plan_generation::PlanDeps;
use crate::usecases::planned_view::PlannedView;

/// What `spellbook sync` does: refresh every region, or also set one up in the files it
/// matches that have no markers for it yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncMode {
    Refresh,
    Init(String),
}

/// A sync planned: the plan (its files are the regions it rewrites, its notes the regions not
/// set up) and how many region blocks the project has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub plan: Plan,
    pub blocks: usize,
}

/// Plans a sync (a query): (1) the regions, which must load; (2) for `Init`, a region by that
/// name; (3) every region laid on the files it matches; (4) each block worked out again from
/// the files its query lists, and under `Init` the markers put in each file of that region
/// that has none (the hand-written lines that match an item moved into the block); (5) each
/// file a block changes planned as a changed file naming its region, and parsed again; (6) the
/// regions not set up said in the notes. A broken marker pair stops the whole plan.
///
/// Example: in bistro-rs with `modules` set up, after `queries/get_day_schedule.rs` was
/// deleted, the plan changes `src/booking/queries/mod.rs` (region modules: +0 −1).
pub fn plan_sync(deps: &PlanDeps, mode: &SyncMode) -> Result<SyncPlan, PlanStop> {
    let regions = deps
        .regions
        .as_ref()
        .map_err(|problem| PlanStop::RegionsDoNotLoad(problem.to_string()))?;
    if let SyncMode::Init(name) = mode
        && !regions.iter().any(|region| region.name == *name)
    {
        return Err(PlanStop::RegionNotThere {
            name: name.clone(),
            known: regions.iter().map(|region| region.name.clone()).collect(),
        });
    }
    let keeper = Keeper {
        view: deps.view,
        grammars: deps.grammars,
        anchors: Some(deps.anchors),
    };
    let kept = keeper.keep_all(regions, mode)?;
    let inputs = match mode {
        SyncMode::Refresh => BTreeMap::new(),
        SyncMode::Init(name) => BTreeMap::from([("init".to_string(), name.clone())]),
    };
    Ok(SyncPlan {
        blocks: kept.blocks,
        plan: Plan {
            id: plan_id("sync", &inputs, &deps.clock.now().to_rfc3339()),
            generator: "sync".into(),
            inputs,
            files: kept.files,
            holes: Vec::new(),
            fix: None,
            notes: kept.notes,
        },
    })
}

/// A generation's plan with every region refreshed over the project as the plan would leave
/// it: a file the plan creates is listed, a file it changes is changed once more on its
/// planned text (still one change from the disk's text), a file it skipped or left becomes a
/// changed file, and a region not set up becomes a note. When `regions.toml` does not load,
/// the plan is kept as it is with a note saying the regions are not being refreshed.
///
/// Example: `new command --set command=cancel_table` in bistro-rs plans `cancel_table.rs` and,
/// here, `src/booking/commands/mod.rs` (region modules: +1 −0).
pub fn refresh_regions(deps: &PlanDeps, plan: Plan) -> Result<Plan, PlanStop> {
    let regions = match deps.regions {
        Ok(regions) => regions,
        Err(problem) => {
            let mut plan = plan;
            plan.notes
                .push(format!("regions are not being refreshed: {problem}"));
            return Ok(plan);
        }
    };
    let planned = plan
        .files
        .iter()
        .filter(|file| matches!(file.status, FileStatus::New | FileStatus::Changed))
        .map(|file| (file.path.clone(), file.text.clone()))
        .collect();
    let view = PlannedView::new(deps.view, planned);
    let keeper = Keeper {
        view: &view,
        grammars: deps.grammars,
        anchors: None,
    };
    let kept = keeper.keep_all(regions, &SyncMode::Refresh)?;
    Ok(with_region_files(plan, kept))
}

/// The plan's files with each file the regions rewrite laid in: a new file keeps its status
/// with its text rewritten; any other is changed from its text on disk.
fn with_region_files(mut plan: Plan, kept: Kept) -> Plan {
    for file in kept.files {
        match plan.files.iter().position(|known| known.path == file.path) {
            Some(at) if plan.files[at].status == FileStatus::New => {
                let mut rewritten = PlannedFile::new_file(file.path, file.text);
                rewritten.region = file.region;
                plan.files[at] = rewritten;
            }
            Some(at) if plan.files[at].status == FileStatus::Changed => {
                let before = plan.files[at].before.clone();
                let mut changed = PlannedFile::changed(file.path, before, file.text);
                changed.region = file.region;
                plan.files[at] = changed;
            }
            Some(at) => plan.files[at] = file,
            None => plan.files.push(file),
        }
    }
    plan.notes.extend(kept.notes);
    plan
}

/// The findings of the regions laid on the given files (every file the project has when
/// `None`): a block that is not what its files make it is `region-stale` on its opening
/// marker's line, a file without markers is `region-not-set-up` on line 1, a broken pair is
/// `region-broken` on its first marker.
///
/// Example: after `queries/get_day_schedule.rs` is deleted, `src/booking/queries/mod.rs:3`
/// has `region-stale`, "region modules is out of date (+0 −1)".
pub fn region_findings(
    view: &dyn CodebaseView,
    grammars: &dyn Grammars,
    regions: &[Region],
    files: Option<&[String]>,
) -> Result<Vec<Finding>, PlanStop> {
    let keeper = Keeper {
        view,
        grammars,
        anchors: None,
    };
    let mut found = Vec::new();
    for instance in instances(view, regions) {
        if files.is_some_and(|files| !files.contains(&instance.file)) {
            continue;
        }
        let Ok(text) = view.text(Path::new(&instance.file)) else {
            continue;
        };
        let state = keeper.state_of(&instance, &text)?;
        found.extend(finding_of(&instance, &text, state));
    }
    Ok(found)
}

fn finding_of(instance: &RegionInstance, before: &str, state: State) -> Option<Finding> {
    let name = &instance.region.name;
    let (rule, line, severity, message, drift) = match state {
        State::Fresh => return None,
        State::Stale { open_row, after } => {
            let changed = PlannedFile::changed(instance.file.clone(), before.to_string(), after);
            let drift = RegionDrift {
                region: name.clone(),
                added: changed.added,
                removed: changed.removed,
            };
            (
                REGION_STALE,
                open_row + 1,
                instance.region.severity,
                out_of_date_text(name, &drift),
                drift,
            )
        }
        State::NotSetUp => (
            REGION_NOT_SET_UP,
            1,
            Severity::Warn,
            format!("region {name} is not set up here (run spellbook sync --init {name})"),
            no_drift(name),
        ),
        State::Broken { row, why } => {
            (REGION_BROKEN, row + 1, Severity::Error, why, no_drift(name))
        }
    };
    Some(Finding {
        rule: rule.into(),
        file: instance.file.clone(),
        line: u32::try_from(line).unwrap_or(u32::MAX),
        severity,
        message,
        fingerprint: fingerprint(rule, &format!("{name} {}", instance.file)),
        node_text: String::new(),
        region: Some(drift),
    })
}

fn no_drift(name: &str) -> RegionDrift {
    RegionDrift {
        region: name.to_string(),
        added: 0,
        removed: 0,
    }
}

/// `region modules is out of date (+0 −1)`.
fn out_of_date_text(name: &str, drift: &RegionDrift) -> String {
    format!(
        "region {name} is out of date (+{} \u{2212}{})",
        drift.added, drift.removed
    )
}

/// The sentence for a region not set up in some of its files.
///
/// Example: `region ports is not set up in src/booking/ports/mod.rs  (run spellbook sync
/// --init ports)`.
pub fn not_set_up_text(name: &str, files: &[String]) -> String {
    format!(
        "region {name} is not set up in {}  (run spellbook sync --init {name})",
        files.join(", ")
    )
}

/// Every region laid on every file it matches, by region then file; spellbook's own folder and
/// git's are never region files.
pub fn instances<'r>(view: &dyn CodebaseView, regions: &'r [Region]) -> Vec<RegionInstance<'r>> {
    let files: Vec<String> = view
        .glob("**")
        .unwrap_or_default()
        .iter()
        .map(|path| forward_slashes(path))
        .filter(|path| !path.starts_with(".sorcerer/") && !path.starts_with(".git/"))
        .collect();
    regions
        .iter()
        .flat_map(|region| files.iter().filter_map(|file| region.instance_in(file)))
        .collect()
}

/// What a region's block is in one file.
enum State {
    /// The block is what its files make it.
    Fresh,
    /// The block differs: its opening marker's row (from 0) and the file's text refreshed.
    Stale { open_row: usize, after: String },
    /// The file has no markers for the region.
    NotSetUp,
    /// The markers are not a pair: the first one's row (from 0), and why.
    Broken { row: usize, why: String },
}

/// What keeping regions reads: the files (as they are, or as a plan leaves them), the grammars
/// (for the comment marker and the parse check), and the anchors when markers are set up.
struct Keeper<'a> {
    view: &'a dyn CodebaseView,
    grammars: &'a dyn Grammars,
    anchors: Option<&'a dyn Anchors>,
}

/// The files the regions rewrite, the notes on regions not set up, and how many blocks there
/// are once the plan is applied.
struct Kept {
    files: Vec<PlannedFile>,
    notes: Vec<String>,
    blocks: usize,
}

impl Keeper<'_> {
    /// Every region in every file it matches, file by file (two regions in one file are kept
    /// one after the other on the same text).
    fn keep_all(&self, regions: &[Region], mode: &SyncMode) -> Result<Kept, PlanStop> {
        let mut by_file: BTreeMap<String, Vec<RegionInstance>> = BTreeMap::new();
        for instance in instances(self.view, regions) {
            by_file
                .entry(instance.file.clone())
                .or_default()
                .push(instance);
        }
        let mut kept = Kept {
            files: Vec::new(),
            notes: Vec::new(),
            blocks: 0,
        };
        let mut not_set_up: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (file, instances) in &by_file {
            let before = self
                .view
                .text(Path::new(file))
                .map_err(|e| region_stop(&instances[0], e.to_string()))?;
            let mut text = before.clone();
            let mut rewritten: Vec<&str> = Vec::new();
            let mut set_up = None;
            for instance in instances {
                let name = instance.region.name.as_str();
                let state = match self.state_of(instance, &text)? {
                    State::NotSetUp if *mode == SyncMode::Init(name.to_string()) => {
                        let (after, items) = self.set_up(instance, &text)?;
                        set_up = Some(items);
                        State::Stale { open_row: 0, after }
                    }
                    state => state,
                };
                match state {
                    State::Fresh => kept.blocks += 1,
                    State::Stale { after, .. } => {
                        kept.blocks += 1;
                        rewritten.push(name);
                        text = after;
                    }
                    State::NotSetUp => not_set_up
                        .entry(name.to_string())
                        .or_default()
                        .push(file.clone()),
                    State::Broken { row, why } => {
                        return Err(PlanStop::RegionBroken {
                            file: file.clone(),
                            line: u32::try_from(row + 1).unwrap_or(u32::MAX),
                            why,
                        });
                    }
                }
            }
            if rewritten.is_empty() {
                continue;
            }
            self.refuse_a_new_parse_error(file, &before, &text, &rewritten.join(", "))?;
            let mut changed = PlannedFile::changed(file.clone(), before, text);
            changed.region = Some(rewritten.join(", "));
            changed.set_up = set_up;
            kept.files.push(changed);
        }
        kept.notes = not_set_up
            .iter()
            .map(|(name, files)| not_set_up_text(name, files))
            .collect();
        Ok(kept)
    }

    /// The region's block in a text: fresh, stale (with the text refreshed), not there, or
    /// broken.
    fn state_of(&self, instance: &RegionInstance, text: &str) -> Result<State, PlanStop> {
        let comment = self.comment_for(instance)?;
        let block = match find_block(text, &instance.region.name, &comment) {
            Ok(Some(block)) => block,
            Ok(None) => return Ok(State::NotSetUp),
            Err(broken) => {
                return Ok(State::Broken {
                    row: broken.row,
                    why: broken.why,
                });
            }
        };
        let (markers, items) = self.block_of(instance, &comment)?;
        let after = splice(text, &block, &markers, &items);
        if after == text {
            return Ok(State::Fresh);
        }
        Ok(State::Stale {
            open_row: block.open_row,
            after,
        })
    }

    /// The markers and the lines the block should hold: the query's glob filled for this
    /// file, the files it matches (a `*` within one folder), less its excludes, each rendered
    /// as an item.
    fn block_of(
        &self,
        instance: &RegionInstance,
        comment: &str,
    ) -> Result<(Markers, Vec<String>), PlanStop> {
        let glob = instance
            .query_glob()
            .map_err(|why| region_stop(instance, format!("query: {why}")))?;
        let excludes = instance
            .query_excludes()
            .map_err(|why| region_stop(instance, format!("query: {why}")))?;
        let listed = matcher(&glob).map_err(|why| region_stop(instance, why))?;
        let left_out = excludes
            .iter()
            .map(|glob| matcher(glob))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|why| region_stop(instance, why))?;
        let matches: Vec<String> = self
            .view
            .glob(&glob)
            .map_err(|e| region_stop(instance, e.to_string()))?
            .iter()
            .map(|path| forward_slashes(path))
            .filter(|path| listed.is_match(path))
            .filter(|path| !left_out.iter().any(|glob| glob.is_match(path)))
            .collect();
        let items = instance
            .items(&matches)
            .map_err(|why| region_stop(instance, format!("item: {why}")))?;
        let markers = Markers {
            opening: instance.opening_marker(comment, &glob),
            closing: instance.closing_marker(comment),
        };
        Ok((markers, items))
    }

    /// The text with the region set up: the markers at the first `init_at` phrase that
    /// resolves, the hand-written lines matching an item moved into the block, the block
    /// filled. Returns the text and how many items the block holds.
    fn set_up(&self, instance: &RegionInstance, text: &str) -> Result<(String, u32), PlanStop> {
        let comment = self.comment_for(instance)?;
        let (markers, items) = self.block_of(instance, &comment)?;
        let (row, indent) = self.init_place(instance, text)?;
        let marked = with_markers_at(text, row, &indent, &markers);
        let adopted = adopt(
            &marked,
            &self.block_in(instance, &marked, &comment)?,
            &items,
        );
        let block = self.block_in(instance, &adopted, &comment)?;
        let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
        Ok((splice(&adopted, &block, &markers, &items), count))
    }

    fn block_in(
        &self,
        instance: &RegionInstance,
        text: &str,
        comment: &str,
    ) -> Result<Block, PlanStop> {
        match find_block(text, &instance.region.name, comment) {
            Ok(Some(block)) => Ok(block),
            Ok(None) => Err(region_stop(instance, "the markers did not go in".into())),
            Err(broken) => Err(region_stop(instance, broken.why)),
        }
    }

    /// Where the markers go: the first `init_at` phrase that puts whole lines somewhere.
    fn init_place(
        &self,
        instance: &RegionInstance,
        text: &str,
    ) -> Result<(usize, String), PlanStop> {
        let Some(anchors) = self.anchors else {
            return Err(region_stop(
                instance,
                "markers are set up by `spellbook sync --init`".into(),
            ));
        };
        let file_name = instance.file.rsplit('/').next().unwrap_or(&instance.file);
        let mut first_miss = None;
        for phrase in &instance.region.init_at {
            let anchor = Anchor::parse(phrase).map_err(|why| region_stop(instance, why))?;
            let resolution = anchors
                .resolve(Path::new(&instance.file), text, &anchor)
                .map_err(|e| region_stop(instance, e.to_string()))?;
            match resolution {
                Resolution::At(Place::Lines { row, indent, .. }) => return Ok((row, indent)),
                miss => {
                    first_miss.get_or_insert_with(|| resolution_text(&anchor, file_name, &miss));
                }
            }
        }
        Err(region_stop(
            instance,
            format!(
                "init_at: no phrase puts the markers anywhere ({})",
                first_miss.unwrap_or_default()
            ),
        ))
    }

    /// The marker lines' comment syntax: the file's language's, or the region's own for a file
    /// no language covers.
    fn comment_for(&self, instance: &RegionInstance) -> Result<String, PlanStop> {
        let path = Path::new(&instance.file);
        match (self.grammars.covers(path), &instance.region.comment) {
            (true, Some(_)) => Err(region_stop(
                instance,
                "`comment` is for a file no language covers; this file's language has its own, so remove `comment`".into(),
            )),
            (true, None) => self.grammars.line_comment(path).ok_or_else(|| {
                region_stop(
                    instance,
                    "the file's language has no `comment` in its language.toml".into(),
                )
            }),
            (false, Some(comment)) => Ok(comment.clone()),
            (false, None) => Err(region_stop(
                instance,
                "no language covers the file; give the region a `comment` (how its comment lines start)".into(),
            )),
        }
    }

    /// A file that parsed before its blocks were rewritten and does not after stops the plan;
    /// one that had errors already is not the region's doing.
    fn refuse_a_new_parse_error(
        &self,
        file: &str,
        before: &str,
        after: &str,
        regions: &str,
    ) -> Result<(), PlanStop> {
        let Ok(Some(loaded)) = self.grammars.for_path(Path::new(file)) else {
            return Ok(());
        };
        let errors_of = |text: &str| {
            parse_errors_of(&loaded, text).map_err(|e| PlanStop::Template(format!("{file}: {e}")))
        };
        if !errors_of(before)?.is_empty() {
            return Ok(());
        }
        match errors_of(after)?.first() {
            None => Ok(()),
            Some(line) => Err(PlanStop::DoesNotParse {
                template: format!("region {regions}"),
                file: file.into(),
                line: *line,
            }),
        }
    }
}

/// A glob over `/`-separated paths in which `*` stays inside one folder.
fn matcher(glob: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(glob)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|e| format!("query: `{glob}` is not a glob ({e})"))
}

fn region_stop(instance: &RegionInstance, why: String) -> PlanStop {
    PlanStop::Region {
        name: instance.region.name.clone(),
        file: instance.file.clone(),
        why,
    }
}
