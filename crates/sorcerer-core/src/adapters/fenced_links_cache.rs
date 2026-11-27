//! A project's fenced links ([`Fences::fenced_links`]) kept between tasks, so that starting a
//! task does not walk the whole tree again. A fenced path must never reach a model, so the cache
//! may cost a walk but may never miss a link: each time it is asked, it re-walks every directory
//! that could hold something it has not seen, and checks again every link it knows.
//!
//! Why nothing can be missed. The walk's result depends only on the tree under the root: the
//! entries of the directories it enters and the text of the `.gitignore` and `.ignore` files in
//! them ([`link_walk`]). So the answer can only change when
//!
//! 1. a directory it entered gains, loses or swaps an entry (a new link, a new directory, a link
//!    replaced by another, a new ignore file). The file system then gives that directory a new
//!    modification time, on every platform, so a directory whose time differs is walked again,
//!    with everything under it;
//! 2. an ignore file's text changes, which need not touch its directory's time: each one's
//!    digest is kept, and a different digest walks its directory again;
//! 3. a link's target changes or appears (a link replaced in place, a directory on its path
//!    swapped for a link): every known link is resolved again on each ask, so this is never
//!    taken from the cache;
//! 4. the fences change (a `[model] not_evidence` edit, another root): the walk is redone.
//!
//! One trap remains in 1: a change in the same tick of the clock as the time recorded (2 s on
//! FAT, finer elsewhere) leaves the time as it was. So a directory's time is trusted only when it
//! is at least [`SETTLE`] older than the walk that recorded it, as told by the file system's own
//! clock (the time it gives a small file it writes just before walking), which also holds on a
//! network drive whose clock differs from this machine's. A directory changed just before a walk
//! is walked again next time. When that clock file cannot be written, nothing is kept and every
//! ask walks the whole tree, as without the cache.
//!
//! Example: after a first task, `ln -s ~/.ssh src/keys` changes `src/`'s time, so the next
//! task's start walks `src/` again (not the whole tree) and denies `src/keys`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::Result;
use crate::adapters::fences::link_walk;
use crate::domain::fences::Fences;
use crate::domain::project::Project;

/// How much older than the walk a directory's time must be to be trusted: more than the
/// coarsest clock tick of a common file system (FAT's 2 s).
pub const SETTLE: Duration = Duration::from_secs(3);

/// The files whose text decides what the walk leaves out.
const IGNORE_FILES: [&str; 2] = [".gitignore", ".ignore"];

/// One project's fenced links, kept between asks; see the module documentation for why a kept
/// answer is never stale.
#[derive(Debug)]
pub struct FencedLinksCache {
    /// The small file written before each walk, whose time is the file system's clock.
    clock_file: PathBuf,
    seen: Mutex<Option<TreeSeen>>,
}

/// What the last walks saw, relative to the root.
#[derive(Debug, Clone)]
struct TreeSeen {
    root: PathBuf,
    patterns: Vec<String>,
    /// Each directory entered, with its time when that time can be trusted; `None` makes the
    /// next ask walk it again.
    dirs: BTreeMap<PathBuf, Option<SystemTime>>,
    /// Each ignore file, with the digest of its text (`None` when it could not be read).
    ignore_files: BTreeMap<PathBuf, Option<Vec<u8>>>,
    /// Every link, fenced or not; each is resolved again on every ask.
    links: BTreeSet<PathBuf>,
}

impl FencedLinksCache {
    /// The cache for a project, its clock file in the project's state directory (which the
    /// walk does not enter).
    pub fn for_project(p: &Project) -> Result<FencedLinksCache> {
        Ok(FencedLinksCache::with_clock_file(
            p.state_dir()?.join("fence-walk.clock"),
        ))
    }

    /// A cache whose clock file is `clock_file`; it must be on the project's file system and
    /// outside what the walk enters (the state directory, say).
    pub fn with_clock_file(clock_file: PathBuf) -> FencedLinksCache {
        FencedLinksCache {
            clock_file,
            seen: Mutex::new(None),
        }
    }

    /// The same list as [`Fences::fenced_links`], walking only the directories that changed
    /// since the last ask.
    pub fn fenced_links(&self, fences: &Fences) -> Vec<PathBuf> {
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = seen.take().filter(|tree| tree.is_for(fences));
        *seen = self.walk_what_changed(previous, fences);
        match &*seen {
            Some(tree) => tree.fenced_links(fences),
            None => fences.fenced_links(),
        }
    }

    /// The tree as it is now: the previous one with the directories that may have changed
    /// walked again, or a whole new walk. `None` when the file system's clock cannot be read.
    fn walk_what_changed(&self, previous: Option<TreeSeen>, fences: &Fences) -> Option<TreeSeen> {
        let fresh_walk = || self.walk(fences, &[PathBuf::new()]);
        let Some(previous) = previous else {
            return fresh_walk();
        };
        let changed = previous.dirs_that_may_have_changed();
        if changed.is_empty() {
            return Some(previous);
        }
        if changed.iter().any(|dir| dir.as_os_str().is_empty()) {
            return fresh_walk();
        }
        let walked = self.walk(fences, &changed)?;
        Some(previous.without(&changed).merged(walked))
    }

    /// Walks the given directories (relative to the root, `""` for all of it) and everything
    /// under them, after reading the file system's clock.
    fn walk(&self, fences: &Fences, tops: &[PathBuf]) -> Option<TreeSeen> {
        let clock = self.file_system_now()?;
        let root = fences.root();
        let tops = tops.to_vec();
        let wanted = tops.clone();
        let on_the_way = move |rel: &Path| {
            wanted
                .iter()
                .any(|top| rel.starts_with(top) || top.starts_with(rel))
        };
        let tree = link_walk(root, on_the_way)
            .filter_map(|entry| Some((entry.path().strip_prefix(root).ok()?.to_path_buf(), entry)))
            .filter(|(rel, _)| tops.iter().any(|top| rel.starts_with(top)))
            .fold(TreeSeen::empty(fences), |tree, (rel, entry)| {
                tree.with_entry(rel, &entry, clock)
            });
        Some(tree)
    }

    /// The file system's time now: the time it gives the clock file once written.
    fn file_system_now(&self) -> Option<SystemTime> {
        if let Some(dir) = self.clock_file.parent() {
            std::fs::create_dir_all(dir).ok()?;
        }
        std::fs::write(&self.clock_file, b"fence walk\n").ok()?;
        std::fs::metadata(&self.clock_file).ok()?.modified().ok()
    }
}

impl TreeSeen {
    fn empty(fences: &Fences) -> TreeSeen {
        TreeSeen {
            root: fences.root().to_path_buf(),
            patterns: fences.patterns(),
            dirs: BTreeMap::new(),
            ignore_files: BTreeMap::new(),
            links: BTreeSet::new(),
        }
    }

    /// Whether this walk was made under these fences (the same root and patterns).
    fn is_for(&self, fences: &Fences) -> bool {
        self.root == fences.root() && self.patterns == fences.patterns()
    }

    /// The known links that lead out of the project or onto a fenced path, each resolved now.
    fn fenced_links(&self, fences: &Fences) -> Vec<PathBuf> {
        self.links
            .iter()
            .filter(|link| !fences.allows(link))
            .cloned()
            .collect()
    }

    /// One walked entry recorded: a link, a directory with its time (kept only when it is
    /// [`SETTLE`] older than the walk's clock), or an ignore file with its digest. An ignore
    /// file that is a link is both: its text is read through the link, so a change at the
    /// target is seen too.
    fn with_entry(mut self, rel: PathBuf, entry: &ignore::DirEntry, clock: SystemTime) -> Self {
        let Some(kind) = entry.file_type() else {
            return self;
        };
        if is_ignore_file(&rel) && !kind.is_dir() {
            self.ignore_files.insert(rel.clone(), digest(entry.path()));
        }
        if kind.is_symlink() {
            self.links.insert(rel);
        } else if kind.is_dir() {
            let settled = modified(entry.path()).filter(|time| *time + SETTLE <= clock);
            self.dirs.insert(rel, settled);
        }
        self
    }

    /// The topmost directories that may hold something this walk has not seen: a directory
    /// whose time differs, was not trusted, or that is gone or no longer a directory, and the
    /// directory of each ignore file whose text differs.
    fn dirs_that_may_have_changed(&self) -> Vec<PathBuf> {
        let dirs = self
            .dirs
            .iter()
            .filter(|(rel, time)| time.is_none() || modified_dir(&self.root.join(rel)) != **time)
            .map(|(rel, _)| rel.clone());
        let ignore_dirs = self
            .ignore_files
            .iter()
            .filter(|(rel, text)| digest(&self.root.join(rel)) != **text)
            .map(|(rel, _)| rel.parent().map(Path::to_path_buf).unwrap_or_default());
        topmost(dirs.chain(ignore_dirs).collect())
    }

    /// This tree without anything at or under the given directories.
    fn without(mut self, tops: &[PathBuf]) -> Self {
        let outside = |rel: &PathBuf| !tops.iter().any(|top| rel.starts_with(top));
        self.dirs.retain(|rel, _| outside(rel));
        self.ignore_files.retain(|rel, _| outside(rel));
        self.links.retain(outside);
        self
    }

    fn merged(mut self, walked: TreeSeen) -> Self {
        self.dirs.extend(walked.dirs);
        self.ignore_files.extend(walked.ignore_files);
        self.links.extend(walked.links);
        self
    }
}

/// The directories none of whose ancestors are in the list.
fn topmost(mut dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    dirs.sort();
    dirs.dedup();
    dirs.iter()
        .filter(|dir| {
            !dirs
                .iter()
                .any(|other| other != *dir && dir.starts_with(other))
        })
        .cloned()
        .collect()
}

fn is_ignore_file(rel: &Path) -> bool {
    rel.file_name()
        .is_some_and(|name| IGNORE_FILES.iter().any(|ignore| name == *ignore))
}

/// The path's own time (a link's, not its target's).
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path).ok()?.modified().ok()
}

/// The time of a directory that is still a directory (not a link to one); `None` otherwise.
fn modified_dir(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    meta.is_dir().then(|| meta.modified().ok()).flatten()
}

fn digest(path: &Path) -> Option<Vec<u8>> {
    let text = std::fs::read(path).ok()?;
    Some(Sha256::digest(text).to_vec())
}
