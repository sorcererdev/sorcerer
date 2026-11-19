//! Working out a pass's scope from the request, through the view.

use std::path::PathBuf;

use globset::{Glob, GlobSet, GlobSetBuilder};
use sorcerer_core::domain::compare::{UnitChange, compare_units};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Unit;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Error, Result};

use crate::domain::config::ScryConfig;
use crate::domain::scope::{Scope, ScopeRequest, ScopedFile, ScopedUnit, header_of};

/// The scope of a pass for REQUEST. With ALL, every unit of each file is in
/// scope, not only the added and changed ones. A file is left out when no
/// language covers it, `[scry] exclude` matches it, it is fenced, or it is
/// not text.
pub fn scope_of(
    project: &Project,
    view: &dyn CodebaseView,
    request: ScopeRequest,
    all: bool,
    cfg: &ScryConfig,
) -> Result<Scope> {
    let fences = Fences::for_project(project)?;
    let exclude = exclude_set(project, &cfg.exclude)?;
    let has_commit = view.merge_base("HEAD").is_ok();
    let base = match &request {
        ScopeRequest::Since(rev) => view.merge_base(rev)?,
        _ if has_commit => "HEAD".to_string(),
        _ => String::new(),
    };
    let relative = |p: &PathBuf| -> Option<PathBuf> {
        if p.is_absolute() {
            fences.relative(p)
        } else {
            Some(p.clone())
        }
    };
    let mut candidates: Vec<PathBuf> = match &request {
        ScopeRequest::Files(ps) => ps.iter().filter_map(relative).collect(),
        _ if base.is_empty() => view.glob("**")?,
        _ => view.changed_since(&base)?,
    };
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|p| seen.insert(p.clone()));

    let mut scope = Scope {
        request,
        base,
        units: Vec::new(),
        files: Vec::new(),
        skipped: Vec::new(),
    };
    for file in candidates {
        let left_out = if project.languages.for_path(&file).is_none() {
            Some("no language")
        } else if exclude.is_match(&file) {
            Some("excluded by [scry] exclude")
        } else if !fences.allows(&file) {
            Some("fenced")
        } else {
            None
        };
        if let Some(why) = left_out {
            scope.skipped.push((file, why.into()));
            continue;
        }
        if !view.exists(&file) {
            if !scope.base.is_empty() && view.text_at(&file, &scope.base)?.is_some() {
                scope.files.push(ScopedFile {
                    diff: view.diff(&file, &scope.base)?,
                    file,
                    units_now: Vec::new(),
                    deleted: true,
                });
            } else {
                scope
                    .skipped
                    .push((file, "deleted, and not at the base".into()));
            }
            continue;
        }
        let text = match view.text(&file) {
            Ok(t) => t,
            Err(Error::NotText { .. }) => {
                scope.skipped.push((file, "not text".into()));
                continue;
            }
            Err(e) => return Err(e),
        };
        let now = view.units(&file)?;
        let picked: Vec<Unit> = if all || scope.base.is_empty() {
            now.clone()
        } else {
            // Units at the base that do not read count as none: all added.
            let before = view.units_at(&file, &scope.base).unwrap_or_default();
            compare_units(&before, &now)
                .into_iter()
                .filter_map(|c| match c {
                    UnitChange::Added(u) | UnitChange::Changed { after: u, .. } => Some(u),
                    _ => None,
                })
                .collect()
        };
        let lines: Vec<&str> = text.lines().collect();
        for unit in picked {
            scope.units.push(ScopedUnit {
                file: file.clone(),
                header: header_of(&lines, &now, &unit),
                unit,
            });
        }
        let diff = if scope.base.is_empty() {
            String::new()
        } else {
            view.diff(&file, &scope.base)?
        };
        scope.files.push(ScopedFile {
            diff: if diff.is_empty() { text } else { diff },
            units_now: now.into_iter().map(|u| u.name).collect(),
            file,
            deleted: false,
        });
    }
    Ok(scope)
}

fn exclude_set(project: &Project, globs: &[String]) -> Result<GlobSet> {
    let bad = |why: String| Error::Config {
        file: project.root.join(".sorcerer/config.toml"),
        why: format!("[scry] exclude: {why}"),
    };
    let mut set = GlobSetBuilder::new();
    for g in globs {
        set.add(Glob::new(g).map_err(|e| bad(e.to_string()))?);
    }
    set.build().map_err(|e| bad(e.to_string()))
}
