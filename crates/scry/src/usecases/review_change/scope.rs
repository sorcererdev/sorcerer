//! Working out a pass's scope from the request, through the view.
//!
//! `scope_of` sets up the `Scoping` (the fences, the `[scry] exclude` set
//! and the `base_revision`), takes the `candidate_files`, and has it
//! `look_at` each one. A file is skipped (`reason_to_skip`, or not text:
//! `text_unless_binary`), deleted (`no_longer_in_the_tree`), or present:
//! its `units_in_scope` become `scoped_units`, and the file a
//! `scoped_file`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    // 1. Set up: the files scry must not read (fences), the `[scry]
    //    exclude` globs, and the revision to compare with (the base: HEAD,
    //    the merge base for --since, or none when there is no commit yet).
    let scoping = Scoping::new(project, view, &request, all, cfg)?;
    // 2. The candidate files: the ones named, or every file when there is
    //    no commit yet, or the files changed since the base.
    let files = scoping.candidate_files(&request)?;
    let mut scope = Scope::empty(request, scoping.base.clone());
    // 3. Look at each: skipped (no language, excluded, fenced, not text),
    //    deleted since the base, or present with its units in scope (every
    //    unit with --all or no commit, else the added and changed ones).
    for file in files {
        match scoping.look_at(file)? {
            FileInScope::Skipped(file, why) => scope.skipped.push((file, why)),
            FileInScope::Deleted(scoped) => scope.files.push(scoped),
            FileInScope::Present(scoped, units) => {
                scope.units.extend(units);
                scope.files.push(scoped);
            }
        }
    }
    Ok(scope)
}

/// What deciding each file's place in a pass's scope needs.
struct Scoping<'a> {
    project: &'a Project,
    view: &'a dyn CodebaseView,
    fences: Fences,
    exclude: GlobSet,
    /// The revision units are compared with; empty when the repository has
    /// no commit.
    base: String,
    /// Every unit of a file is in scope, not only the added and changed ones.
    all: bool,
}

/// Where one candidate file stands: skipped (and why), deleted since the
/// base, or present with its units in scope.
enum FileInScope {
    Skipped(PathBuf, String),
    Deleted(ScopedFile),
    Present(ScopedFile, Vec<ScopedUnit>),
}

impl<'a> Scoping<'a> {
    fn new(
        project: &'a Project,
        view: &'a dyn CodebaseView,
        request: &ScopeRequest,
        all: bool,
        cfg: &ScryConfig,
    ) -> Result<Scoping<'a>> {
        Ok(Scoping {
            fences: Fences::for_project(project)?,
            exclude: exclude_set(project, &cfg.exclude)?,
            base: Self::base_revision(view, request)?,
            project,
            view,
            all,
        })
    }

    /// The revision units are compared with: the merge base for `Since`,
    /// else `HEAD`; empty when the repository has no commit.
    fn base_revision(view: &dyn CodebaseView, request: &ScopeRequest) -> Result<String> {
        Ok(match request {
            ScopeRequest::Since(rev) => view.merge_base(rev)?,
            _ if has_a_commit(view) => "HEAD".to_string(),
            _ => String::new(),
        })
    }

    /// The files named, or every file when there is no base, or the files
    /// changed since the base; each once, in order.
    fn candidate_files(&self, request: &ScopeRequest) -> Result<Vec<PathBuf>> {
        let files = match request {
            ScopeRequest::Files(named) => named
                .iter()
                .filter_map(|p| self.relative_to_the_project(p))
                .collect(),
            _ if self.base.is_empty() => self.view.glob("**")?,
            _ => self.view.changed_since(&self.base)?,
        };
        let mut seen = HashSet::new();
        Ok(files
            .into_iter()
            // `insert` is false for a file already seen, so each file passes once
            .filter(|f| seen.insert(f.clone()))
            .collect())
    }

    /// PATH relative to the project's root; None for an absolute path
    /// outside it.
    fn relative_to_the_project(&self, path: &Path) -> Option<PathBuf> {
        if path.is_absolute() {
            self.fences.relative(path)
        } else {
            Some(path.to_path_buf())
        }
    }

    /// Where FILE stands in the scope, read only as far as that needs.
    fn look_at(&self, file: PathBuf) -> Result<FileInScope> {
        if let Some(why) = self.reason_to_skip(&file) {
            return Ok(FileInScope::Skipped(file, why.into()));
        }
        if !self.view.exists(&file) {
            return self.no_longer_in_the_tree(file);
        }
        let Some(text) = self.text_unless_binary(&file)? else {
            return Ok(FileInScope::Skipped(file, "not text".into()));
        };
        let units_now = self.view.units(&file)?;
        let units = self.scoped_units(&file, &text, &units_now);
        let scoped = self.scoped_file(file, text, units_now)?;
        Ok(FileInScope::Present(scoped, units))
    }

    /// Why FILE is left out without being read, if it is; the first reason
    /// that holds wins.
    fn reason_to_skip(&self, file: &Path) -> Option<&'static str> {
        if self.project.languages.for_path(file).is_none() {
            Some("no language")
        } else if self.exclude.is_match(file) {
            Some("excluded by [scry] exclude")
        } else if !self.fences.allows(file) {
            Some("fenced")
        } else {
            None
        }
    }

    /// A file no longer in the tree: deleted, with its diff, when it was at
    /// the base; else left out.
    fn no_longer_in_the_tree(&self, file: PathBuf) -> Result<FileInScope> {
        let was_at_the_base =
            !self.base.is_empty() && self.view.text_at(&file, &self.base)?.is_some();
        if !was_at_the_base {
            return Ok(FileInScope::Skipped(
                file,
                "deleted, and not at the base".into(),
            ));
        }
        Ok(FileInScope::Deleted(ScopedFile {
            diff: self.view.diff(&file, &self.base)?,
            file,
            units_now: Vec::new(),
            deleted: true,
        }))
    }

    /// FILE's text; None when the view finds it is not text; any other read
    /// error is returned.
    fn text_unless_binary(&self, file: &Path) -> Result<Option<String>> {
        match self.view.text(file) {
            Ok(text) => Ok(Some(text)),
            Err(Error::NotText { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The units of FILE a pass looks at: all of them with `all` or when
    /// there is no base, else the ones added or changed since the base.
    fn units_in_scope(&self, file: &Path, units_now: &[Unit]) -> Vec<Unit> {
        if self.all || self.base.is_empty() {
            return units_now.to_vec();
        }
        // Units at the base that do not read count as none: all added.
        let units_at_base = self.view.units_at(file, &self.base).unwrap_or_default();
        compare_units(&units_at_base, units_now)
            .into_iter()
            .filter_map(UnitChange::unit_now)
            .collect()
    }

    /// The units of FILE in scope as the scope holds them, each with the
    /// first line of the unit that encloses it.
    fn scoped_units(&self, file: &Path, text: &str, units_now: &[Unit]) -> Vec<ScopedUnit> {
        let lines: Vec<&str> = text.lines().collect();
        self.units_in_scope(file, units_now)
            .into_iter()
            .map(|unit| ScopedUnit {
                file: file.to_path_buf(),
                header: header_of(&lines, units_now, &unit),
                unit,
            })
            .collect()
    }

    /// FILE as the scope holds it: its diff against the base, or its whole
    /// TEXT when that diff is empty (no base, untracked, or unchanged), and
    /// the name of every unit it has now.
    fn scoped_file(&self, file: PathBuf, text: String, units_now: Vec<Unit>) -> Result<ScopedFile> {
        let diff = if self.base.is_empty() {
            String::new()
        } else {
            self.view.diff(&file, &self.base)?
        };
        Ok(ScopedFile {
            diff: if diff.is_empty() { text } else { diff },
            units_now: units_now.into_iter().map(|u| u.name).collect(),
            file,
            deleted: false,
        })
    }
}

/// Whether the repository has any commit, so `HEAD` names a revision.
fn has_a_commit(view: &dyn CodebaseView) -> bool {
    view.merge_base("HEAD").is_ok()
}

/// The `[scry] exclude` globs as one set; a bad glob is a config error.
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
