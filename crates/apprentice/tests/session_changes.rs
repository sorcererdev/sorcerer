//! The session's changes from git: the files changed since the merge base with the default
//! branch, the region's file first, a new file as the diff that adds it, a fenced file never.
mod support;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use apprentice::adapters::git_session::GitSession;
use apprentice::ports::session_changes::SessionChanges;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::fences::Fences;
use support::{TempProject, test_cache, write};

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn session_of(temp: &TempProject) -> GitSession {
    let project = temp.project();
    GitSession {
        fences: Fences::for_project(&project).unwrap(),
        view: Arc::new(FilesView::new(Arc::new(project), test_cache())),
    }
}

#[test]
fn the_branchs_changes_come_with_the_regions_file_first_and_fenced_files_left_out() {
    let temp = TempProject::with(&[
        ("a.txt", "one\n"),
        ("b.txt", "two\n"),
        ("README.md", "# R\n"),
    ]);
    git(&temp.root, &["init", "-q", "-b", "main"]);
    git(&temp.root, &["add", "-A"]);
    git(&temp.root, &["commit", "-q", "-m", "base"]);
    git(&temp.root, &["checkout", "-q", "-b", "work"]);
    write(&temp.root.join("a.txt"), "one\nmore\n");
    git(&temp.root, &["commit", "-q", "-am", "on the branch"]);
    write(
        &temp.root.join("b.txt"),
        "two\nunsaved in git, saved on disk\n",
    );
    write(&temp.root.join("README.md"), "# R\n\nFill this in\n");
    write(&temp.root.join("new.txt"), "brand new\n");
    write(&temp.root.join(".env"), "SECRET=1\n");

    let diffs = session_of(&temp).diffs("README.md");

    let paths: Vec<&str> = diffs.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["README.md", "a.txt", "b.txt", "new.txt"]);
    assert!(
        diffs.files[1].diff.contains("+more"),
        "a commit on the branch counts"
    );
    assert!(
        diffs.files[3].diff.contains("+brand new"),
        "a new file reads as added"
    );
    let all: String = diffs.files.iter().map(|file| file.diff.as_str()).collect();
    assert!(!all.contains("SECRET"), "a fenced file never reaches Jev");
}

#[test]
fn a_project_with_no_default_branch_has_no_session() {
    let temp = TempProject::with(&[("a.txt", "one\n")]);
    assert!(session_of(&temp).diffs("a.txt").files.is_empty());
}
