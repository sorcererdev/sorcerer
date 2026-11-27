//! Gathering what the writer reads: the test file, the related files, the uncommitted work,
//! scry's findings and the project's rules.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use globset::{Glob, GlobSet, GlobSetBuilder};
use sorcerer_core::domain::language::Language;
use sorcerer_core::domain::unit::{Span, UnitKind};

use super::{ChangeDeps, ChangeRefused, ChangeRequest, Site};
use crate::domain::context_choice::{CONTEXT_BUDGET_TOKENS, Chosen, chosen_within, closest_first};
use crate::domain::language_tables::tests_path;
use crate::domain::scry_notes::{FindingsRead, ScryNote, StoredFinding, note_of, notes_near};
use crate::domain::situation::{RelatedFile, Situation, session_text, situation_text};
use crate::domain::spellbook_findings::rules_for_the_file_text;

/// The situation text for a site: everything the writer is shown.
pub(super) fn situation_for(
    site: &Site,
    request: &ChangeRequest,
    deps: &ChangeDeps,
) -> Result<String, ChangeRefused> {
    let failed = |e: sorcerer_core::Error| ChangeRefused::Failed(e.to_string());
    let language = &site.language;
    let mut left_out: Vec<PathBuf> = Vec::new();
    let test_file = tests_path(language, &site.file)
        .map_err(failed)?
        .filter(|path| allowed_or_left_out(deps, path, &mut left_out));
    let test_text = test_file
        .as_deref()
        .and_then(|path| text_if_there(deps, path));
    let related = deps
        .context
        .related_files(&site.file, &site.text)
        .map_err(failed)?;
    left_out.extend(related.left_out);
    let chosen = related_with_room(site, related.files);
    let findings = scry_findings(deps);
    let notes = notes_near(
        &findings,
        &site.unit,
        &site.file,
        test_file.as_deref(),
        |path, lines| fingerprint_now(deps, path, lines),
    );
    let finding_asked = finding_asked(request, &findings, deps)?;
    let situation = situation_text(&Situation {
        language: language.name.clone(),
        unit_words: language.unit_words.clone().unwrap_or_default(),
        file: site.file.clone(),
        unit: site.unit.name.clone(),
        broken_region: site.unit.kind == UnitKind::BrokenRegion,
        line: request.line,
        span: site.span,
        module: site.text.clone(),
        test_file,
        test_text,
        related: chosen.kept,
        left_for_room: chosen
            .left_for_room
            .into_iter()
            .map(|file| file.path)
            .collect(),
        session: session_of(deps, language, &mut left_out),
        finding_asked,
        notes,
        rules: deps.context.rules(),
        left_out: without_repeats(left_out),
    });
    let kept = rules_for_the_file_text(&deps.checker.rules_for(&site.file));
    Ok(format!("{situation}{kept}"))
}

/// The related files the step has room for, whole: the imports the unit uses first, then the
/// other imports, then the files near them, each kept while it fits the step's 6,000 tokens and
/// left out whole (to be named, not shown) when it does not.
fn related_with_room(site: &Site, files: Vec<RelatedFile>) -> Chosen {
    chosen_within(
        closest_first(files, &unit_text(site)),
        CONTEXT_BUDGET_TOKENS,
    )
}

/// The lines of the unit under the cursor, as the file's text has them.
fn unit_text(site: &Site) -> String {
    let span = site.unit.span;
    site.text
        .lines()
        .skip(span.start.saturating_sub(1) as usize)
        .take((span.end + 1).saturating_sub(span.start) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a model may be shown the file; when not, it is named in `left_out`.
fn allowed_or_left_out(deps: &ChangeDeps, path: &Path, left_out: &mut Vec<PathBuf>) -> bool {
    let allowed = deps.fences.allows(path);
    if !allowed {
        left_out.push(path.to_path_buf());
    }
    allowed
}

fn without_repeats(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !seen.contains(&path) {
            seen.push(path);
        }
    }
    seen
}

fn text_if_there(deps: &ChangeDeps, path: &Path) -> Option<String> {
    deps.view.exists(path).then(|| deps.view.text(path).ok())?
}

/// scry's findings; none when scry has stored nothing conjure can read.
fn scry_findings(deps: &ChangeDeps) -> Vec<StoredFinding> {
    match deps.context.scry_findings() {
        FindingsRead::Found(findings) => findings,
        FindingsRead::NoFile | FindingsRead::OtherVersion(_) => Vec::new(),
    }
}

/// The finding the person asked to conjure for; asking for one scry does not have is an error.
fn finding_asked(
    request: &ChangeRequest,
    findings: &[StoredFinding],
    deps: &ChangeDeps,
) -> Result<Option<ScryNote>, ChangeRefused> {
    let Some(id) = &request.finding else {
        return Ok(None);
    };
    let Some(finding) = findings.iter().find(|finding| finding.id == *id) else {
        return Err(ChangeRefused::Failed(format!(
            "no finding `{id}` in scry's findings"
        )));
    };
    let stale = is_stale(finding, deps);
    Ok(Some(note_of(finding, stale)))
}

fn is_stale(finding: &StoredFinding, deps: &ChangeDeps) -> bool {
    let now = fingerprint_now(deps, &finding.place.file, finding.place.lines);
    matches!((&finding.place.fingerprint, now), (Some(stored), Some(now)) if *stored != now)
}

/// The fingerprint of the unit that holds these lines of a file now.
fn fingerprint_now(deps: &ChangeDeps, file: &Path, lines: Span) -> Option<String> {
    let unit = deps
        .view
        .unit_or_broken_region_at(file, lines.start)
        .ok()??;
    Some(unit.fingerprint.as_str().to_string())
}

/// The uncommitted work: each changed file's diff, the one touched last first, and the new
/// files. Outside a git repository there is none.
fn session_of(deps: &ChangeDeps, language: &Language, left_out: &mut Vec<PathBuf>) -> String {
    let Ok(changed) = deps.view.changed_since("HEAD") else {
        return String::new();
    };
    let globs = globs_of(language);
    let mut diffs: Vec<(PathBuf, String)> = Vec::new();
    let mut new_files: Vec<PathBuf> = Vec::new();
    for path in changed {
        if globs.is_match(&path) && !allowed_or_left_out(deps, &path, left_out) {
            continue;
        }
        let diff = deps.view.diff(&path, "HEAD").unwrap_or_default();
        if !diff.is_empty() {
            diffs.push((path, diff));
        } else if is_new_to_git(deps, &path) {
            new_files.push(path);
        }
    }
    diffs.sort_by_key(|(path, _)| Reverse(touched_at(deps, path)));
    session_text(&diffs, &new_files, &globs)
}

fn is_new_to_git(deps: &ChangeDeps, path: &Path) -> bool {
    deps.view.exists(path)
        && deps
            .view
            .text_at(path, "HEAD")
            .is_ok_and(|at_head| at_head.is_none())
}

fn touched_at(deps: &ChangeDeps, path: &Path) -> Option<SystemTime> {
    std::fs::metadata(deps.project.root.join(path))
        .and_then(|meta| meta.modified())
        .ok()
}

fn globs_of(language: &Language) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in &language.files {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}
