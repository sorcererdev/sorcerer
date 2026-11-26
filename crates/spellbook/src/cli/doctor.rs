//! `spellbook doctor`: whether spellbook can run in a project, a row for each thing it reads.
//! It only reads.

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::json_store::{Snapshot, Store};
use sorcerer_core::adapters::trust_list::TrustList;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::needs_trust;
use sorcerer_core::ports::env::Env;

use crate::adapters::cached_grammars::CachedGrammars;
use crate::adapters::generator_dir::{GENERATORS_DIR, generators_in};
use crate::adapters::json_holes::HOLES_FILE;
use crate::adapters::json_report::REPORT_FILE;
use crate::adapters::jsonl_generation_store::JsonlGenerationStore;
use crate::adapters::regions_toml::load_regions;
use crate::adapters::rules_toml::{RULES_FILE, load_rules};
use crate::domain::config::spellbook_config;
use crate::domain::finding::{REGION_BROKEN, REGION_NOT_SET_UP, REGION_STALE};
use crate::domain::holes_table::holes_table;
use crate::domain::plan::plan_stop_text;
use crate::domain::record::HolesFile;
use crate::domain::region::REGIONS_FILE;
use crate::domain::report::ReportFile;
use crate::ports::generation_store::GenerationStore;
use crate::usecases::sync_regions::{instances, region_findings};

use super::format::count;

/// One row of `spellbook doctor`: what was checked, and what was found or why it fails.
#[derive(Debug, Clone, PartialEq)]
pub struct DoctorRow {
    pub name: &'static str,
    pub result: std::result::Result<String, String>,
}

/// Prints the doctor's rows. The exit code is 2 when any row failed, else 0.
pub(super) fn doctor(project: &Project, env: &dyn Env, out: &mut dyn Write) -> Result<i32> {
    let rows = doctor_rows(project, env);
    for row in &rows {
        write!(out, "{}", row_text(row))?;
    }
    Ok(if rows.iter().any(|row| row.result.is_err()) {
        2
    } else {
        0
    })
}

/// The rows, in this order: `config`, `generators`, `rules`, `regions`, `languages`, `conjure`, `trust`,
/// `holes`, `report`, `generations`.
pub fn doctor_rows(project: &Project, env: &dyn Env) -> Vec<DoctorRow> {
    let row = |name, result| DoctorRow { name, result };
    vec![
        row(
            "config",
            spellbook_config(project)
                .map(|_| "ok".to_string())
                .map_err(|e| e.to_string()),
        ),
        row("generators", generators_row(project)),
        row("rules", rules_row(project)),
        row("regions", regions_row(project, env)),
        row("languages", Ok(languages_row(project))),
        row("conjure", Ok(conjure_row(env))),
        row(
            "trust",
            Ok(trust_row(project, TrustList::user(env).as_ref())),
        ),
        row("holes", holes_row(project)),
        row("report", report_row(project)),
        row("generations", generations_row(project)),
    ]
}

/// Each generator loaded, or the first that did not with its problem.
fn generators_row(project: &Project) -> Result<String, String> {
    let loaded = generators_in(project);
    if loaded.is_empty() {
        return Ok(format!(
            "none yet (add one as {GENERATORS_DIR}/<name>/generator.toml)"
        ));
    }
    let problems: Vec<String> = loaded
        .iter()
        .filter_map(|generator| generator.as_ref().err().map(ToString::to_string))
        .collect();
    let names: Vec<&str> = loaded
        .iter()
        .filter_map(|generator| generator.as_ref().ok().map(|g| g.id.as_str()))
        .collect();
    match problems.as_slice() {
        [] => Ok(names.join(", ")),
        _ => Err(problems.join("; ")),
    }
}

/// Each language and the hole kinds its `[holes]` gives.
fn languages_row(project: &Project) -> String {
    let languages = &project.languages;
    let described: Vec<String> = languages
        .names()
        .into_iter()
        .filter_map(|name| languages.get(name))
        .map(|language| match holes_table(language) {
            Ok(table) if table.kinds.is_empty() => format!("{} (no [holes])", language.name),
            Ok(table) => format!(
                "{} (holes: {})",
                language.name,
                table.kinds.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
            Err(e) => format!("{} ({e})", language.name),
        })
        .collect();
    described.join("; ")
}

fn conjure_row(env: &dyn Env) -> String {
    match env.which("conjure") {
        Some(path) => format!("{}: holes can be conjured in the editor", path.display()),
        None => "not on PATH: holes are filled by hand; install conjure to fill them with a model"
            .into(),
    }
}

fn trust_row(project: &Project, list: Option<&TrustList>) -> String {
    if needs_trust(project).is_empty() {
        return "not needed".into();
    }
    match list.is_some_and(|list| list.is_trusted(&project.root)) {
        true => "trusted".into(),
        false => "needed: the editor's server waits for \"spellbook trust\"".into(),
    }
}

fn holes_row(project: &Project) -> Result<String, String> {
    let store: Store<HolesFile> = Store::at(project.state.join(HOLES_FILE));
    match store.read().map_err(|e| e.to_string())? {
        Snapshot::Absent => Ok("no holes.json yet".into()),
        Snapshot::Current(file) => Ok(format!(
            "holes.json version 1, {} at the last look",
            count(file.holes.len(), "open hole", "open holes")
        )),
        Snapshot::Unreadable(why) => Err(format!("holes.json cannot be read: {why}")),
    }
}

/// The rules file read, with how many rules it holds, or its problem.
fn rules_row(project: &Project) -> Result<String, String> {
    let rules = load_rules(project).map_err(|problem| problem.to_string())?;
    if rules.is_empty() {
        return Ok(format!(
            "none yet (add {RULES_FILE}; `spellbook check` has nothing to check)"
        ));
    }
    let ids: Vec<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
    Ok(format!(
        "{}: {}",
        count(rules.len(), "rule", "rules"),
        ids.join(", ")
    ))
}

/// The regions declared, how many files they keep, and which are not set up or out of date.
/// Only reads: no grammar is loaded (the comment markers are the languages' data).
///
/// Example: `2 declared, 5 files, 1 not set up (ports in src/booking/ports/mod.rs)`.
fn regions_row(project: &Project, env: &dyn Env) -> Result<String, String> {
    let regions = load_regions(project).map_err(|problem| problem.to_string())?;
    if regions.is_empty() {
        return Ok(format!(
            "none declared (add {REGIONS_FILE} to keep a list such as a folder's module lines)"
        ));
    }
    let project = Arc::new(project.clone());
    let cache = GrammarCache::user(env);
    let view = FilesView::new(project.clone(), cache.clone());
    let grammars = CachedGrammars {
        languages: project.languages.clone(),
        cache,
    };
    let files = instances(&view, &regions).len();
    let found =
        region_findings(&view, &grammars, &regions, None).map_err(|stop| plan_stop_text(&stop))?;
    let mut text = format!(
        "{} declared, {}",
        regions.len(),
        count(files, "file", "files")
    );
    for (rule, words) in [
        (REGION_NOT_SET_UP, "not set up"),
        (REGION_STALE, "out of date"),
        (REGION_BROKEN, "with broken markers"),
    ] {
        let places: Vec<String> = found
            .iter()
            .filter(|finding| finding.rule == rule)
            .map(|finding| {
                let region = finding
                    .region
                    .as_ref()
                    .map_or("", |drift| drift.region.as_str());
                format!("{region} in {}", finding.file)
            })
            .collect();
        if !places.is_empty() {
            text.push_str(&format!(
                ", {} {words} ({})",
                places.len(),
                places.join(", ")
            ));
        }
    }
    Ok(text)
}

/// The last check: how many findings it left and when.
fn report_row(project: &Project) -> Result<String, String> {
    let store: Store<ReportFile> = Store::at(project.state.join(REPORT_FILE));
    match store.read().map_err(|e| e.to_string())? {
        Snapshot::Absent => Ok("no report.json yet (run `spellbook check`)".into()),
        Snapshot::Current(file) => Ok(format!(
            "report.json version 1, {} at the last check{}",
            count(file.findings.len(), "finding", "findings"),
            file.checked
                .map(|at| format!(", {}", at.format("%Y-%m-%d %H:%M UTC")))
                .unwrap_or_default()
        )),
        Snapshot::Unreadable(why) => Err(format!("report.json cannot be read: {why}")),
    }
}

fn generations_row(project: &Project) -> Result<String, String> {
    let store = JsonlGenerationStore {
        path: project.state.join("spellbook/generations.jsonl"),
    };
    let records = store.all().map_err(|e| e.to_string())?;
    Ok(match records.last() {
        None => "none yet".into(),
        Some(last) => format!(
            "{}, the last {} at {}",
            count(records.len(), "generation", "generations"),
            last.generator,
            last.at.format("%Y-%m-%d %H:%M UTC")
        ),
    })
}

fn row_text(row: &DoctorRow) -> String {
    match &row.result {
        Ok(found) => format!("{:<12} ok    {found}\n", row.name),
        Err(why) => format!("{:<12} FAIL  {why}\n", row.name),
    }
}
