//! The scope of a pass: which files it looks at, and which of their units a
//! change adds or alters. A pass is started for a saved file, for files the
//! user names, for the working tree, or for a branch since it left a rev.

use std::path::PathBuf;

use globset::{Glob, GlobSet, GlobSetBuilder};
use sorcerer_core::domain::compare::{UnitChange, compare_units};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Unit;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Error, Result};

use crate::config::ScryConfig;

/// Why a pass runs, which says which files it looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeWhy {
    /// The editor saved this file.
    Saved(PathBuf),
    /// The user named these files.
    Named(Vec<PathBuf>),
    /// The files a branch changed since it left REV (against their merge base).
    Since(String),
    /// The files that differ from HEAD, untracked ones too.
    Working,
}

/// A unit in scope, with the first line of the unit that encloses it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopedUnit {
    pub file: PathBuf,
    pub unit: Unit,
    pub header: Option<String>,
}

/// A file in scope, whether or not any of its units are.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopedFile {
    pub file: PathBuf,
    /// The file's diff against the base; its whole text when that is empty
    /// (untracked, unchanged, or no commit yet).
    pub diff: String,
    /// Every unit name in the file now, for the gone sweep.
    pub units_now: Vec<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub why: ScopeWhy,
    /// The rev compared against: `HEAD`, the merge base's commit id for
    /// `Since`, or empty when the repository has no commit yet.
    pub base: String,
    pub units: Vec<ScopedUnit>,
    pub files: Vec<ScopedFile>,
    /// The files considered and left out, each with why (for the run log).
    pub skipped: Vec<(PathBuf, String)>,
}

/// The scope of a pass for WHY. With ALL, every unit of each file is in
/// scope, not only the added and changed ones. A file is left out when no
/// language covers it, `[scry] exclude` matches it, it is fenced, or it is
/// not text.
pub fn scope_of(
    project: &Project,
    view: &dyn CodebaseView,
    why: ScopeWhy,
    all: bool,
    cfg: &ScryConfig,
) -> Result<Scope> {
    let fences = Fences::for_project(project)?;
    let exclude = exclude_set(project, &cfg.exclude)?;
    let has_commit = view.merge_base("HEAD").is_ok();
    let base = match &why {
        ScopeWhy::Since(rev) => view.merge_base(rev)?,
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
    let mut candidates: Vec<PathBuf> = match &why {
        ScopeWhy::Saved(p) => relative(p).into_iter().collect(),
        ScopeWhy::Named(ps) => ps.iter().filter_map(relative).collect(),
        _ if base.is_empty() => view.glob("**")?,
        _ => view.changed_since(&base)?,
    };
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|p| seen.insert(p.clone()));

    let mut scope = Scope {
        why,
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
        if !project.root.join(&file).exists() {
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

/// The definition line, trimmed, of the innermost unit that encloses UNIT
/// (olifant's `headerOf`); None when nothing encloses it.
fn header_of(lines: &[&str], units: &[Unit], unit: &Unit) -> Option<String> {
    let outer = units
        .iter()
        .filter(|m| m.name != unit.name && m.span.contains(&unit.span) && m.span != unit.span)
        .min_by_key(|m| m.span.end - m.span.start)?;
    let line = def_line(lines, outer)?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// The unit's definition line: the first line of its span, before its body,
/// that holds its own name (so a decorator above it is passed over); else
/// the span's first line.
fn def_line<'t>(lines: &[&'t str], u: &Unit) -> Option<&'t str> {
    let own = u.name.rsplit(['.', ':']).next().unwrap_or(&u.name);
    let last = u.body.map_or(u.span.end, |b| b.start.max(u.span.start));
    let at = |n: u32| lines.get(n.checked_sub(1)? as usize).copied();
    (u.span.start..=last)
        .filter_map(at)
        .find(|l| !own.is_empty() && l.contains(own))
        .or_else(|| at(u.span.start))
}
