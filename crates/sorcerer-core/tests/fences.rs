#![allow(clippy::too_many_lines)]
mod support;

use sorcerer_core::adapters::platform::paths::link_to;
use std::path::Path;

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;

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
    link_to(outside.path(), &d.path().join("src/link")).unwrap();
    let f = fences(d.path());
    assert!(!f.allows(Path::new("src/link/hosts")));
    assert!(!f.allows(Path::new("src/link")));
}

#[test]
fn a_symlink_inside_to_a_fenced_file_is_fenced() {
    let d = project();
    link_to(&d.path().join(".env"), &d.path().join("src/e")).unwrap();
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
    let gone = tempfile::TempDir::new().unwrap();
    link_to(&gone.path().join("secret"), &d.path().join("src/dangling")).unwrap();
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
    link_to(outside.path(), &d.path().join("src/out")).unwrap();
    link_to(&d.path().join(".env"), &d.path().join("src/e")).unwrap();
    link_to(&d.path().join("src/a.py"), &d.path().join("src/ok")).unwrap();
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

/// The project with a note, sorcerer's config and its state.
fn project_with_notes() -> tempfile::TempDir {
    let d = project();
    support::write(d.path(), ".sorcerer/notes/plan.md", "## Plan\n");
    support::write(d.path(), ".sorcerer/config.toml", "");
    d
}

#[test]
fn the_notes_are_the_one_place_in_the_sorcerer_dir_that_is_not_fenced() {
    let d = project_with_notes();
    let f = fences(d.path());
    assert!(f.allows(Path::new(".sorcerer/notes/plan.md")));
    assert!(f.allows(Path::new(".sorcerer/notes/new-one.md")));
    assert!(f.allows(Path::new(".sorcerer/notes")));
    for fenced in [
        ".sorcerer/config.toml",
        ".sorcerer/state/ledger.jsonl",
        ".sorcerer/notes2/x.md",
        ".sorcerer/state/notes/x.md",
        ".sorcerer/notes/../state/ledger.jsonl",
        ".sorcerer/notes/../config.toml",
        ".sorcerer/notes/missing/../../state/ledger.jsonl",
    ] {
        assert!(!f.allows(Path::new(fenced)), "{fenced} is fenced");
    }
}

#[test]
fn the_other_fences_still_hold_inside_the_notes() {
    let d = project_with_notes();
    support::write(
        d.path(),
        ".sorcerer/config.toml",
        "[model]\nnot_evidence = [\".sorcerer/notes/private\"]\n",
    );
    support::write(d.path(), ".sorcerer/notes/.env", "SECRET=z\n");
    support::write(d.path(), ".sorcerer/notes/private/p.md", "p\n");
    let f = fences(d.path());
    assert!(!f.allows(Path::new(".sorcerer/notes/.env")));
    assert!(!f.allows(Path::new(".sorcerer/notes/private/p.md")));
    assert!(f.allows(Path::new(".sorcerer/notes/plan.md")));
}

#[test]
fn a_link_in_the_notes_onto_a_fenced_place_stays_fenced_and_is_named() {
    let d = project_with_notes();
    link_to(
        &d.path().join(".sorcerer/state/ledger.jsonl"),
        &d.path().join(".sorcerer/notes/ledger.md"),
    )
    .unwrap();
    link_to(
        &d.path().join(".env"),
        &d.path().join(".sorcerer/notes/env.md"),
    )
    .unwrap();
    let f = fences(d.path());
    assert!(!f.allows(Path::new(".sorcerer/notes/ledger.md")));
    assert!(!f.allows(Path::new(".sorcerer/notes/env.md")));
    let mut links = f.fenced_links();
    links.sort();
    assert_eq!(
        links,
        vec![
            Path::new(".sorcerer/notes/env.md").to_path_buf(),
            Path::new(".sorcerer/notes/ledger.md").to_path_buf(),
        ]
    );
}

#[test]
fn a_notes_directory_that_is_a_link_into_the_state_or_out_is_fenced() {
    for target in ["state", "outside"] {
        let d = project();
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::write(outside.path().join("ledger.jsonl"), "x").unwrap();
        let to = match target {
            "state" => d.path().join(".sorcerer/state"),
            _ => outside.path().to_path_buf(),
        };
        link_to(&to, &d.path().join(".sorcerer/notes")).unwrap();
        let f = fences(d.path());
        assert!(
            !f.allows(Path::new(".sorcerer/notes/ledger.jsonl")),
            "{target}"
        );
        assert!(!f.allows(Path::new(".sorcerer/notes")), "{target}");
        assert_eq!(
            f.fenced_links(),
            vec![Path::new(".sorcerer/notes").to_path_buf()],
            "{target}"
        );
    }
}

#[test]
fn deny_patterns_name_each_entry_of_the_sorcerer_dir_but_the_notes() {
    let d = project_with_notes();
    support::write(d.path(), ".sorcerer/inspect.md", "x\n");
    let patterns = fences(d.path()).deny_patterns();
    for wanted in [
        "**/.env*",
        ".git/**",
        ".sorcerer/state",
        ".sorcerer/state/**",
        ".sorcerer/config.toml",
        ".sorcerer/inspect.md",
        ".sorcerer/spellbook/**",
        ".sorcerer/scry.toml",
    ] {
        assert!(
            patterns.contains(&wanted.to_string()),
            "{wanted}: {patterns:?}"
        );
    }
    assert!(
        !patterns
            .iter()
            .any(|p| p == ".sorcerer/**" || p.starts_with(".sorcerer/notes")),
        "{patterns:?}"
    );
}

#[test]
fn a_model_is_told_the_notes_are_the_exception() {
    let d = project();
    let described = fences(d.path()).described();
    assert!(
        described.contains(".sorcerer/** (except .sorcerer/notes/)"),
        "{described}"
    );
}
