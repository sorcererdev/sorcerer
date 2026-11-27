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
//! 2. an ignore file's text changes, which need not touch its directory's time: the digest of
//!    each entered directory's `.gitignore` and `.ignore` is kept, read by name rather than
//!    taken from the walk (which leaves out an ignore file that ignores itself, such as `*`),
//!    and a different digest walks that directory again;
//! 3. a link's target changes or appears (a link replaced in place, a directory on its path
//!    swapped for a link): every known link is resolved again on each ask, so this is never
//!    taken from the cache;
//! 4. the fences change (a `[model] not_evidence` edit, another root): the walk is redone.
//!
//! A directory the walk could not list (no permission) is never trusted: it is walked again on
//! every ask until it lists, since giving the permission back does not change its time.
//!
//! Point 1 is checked, not assumed: before each whole walk a probe adds an entry to a directory
//! of its own in the state directory and looks whether that directory's time moved. Some file
//! systems keep it (FAT on Windows); there nothing is kept and every ask walks the whole tree.
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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::Result;
use crate::adapters::fences::link_walk;
use crate::adapters::platform::paths::set_directory_modified;
use crate::domain::fences::Fences;
use crate::domain::project::Project;

/// How much older than the walk a directory's time must be to be trusted: more than the
/// coarsest clock tick of a common file system (FAT's 2 s).
pub const SETTLE: Duration = Duration::from_secs(3);

/// The files whose text decides what the walk leaves out, looked for in every directory.
const IGNORE_FILES: [&str; 2] = [".gitignore", ".ignore"];

/// One project's fenced links, kept between asks; see the module documentation for why a kept
/// answer is never stale.
#[derive(Debug)]
pub struct FencedLinksCache {
    /// The small file written before each walk, whose time is the file system's clock.
    clock_file: PathBuf,
    /// How the probe adds an entry to its directory; tests stand in for other file systems.
    add_probe_entry: fn(&Path) -> std::io::Result<()>,
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
            add_probe_entry: add_entry,
            seen: Mutex::new(None),
        }
    }

    /// The cache with another way of adding the probe's entry, for tests.
    #[doc(hidden)]
    pub fn with_probe(mut self, add_probe_entry: fn(&Path) -> std::io::Result<()>) -> Self {
        self.add_probe_entry = add_probe_entry;
        self
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
        let fresh_walk = || {
            self.directory_times_follow_entries()
                .then(|| self.walk(fences, &[PathBuf::new()]))
                .flatten()
        };
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
    /// under them, after reading the file system's clock. A directory that could not be read
    /// is recorded untrusted.
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
        let (entries, errors): (Vec<_>, Vec<_>) =
            link_walk(root, on_the_way).partition(std::result::Result::is_ok);
        let tree = entries
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((entry.path().strip_prefix(root).ok()?.to_path_buf(), entry)))
            .filter(|(rel, _)| tops.iter().any(|top| rel.starts_with(top)))
            .fold(TreeSeen::empty(fences), |tree, (rel, entry)| {
                tree.with_entry(rel, &entry, clock)
            });
        let unread = errors
            .iter()
            .filter_map(|error| error.as_ref().err())
            .flat_map(error_paths)
            .map(|path| {
                path.strip_prefix(root)
                    .map(Path::to_path_buf)
                    .unwrap_or_default()
            });
        Some(unread.fold(tree, TreeSeen::with_unread))
    }

    /// Whether this file system gives a directory a new time when an entry is added to it,
    /// which the cache relies on. The probe directory's time is set long back, an entry is
    /// added, and its time must have moved. Anything that fails answers no.
    fn directory_times_follow_entries(&self) -> bool {
        let Some(state) = self.clock_file.parent() else {
            return false;
        };
        let dir = state.join("fence-probe");
        let _ = std::fs::create_dir_all(&dir);
        if modified_dir(&dir).is_none() {
            return false;
        }
        let entry = dir.join("entry");
        let _ = std::fs::remove_file(&entry);
        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let moved = set_directory_modified(&dir, long_ago).is_ok()
            && (self.add_probe_entry)(&dir).is_ok()
            && modified_dir(&dir).is_some_and(|time| time != long_ago);
        let _ = std::fs::remove_file(&entry);
        moved
    }

    /// The file system's time now: the time it gives the clock file once written. The file is
    /// made new each time, never written through a link that stands in its place.
    fn file_system_now(&self) -> Option<SystemTime> {
        if let Some(dir) = self.clock_file.parent() {
            std::fs::create_dir_all(dir).ok()?;
        }
        write_new(&self.clock_file, b"fence walk\n").ok()?;
        std::fs::symlink_metadata(&self.clock_file)
            .ok()?
            .modified()
            .ok()
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

    /// One walked entry recorded: a link, or a directory with its time (kept only when it is
    /// [`SETTLE`] older than the walk's clock) and the digests of its ignore files, read by
    /// name (through a link, when one is a link, so a change at the target is seen too).
    fn with_entry(mut self, rel: PathBuf, entry: &ignore::DirEntry, clock: SystemTime) -> Self {
        let Some(kind) = entry.file_type() else {
            return self;
        };
        if kind.is_symlink() {
            self.links.insert(rel);
        } else if kind.is_dir() {
            for name in IGNORE_FILES {
                let text = digest(&entry.path().join(name));
                self.ignore_files.insert(rel.join(name), text);
            }
            let settled = modified(entry.path()).filter(|time| *time + SETTLE <= clock);
            self.dirs.insert(rel, settled);
        }
        self
    }

    /// The tree with the directory holding an unread path (the nearest recorded one at or
    /// above it, else the root) untrusted, so the next ask walks it again.
    fn with_unread(mut self, rel: PathBuf) -> Self {
        let holder = rel
            .ancestors()
            .find(|dir| self.dirs.contains_key(*dir))
            .map(Path::to_path_buf)
            .unwrap_or_default();
        self.dirs.insert(holder, None);
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
        let changed: Vec<PathBuf> = dirs.chain(ignore_dirs).collect();
        if changed.iter().any(|dir| dir.as_os_str().is_empty()) {
            return vec![PathBuf::new()];
        }
        topmost(changed)
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

/// The paths a walk error names (a directory or ignore file that could not be read).
fn error_paths(error: &ignore::Error) -> Vec<PathBuf> {
    match error {
        ignore::Error::WithPath { path, .. } => vec![path.clone()],
        ignore::Error::Loop { child, .. } => vec![child.clone()],
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            error_paths(err)
        }
        ignore::Error::Partial(errors) => errors.iter().flat_map(error_paths).collect(),
        // no path: the root stands for it, so the whole tree is walked again
        _ => vec![PathBuf::new()],
    }
}

/// Writes a file made new: whatever stands at the path (a link, say) is removed first, and the
/// file is created only if nothing is there, so nothing is written through a link.
fn write_new(path: &Path, text: &[u8]) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(text)
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

/// The probe's entry, made new.
fn add_entry(dir: &Path) -> std::io::Result<()> {
    write_new(&dir.join("entry"), b"")
}
