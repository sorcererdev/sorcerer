mod support;

use std::os::unix::fs::symlink;
use std::path::Path;

use sorcerer_core::Project;
use sorcerer_core::fences::Fences;

fn fences(d: &Path) -> Fences {
    Fences::for_project(&Project::open(d).unwrap()).unwrap()
}

fn project() -> tempfile::TempDir {
    let d = support::git_project();
    support::write(d.path(), "src/a.py", "x = 1\n");
    support::write(d.path(), ".env", "SECRET=x\n");
    support::write(d.path(), "config/.env.local", "SECRET=y\n");
    support::write(d.path(), ".sorcerer/state/ledger.jsonl", "");
    d
}

#[test]
fn src_files_are_allowed() {
    let d = project();
    let f = fences(d.path());
    assert!(f.allows(Path::new("src/a.py")));
    assert!(f.allows(Path::new("src/not-yet-written.py")));
}

#[test]
fn env_files_anywhere_are_fenced() {
    let d = project();
    let f = fences(d.path());
    assert!(!f.allows(Path::new(".env")));
    assert!(!f.allows(Path::new("config/.env.local")));
    assert!(!f.allows(Path::new(".ENV")));
}

#[test]
fn the_sorcerer_and_git_dirs_are_fenced() {
    let d = project();
    let f = fences(d.path());
    assert!(!f.allows(Path::new(".sorcerer/state/ledger.jsonl")));
    assert!(!f.allows(Path::new(".git/config")));
}

#[test]
fn dot_dot_is_resolved_before_matching() {
    let d = project();
    let f = fences(d.path());
    assert!(!f.allows(Path::new("src/../.env")));
    assert!(!f.allows(Path::new("src/missing/../../.env")));
    assert!(!f.allows(Path::new("../outside.txt")));
}

#[test]
fn a_symlink_out_of_the_root_is_fenced() {
    let d = project();
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::write(outside.path().join("hosts"), "x").unwrap();
    symlink(outside.path(), d.path().join("src/link")).unwrap();
    let f = fences(d.path());
    assert!(!f.allows(Path::new("src/link/hosts")));
    assert!(!f.allows(Path::new("src/link")));
}

#[test]
fn a_symlink_inside_to_a_fenced_file_is_fenced() {
    let d = project();
    symlink(d.path().join(".env"), d.path().join("src/e")).unwrap();
    assert!(!fences(d.path()).allows(Path::new("src/e")));
}

#[test]
fn not_evidence_globs_are_fenced() {
    let d = project();
    support::write(
        d.path(),
        ".sorcerer/config.toml",
        "[model]\nnot_evidence = [\"docs/plans/**\"]\n",
    );
    support::write(d.path(), "docs/plans/p.md", "plan\n");
    let f = fences(d.path());
    assert!(!f.allows(Path::new("docs/plans/p.md")));
    assert!(f.patterns().contains(&"docs/plans/**".to_string()));
}

#[test]
fn an_absolute_path_is_judged_by_where_it_is() {
    let d = project();
    let f = fences(d.path());
    assert!(!f.allows(Path::new("/etc/hosts")));
    assert!(f.allows(&Project::open(d.path()).unwrap().root.join("src/a.py")));
}

#[test]
fn relative_gives_the_path_inside_the_root() {
    let d = project();
    let f = fences(d.path());
    assert_eq!(
        f.relative(Path::new("src/./a.py")).unwrap(),
        Path::new("src/a.py")
    );
    assert_eq!(f.relative(Path::new("../x")), None);
}

#[test]
fn a_dangling_symlink_is_fenced() {
    let d = project();
    symlink("/nonexistent/secret", d.path().join("src/dangling")).unwrap();
    assert!(!fences(d.path()).allows(Path::new("src/dangling")));
}

#[test]
fn a_directory_style_not_evidence_entry_fences_the_whole_directory() {
    let d = project();
    support::write(
        d.path(),
        ".sorcerer/config.toml",
        "[model]\nnot_evidence = [\"docs/plans\", \"notes/\"]\n",
    );
    support::write(d.path(), "docs/plans/p.md", "plan\n");
    support::write(d.path(), "notes/n.md", "n\n");
    let f = fences(d.path());
    assert!(!f.allows(Path::new("docs/plans/p.md")));
    assert!(!f.allows(Path::new("docs/plans")));
    assert!(!f.allows(Path::new("notes/n.md")));
    assert!(
        f.patterns().contains(&"docs/plans/**".to_string()),
        "{:?}",
        f.patterns()
    );
}

#[test]
fn fenced_links_lists_symlinks_out_of_the_root_or_onto_fenced_files() {
    let d = project();
    let outside = tempfile::TempDir::new().unwrap();
    symlink(outside.path(), d.path().join("src/out")).unwrap();
    symlink(d.path().join(".env"), d.path().join("src/e")).unwrap();
    symlink(d.path().join("src/a.py"), d.path().join("src/ok")).unwrap();
    let mut links = fences(d.path()).fenced_links();
    links.sort();
    assert_eq!(
        links,
        vec![
            std::path::PathBuf::from("src/e"),
            std::path::PathBuf::from("src/out")
        ]
    );
}
