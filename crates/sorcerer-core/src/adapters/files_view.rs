//! `FilesView` answers the codebase view's questions from the files on disk, git and
//! tree-sitter.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use globset::Glob;
use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use grep_searcher::sinks::UTF8;

use crate::adapters::grammar::GrammarCache;
use crate::domain::facts::{Facts, facts_of};
use crate::domain::project::Project;
use crate::domain::unit::{Unit, units_of};
use crate::ports::view::{CodebaseView, GrepHit};
use crate::{Error, Result};

/// The codebase as it is on disk, with git for its history.
pub struct FilesView {
    project: Arc<Project>,
    cache: GrammarCache,
}

impl FilesView {
    /// A view of the project; grammars are built or loaded through the cache.
    pub fn new(project: Arc<Project>, cache: GrammarCache) -> FilesView {
        FilesView { project, cache }
    }

    /// The project this view reads.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The grammar cache this view parses with.
    pub fn cache(&self) -> &GrammarCache {
        &self.cache
    }
}

impl CodebaseView for FilesView {
    fn exists(&self, path: &Path) -> bool {
        self.project.root.join(path).metadata().is_ok()
    }

    fn text(&self, path: &Path) -> Result<String> {
        let full = self.project.root.join(path);
        let bytes = std::fs::read(&full).map_err(|e| Error::io(&full, e))?;
        String::from_utf8(bytes).map_err(|_| Error::NotText {
            path: path.to_path_buf(),
        })
    }

    fn text_at(&self, path: &Path, rev: &str) -> Result<Option<String>> {
        self.require_commit(rev)?;
        let object = format!("{rev}:./{}", path.display());
        if !self.git_has(&object) {
            return Ok(None);
        }
        String::from_utf8(self.git_show(&object)?)
            .map(Some)
            .map_err(|_| Error::NotText {
                path: path.to_path_buf(),
            })
    }

    fn units(&self, path: &Path) -> Result<Vec<Unit>> {
        if self.project.languages.for_path(path).is_none() {
            return Ok(Vec::new());
        }
        let text = self.text(path)?;
        self.units_in(path, &text)
    }

    fn units_at(&self, path: &Path, rev: &str) -> Result<Vec<Unit>> {
        if self.project.languages.for_path(path).is_none() {
            return Ok(Vec::new());
        }
        match self.text_at(path, rev)? {
            Some(text) => self.units_in(path, &text),
            None => Ok(Vec::new()),
        }
    }

    fn facts(&self, path: &Path, unit: &Unit) -> Result<Facts> {
        let Some(lang) = self.project.languages.for_path(path) else {
            return Ok(Facts::default());
        };
        let loaded = self.project.languages.load(&lang.name, &self.cache)?;
        facts_of(&loaded, &self.text(path)?, unit)
    }

    fn grep(&self, pattern: &str, glob: Option<&str>) -> Result<Vec<GrepHit>> {
        let matcher = RegexMatcher::new(pattern).map_err(|e| Error::Config {
            file: PathBuf::from("grep"),
            why: e.to_string(),
        })?;
        let only = glob.map(glob_matcher).transpose()?;
        Ok(self
            .files()
            .into_iter()
            .filter(|path| only.as_ref().is_none_or(|only| only.is_match(path)))
            .flat_map(|path| self.grep_file(&matcher, path))
            .collect())
    }

    fn glob(&self, pattern: &str) -> Result<Vec<PathBuf>> {
        let matcher = glob_matcher(pattern)?;
        Ok(self
            .files()
            .into_iter()
            .filter(|path| matcher.is_match(path))
            .collect())
    }

    fn changed_since(&self, rev: &str) -> Result<Vec<PathBuf>> {
        let changed = self.git_or_error(&[
            "diff",
            "--name-only",
            "--no-renames",
            "--relative",
            "--end-of-options",
            rev,
            "--",
        ])?;
        let untracked = self.git_or_error(&["ls-files", "--others", "--exclude-standard"])?;
        let mut paths: Vec<PathBuf> = changed
            .lines()
            .chain(untracked.lines())
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect();
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    fn diff(&self, path: &Path, rev: &str) -> Result<String> {
        let relative = path.display().to_string();
        self.git_or_error(&[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--relative",
            "--end-of-options",
            rev,
            "--",
            &relative,
        ])
    }

    fn merge_base(&self, rev: &str) -> Result<String> {
        self.git(&["merge-base", "--end-of-options", rev, "HEAD"])
            .map(|s| s.trim().to_string())
            .map_err(|why| Error::Git {
                why: format!("no merge base with {rev}: {why}"),
            })
    }
}

impl FilesView {
    /// Every file under the root that git would not ignore, relative to the root, sorted.
    fn files(&self) -> Vec<PathBuf> {
        let root = &self.project.root;
        let mut files: Vec<PathBuf> = ignore::WalkBuilder::new(root)
            .require_git(false)
            .build()
            .flatten()
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .filter_map(|e| e.path().strip_prefix(root).ok().map(Path::to_path_buf))
            .collect();
        files.sort();
        files
    }

    /// The lines of one file the regular expression matches. A file that cannot be searched
    /// (not text, gone since the listing) has none.
    fn grep_file(&self, matcher: &RegexMatcher, path: PathBuf) -> Vec<GrepHit> {
        let mut hits = Vec::new();
        let full = self.project.root.join(&path);
        let _ = Searcher::new().search_path(
            matcher,
            &full,
            UTF8(|line, text| {
                hits.push(GrepHit {
                    path: path.clone(),
                    line: line as u32,
                    text: text.trim_end_matches(['\n', '\r']).to_string(),
                });
                Ok(true)
            }),
        );
        hits
    }

    /// The units of a file's text, using the language its path belongs to.
    fn units_in(&self, path: &Path, text: &str) -> Result<Vec<Unit>> {
        match self.project.languages.for_path(path) {
            None => Ok(Vec::new()),
            Some(lang) => {
                let loaded = self.project.languages.load(&lang.name, &self.cache)?;
                units_of(&loaded, text)
            }
        }
    }

    /// An error naming `rev` unless it is a commit.
    fn require_commit(&self, rev: &str) -> Result<()> {
        let commit = format!("{rev}^{{commit}}");
        self.git(&[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &commit,
        ])
        .map(|_| ())
        .map_err(|_| Error::Git {
            why: format!("{rev} is not a commit"),
        })
    }

    /// Whether git has the object, such as `HEAD:./src/orders.rs` (that file at that commit).
    fn git_has(&self, object: &str) -> bool {
        self.git(&["cat-file", "-e", "--end-of-options", object])
            .is_ok()
    }

    /// The object's raw bytes, which may not be UTF-8.
    fn git_show(&self, object: &str) -> Result<Vec<u8>> {
        Command::new("git")
            .arg("-C")
            .arg(&self.project.root)
            .args(["show", "--end-of-options", object])
            .output()
            .map(|out| out.stdout)
            .map_err(|e| Error::Git { why: e.to_string() })
    }

    fn git_or_error(&self, args: &[&str]) -> Result<String> {
        self.git(args).map_err(|why| Error::Git { why })
    }

    /// Runs git in the project root: its output on success, else what it printed to stderr.
    fn git(&self, args: &[&str]) -> std::result::Result<String, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.project.root)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

/// A glob over paths; a bad pattern is a config error naming it.
fn glob_matcher(pattern: &str) -> Result<globset::GlobMatcher> {
    Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| Error::Config {
            file: PathBuf::from(pattern),
            why: e.to_string(),
        })
}
