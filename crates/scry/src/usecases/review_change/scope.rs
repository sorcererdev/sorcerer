//! Working out what a pass reviews: its scope.
//!
//! `scope_of` sets up a `Scoping` (the fenced-off files, the `[scry]
//! exclude` globs, and the `Base` to compare with), lists the candidate
//! files, and looks at each one. A file is skipped (no language covers it,
//! it is excluded or fenced off, or it is not text), deleted since the
//! base, or present: then its units in scope are every unit (with `--all`,
//! or when there is no commit yet) or only the ones added or changed since
//! the base.
//!
//! Example: you changed `cancel` in `orders.py` and added `notes.txt`.
//! With the working tree as the request, the base is `HEAD`; `notes.txt`
//! is skipped (no language), and `orders.py` is present with one unit in
//! scope, `cancel`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use sorcerer_core::domain::compare::{UnitChange, compare_units};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Unit;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_core::{Error, Result};

use crate::domain::config::ScryConfig;
use crate::domain::scope::{Base, Scope, ScopeRequest, ScopedFile, ScopedUnit, header_of};

/// The scope of a pass for the request: the candidate files, each looked at
/// in turn. With `all`, every unit of each file is in scope, not only the
/// added and changed ones.
pub fn scope_of(
    project: &Project,
    view: &dyn CodebaseView,
    request: ScopeRequest,
    all: bool,
    cfg: &ScryConfig,
) -> Result<Scope> {
    let scoping = Scoping::new(project, view, &request, all, cfg)?;
    let files = scoping.candidate_files(&request)?;
    let empty = Scope::empty(request, scoping.base.clone());
    files.into_iter().try_fold(empty, |scope, file| {
        Ok(with_file(scope, scoping.look_at(file)?))
    })
}

/// The scope with one more file's place in it.
fn with_file(scope: Scope, file: FileInScope) -> Scope {
    match file {
        FileInScope::Skipped(file, why) => Scope {
            skipped: [scope.skipped, vec![(file, why)]].concat(),
            ..scope
        },
        FileInScope::Deleted(scoped) => Scope {
            files: [scope.files, vec![scoped]].concat(),
            ..scope
        },
        FileInScope::Present(scoped, units) => Scope {
            files: [scope.files, vec![scoped]].concat(),
            units: [scope.units, units].concat(),
            ..scope
        },
    }
}

/// A file's units against the base: the ones a pass reviews, and the ones
/// the same as at the base, by name and fingerprint.
struct AgainstBase {
    in_scope: Vec<Unit>,
    same: Vec<(String, Fingerprint)>,
}

/// What deciding each file's place in a pass's scope needs.
struct Scoping<'a> {
    project: &'a Project,
    view: &'a dyn CodebaseView,
    fences: Fences,
    exclude: GlobSet,
    /// What the code is compared with.
    base: Base,
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

    /// What the code is compared with: for `--since`, the commit where the
    /// branch left; else `HEAD`; when the repository has no commit yet,
    /// nothing.
    fn base_revision(view: &dyn CodebaseView, request: &ScopeRequest) -> Result<Base> {
        Ok(match request {
            ScopeRequest::Since(rev) => Base::Revision(view.merge_base(rev)?),
            _ if has_a_commit(view) => Base::Revision("HEAD".to_string()),
            _ => Base::NoCommitYet,
        })
    }

    /// The files to look at: the ones named, or every file when there is no
    /// commit yet, or the files changed since the base; each once, in order.
    fn candidate_files(&self, request: &ScopeRequest) -> Result<Vec<PathBuf>> {
        let files = match request {
            ScopeRequest::Files(named) => named
                .iter()
                .filter_map(|p| self.relative_to_the_project(p))
                .collect(),
            _ => match self.base.revision() {
                None => self.view.glob("**")?,
                Some(rev) => self.view.changed_since(rev)?,
            },
        };
        let mut seen = HashSet::new();
        Ok(files
            .into_iter()
            // `insert` is false for a file already seen, so each file passes once
            .filter(|f| seen.insert(f.clone()))
            .collect())
    }

    /// The path relative to the project's root; none for an absolute path
    /// outside the project.
    fn relative_to_the_project(&self, path: &Path) -> Option<PathBuf> {
        if path.is_absolute() {
            self.fences.relative(path)
        } else {
            Some(path.to_path_buf())
        }
    }

    /// Where the file stands in the scope; it is read only as far as that
    /// needs.
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
        let against_base = self.compared_with_base(&file, &units_now);
        let units = self.scoped_units(&file, &text, &units_now, against_base.in_scope);
        let scoped = self.scoped_file(file, text, units_now, against_base.same)?;
        Ok(FileInScope::Present(scoped, units))
    }

    /// Why the file is left out without being read, if it is; the first
    /// reason that holds wins.
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
        let base = match self.base.revision() {
            Some(rev) if self.view.text_at(&file, rev)?.is_some() => rev,
            _ => {
                return Ok(FileInScope::Skipped(
                    file,
                    "deleted, and not at the base".into(),
                ));
            }
        };
        Ok(FileInScope::Deleted(ScopedFile {
            diff: self.view.diff(&file, base)?,
            file,
            units_now: Vec::new(),
            same: Vec::new(),
            deleted: true,
        }))
    }

    /// The file's text; none when the view finds it is not text (an image,
    /// say). Any other read error is returned.
    fn text_unless_binary(&self, file: &Path) -> Result<Option<String>> {
        match self.view.text(file) {
            Ok(text) => Ok(Some(text)),
            Err(Error::NotText { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The file's units against the base: the ones a pass reviews (all of
    /// them with `--all` or when there is no commit yet, else the ones added
    /// or changed since the base), and the ones the same as at the base
    /// (none with `--all` or no commit).
    fn compared_with_base(&self, file: &Path, units_now: &[Unit]) -> AgainstBase {
        let rev = match self.base.revision() {
            Some(rev) if !self.all => rev,
            _ => {
                return AgainstBase {
                    in_scope: units_now.to_vec(),
                    same: Vec::new(),
                };
            }
        };
        // Units at the base that do not read count as none: all added.
        let units_at_base = self.view.units_at(file, rev).unwrap_or_default();
        let (same, changed): (Vec<UnitChange>, Vec<UnitChange>) =
            compare_units(&units_at_base, units_now)
                .into_iter()
                .partition(|change| matches!(change, UnitChange::Same(_)));
        AgainstBase {
            in_scope: changed
                .into_iter()
                .filter_map(UnitChange::unit_now)
                .collect(),
            same: same
                .into_iter()
                .filter_map(|change| match change {
                    UnitChange::Same(unit) => Some((unit.name, unit.fingerprint)),
                    _ => None,
                })
                .collect(),
        }
    }

    /// The file's units in scope as the scope holds them, each with the
    /// definition line of the unit that encloses it (`header_of`).
    fn scoped_units(
        &self,
        file: &Path,
        text: &str,
        units_now: &[Unit],
        in_scope: Vec<Unit>,
    ) -> Vec<ScopedUnit> {
        let lines: Vec<&str> = text.lines().collect();
        in_scope
            .into_iter()
            .map(|unit| ScopedUnit {
                file: file.to_path_buf(),
                header: header_of(&lines, units_now, &unit),
                unit,
            })
            .collect()
    }

    /// The file as the scope holds it: its diff against the base, or its
    /// whole text when that diff is empty (no commit yet, untracked, or
    /// unchanged), and the name of every unit it has now.
    fn scoped_file(
        &self,
        file: PathBuf,
        text: String,
        units_now: Vec<Unit>,
        same: Vec<(String, Fingerprint)>,
    ) -> Result<ScopedFile> {
        let diff = match self.base.revision() {
            None => String::new(),
            Some(rev) => self.view.diff(&file, rev)?,
        };
        Ok(ScopedFile {
            diff: if diff.is_empty() { text } else { diff },
            units_now: units_now.into_iter().map(|u| u.name).collect(),
            same,
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
