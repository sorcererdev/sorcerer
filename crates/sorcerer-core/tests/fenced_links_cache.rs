//! The fenced-links cache answers as a fresh walk would, after any change, and faster when
//! nothing changed.
mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use sorcerer_core::adapters::fenced_links_cache::FencedLinksCache;
use sorcerer_core::adapters::platform::paths::{
    allow_listing, forbid_listing, link_to, set_directory_modified,
};
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;

fn project() -> tempfile::TempDir {
    let d = support::git_project();
    support::write(d.path(), "src/a.py", "x = 1\n");
    support::write(d.path(), "src/deep/er/b.py", "y = 2\n");
    support::write(d.path(), ".env", "SECRET=x\n");
    support::write(d.path(), "docs/plans/today.md", "plan\n");
    d
}

fn fences(d: &Path) -> Fences {
    Fences::for_project(&Project::open(d).unwrap()).unwrap()
}

fn fences_with_not_evidence(d: &Path, entries: &[&str]) -> Fences {
    let root = dunce::canonicalize(d).unwrap();
    let entries: Vec<String> = entries.iter().map(|e| e.to_string()).collect();
    Fences::new(root, &entries).unwrap()
}

fn cache(d: &Path) -> FencedLinksCache {
    FencedLinksCache::for_project(&Project::open(d).unwrap()).unwrap()
}

fn sorted(mut links: Vec<PathBuf>) -> Vec<PathBuf> {
    links.sort();
    links
}

/// Sets every directory under the root (and the root) to one time long ago, so the cache trusts
/// their times as it would for a tree nobody touched in the last seconds. It is the same time
/// each call, so a directory aged again looks untouched. The cache is made first: making it adds
/// the state directory to the root.
fn age_directories(root: &Path) {
    let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for entry in walk_dirs(root) {
        set_directory_modified(&entry, long_ago).unwrap();
    }
}

fn walk_dirs(dir: &Path) -> Vec<PathBuf> {
    let below = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|kind| kind.is_dir()))
        .flat_map(|e| walk_dirs(&e.path()));
    std::iter::once(dir.to_path_buf()).chain(below).collect()
}

#[test]
fn the_cache_answers_as_a_walk_does() {
    let d = project();
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("src/out")).unwrap();
    link_to(&d.path().join(".env"), &d.path().join("src/deep/e")).unwrap();
    link_to(&d.path().join("src/a.py"), &d.path().join("src/ok")).unwrap();
    let c = cache(d.path());
    age_directories(d.path());
    let f = fences(d.path());
    let expected = vec![PathBuf::from("src/deep/e"), PathBuf::from("src/out")];
    assert_eq!(sorted(f.fenced_links()), expected);
    assert_eq!(sorted(c.fenced_links(&f)), expected, "first ask");
    assert_eq!(sorted(c.fenced_links(&f)), expected, "cached ask");
}

#[test]
fn a_link_added_after_the_first_ask_is_fenced_on_the_next() {
    let d = project();
    let c = cache(d.path());
    age_directories(d.path());
    let f = fences(d.path());
    assert!(c.fenced_links(&f).is_empty());
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("src/deep/er/out")).unwrap();
    support::write(d.path(), "src/new/.env.local", "SECRET=y\n");
    link_to(
        &d.path().join("src/new/.env.local"),
        &d.path().join("src/new/plain"),
    )
    .unwrap();
    assert_eq!(
        sorted(c.fenced_links(&f)),
        vec![
            PathBuf::from("src/deep/er/out"),
            PathBuf::from("src/new/plain")
        ]
    );
}

#[test]
fn a_link_added_in_the_same_moment_as_the_walk_is_fenced_on_the_next() {
    // the directories were just made, so their times are not trusted yet
    let d = project();
    let f = fences(d.path());
    let c = cache(d.path());
    assert!(c.fenced_links(&f).is_empty());
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("src/out")).unwrap();
    assert_eq!(c.fenced_links(&f), vec![PathBuf::from("src/out")]);
}

#[test]
fn a_link_pointed_elsewhere_is_fenced_even_when_its_directory_looks_unchanged() {
    let d = project();
    link_to(&d.path().join("src/a.py"), &d.path().join("src/deep/ok")).unwrap();
    let c = cache(d.path());
    age_directories(d.path());
    let f = fences(d.path());
    assert!(c.fenced_links(&f).is_empty());
    std::fs::remove_file(d.path().join("src/deep/ok")).unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("src/deep/ok")).unwrap();
    age_directories(d.path());
    assert_eq!(c.fenced_links(&f), vec![PathBuf::from("src/deep/ok")]);
}

/// lackey-notes-workable review M1: git checks out a force-added ignored link, so the walk
/// reads no ignore file and the cache names such a link from the first ask.
#[test]
fn a_link_in_an_ignored_directory_is_named() {
    let d = project();
    support::write(d.path(), ".gitignore", "build/\n.sorcerer/notes/\n");
    support::write(d.path(), "build/keep.txt", "");
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("build/out")).unwrap();
    std::fs::create_dir_all(d.path().join(".sorcerer/notes")).unwrap();
    link_to(
        &d.path().join(".env"),
        &d.path().join(".sorcerer/notes/env.md"),
    )
    .unwrap();
    let c = cache(d.path());
    age_directories(d.path());
    let f = fences(d.path());
    let wanted = vec![
        PathBuf::from(".sorcerer/notes/env.md"),
        PathBuf::from("build/out"),
    ];
    assert_eq!(sorted(c.fenced_links(&f)), wanted);
    assert_eq!(sorted(f.fenced_links()), wanted);
    std::fs::write(d.path().join(".gitignore"), "").unwrap();
    age_directories(d.path());
    assert_eq!(sorted(c.fenced_links(&f)), wanted);
}

#[test]
fn a_fence_config_change_takes_effect_at_once() {
    let d = project();
    link_to(
        &d.path().join("docs/plans/today.md"),
        &d.path().join("src/plan"),
    )
    .unwrap();
    let c = cache(d.path());
    age_directories(d.path());
    assert!(c.fenced_links(&fences(d.path())).is_empty());
    let fenced_plans = fences_with_not_evidence(d.path(), &["docs/plans"]);
    assert_eq!(
        c.fenced_links(&fenced_plans),
        vec![PathBuf::from("src/plan")]
    );
    assert!(c.fenced_links(&fences(d.path())).is_empty(), "and back");
}

#[test]
fn without_a_clock_file_every_ask_walks() {
    let d = project();
    support::write(d.path(), "not-a-dir", "");
    age_directories(d.path());
    let f = fences(d.path());
    let c = FencedLinksCache::with_clock_file(d.path().join("not-a-dir/clock"));
    assert!(c.fenced_links(&f).is_empty());
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("src/out")).unwrap();
    age_directories(d.path());
    assert_eq!(c.fenced_links(&f), vec![PathBuf::from("src/out")]);
}

#[test]
fn a_cached_ask_is_faster_than_a_walk_on_a_large_tree() {
    let d = project();
    for dir in 0..200 {
        for sub in 0..5 {
            for file in 0..20 {
                support::write(d.path(), &format!("d{dir}/s{sub}/f{file}.txt"), "x");
            }
        }
    }
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("d7/s3/out")).unwrap();
    let c = cache(d.path());
    age_directories(d.path());
    let f = fences(d.path());
    assert_eq!(c.fenced_links(&f), vec![PathBuf::from("d7/s3/out")]);
    let fastest = |ask: &dyn Fn() -> Vec<PathBuf>| {
        (0..3)
            .map(|_| {
                let start = Instant::now();
                assert_eq!(ask(), vec![PathBuf::from("d7/s3/out")]);
                start.elapsed()
            })
            .min()
            .unwrap()
    };
    let walk = fastest(&|| f.fenced_links());
    let cached = fastest(&|| c.fenced_links(&f));
    eprintln!("walk {walk:?}, cached {cached:?}");
    assert!(cached * 3 < walk, "walk {walk:?}, cached {cached:?}");
}

#[test]
fn a_directory_unlistable_at_the_first_ask_is_walked_once_it_can_be_listed() {
    let d = project();
    support::write(d.path(), "src/private/keep.txt", "");
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("src/private/keys")).unwrap();
    let c = cache(d.path());
    age_directories(d.path());
    let private = d.path().join("src/private");
    forbid_listing(&private).unwrap();
    if std::fs::read_dir(&private).is_ok() {
        // Windows, or running as root: the directory cannot be made unlistable here
        allow_listing(&private).unwrap();
        return;
    }
    let f = fences(d.path());
    let first = c.fenced_links(&f);
    allow_listing(&private).unwrap();
    assert!(first.is_empty(), "{first:?}");
    assert_eq!(c.fenced_links(&f), vec![PathBuf::from("src/private/keys")]);
}

#[test]
fn a_link_beside_an_ignore_file_that_ignores_everything_is_named() {
    let d = project();
    support::write(d.path(), "tmp/.gitignore", "*\n");
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("tmp/out")).unwrap();
    let c = cache(d.path());
    age_directories(d.path());
    let f = fences(d.path());
    assert_eq!(c.fenced_links(&f), vec![PathBuf::from("tmp/out")]);
    link_to(outside.path(), &d.path().join("tmp/out2")).unwrap();
    assert_eq!(
        sorted(c.fenced_links(&f)),
        vec![PathBuf::from("tmp/out"), PathBuf::from("tmp/out2")]
    );
}

/// A file system that does not change a directory's time when an entry is added (FAT on
/// Windows): the probe's entry is added, then the directory's time is put back.
fn add_entry_as_fat_does(dir: &Path) -> std::io::Result<()> {
    let before = std::fs::symlink_metadata(dir)?.modified()?;
    std::fs::write(dir.join("entry"), "")?;
    set_directory_modified(dir, before)
}

#[test]
fn a_file_system_that_keeps_directory_times_is_walked_every_time() {
    let d = project();
    let c = cache(d.path()).with_probe(add_entry_as_fat_does);
    age_directories(d.path());
    let f = fences(d.path());
    assert!(c.fenced_links(&f).is_empty());
    let outside = tempfile::TempDir::new().unwrap();
    link_to(outside.path(), &d.path().join("src/deep/out")).unwrap();
    age_directories(d.path());
    assert_eq!(c.fenced_links(&f), vec![PathBuf::from("src/deep/out")]);
}

#[test]
fn a_clock_file_that_is_a_link_is_not_written_through() {
    let d = project();
    let outside = tempfile::TempDir::new().unwrap();
    let target = outside.path().join("rc");
    std::fs::write(&target, "keep me\n").unwrap();
    let c = cache(d.path());
    let clock = d.path().join(".sorcerer/state/fence-walk.clock");
    link_to(&target, &clock).unwrap();
    let out = tempfile::TempDir::new().unwrap();
    link_to(out.path(), &d.path().join("src/out")).unwrap();
    assert_eq!(
        c.fenced_links(&fences(d.path())),
        vec![PathBuf::from("src/out")]
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
}

#[test]
fn a_probe_directory_that_is_a_link_is_not_worked_through() {
    let d = project();
    let c = cache(d.path());
    let victim = tempfile::TempDir::new().unwrap();
    std::fs::write(victim.path().join("entry"), "keep me\n").unwrap();
    let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    set_directory_modified(victim.path(), long_ago).unwrap();
    link_to(victim.path(), &d.path().join(".sorcerer/state/fence-probe")).unwrap();
    let out = tempfile::TempDir::new().unwrap();
    link_to(out.path(), &d.path().join("src/out")).unwrap();
    assert_eq!(
        c.fenced_links(&fences(d.path())),
        vec![PathBuf::from("src/out")]
    );
    assert_eq!(
        std::fs::read_to_string(victim.path().join("entry")).unwrap(),
        "keep me\n"
    );
    let time = std::fs::metadata(victim.path())
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(time, long_ago);
}

#[test]
fn a_state_directory_that_is_a_link_is_not_probed_through() {
    let d = project();
    let victim = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(d.path().join(".sorcerer")).unwrap();
    link_to(victim.path(), &d.path().join(".sorcerer/state")).unwrap();
    let c = cache(d.path());
    let out = tempfile::TempDir::new().unwrap();
    link_to(out.path(), &d.path().join("src/out")).unwrap();
    assert_eq!(
        c.fenced_links(&fences(d.path())),
        vec![PathBuf::from("src/out")]
    );
    assert!(!victim.path().join("fence-probe").exists());
}

#[test]
fn a_link_put_in_the_notes_after_the_first_ask_is_fenced_on_the_next() {
    let d = project();
    support::write(d.path(), ".sorcerer/notes/plan.md", "## Plan\n");
    let c = cache(d.path());
    age_directories(d.path());
    let f = fences(d.path());
    assert!(c.fenced_links(&f).is_empty());
    link_to(
        &d.path().join(".env"),
        &d.path().join(".sorcerer/notes/env.md"),
    )
    .unwrap();
    assert_eq!(
        c.fenced_links(&f),
        vec![PathBuf::from(".sorcerer/notes/env.md")]
    );
}
