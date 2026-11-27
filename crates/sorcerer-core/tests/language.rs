#![allow(clippy::too_many_lines)]
mod support;

use std::path::Path;

use sorcerer_core::domain::language::{GrammarSource, Languages};
use sorcerer_core::domain::project::Project;

fn open(d: &Path) -> Project {
    Project::open(d).unwrap()
}

/// The languages under `crates/sorcerer-core/library/languages/`, in name order: what ships.
fn shipped_dirs() -> Vec<String> {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("library/languages");
    let mut names: Vec<String> = std::fs::read_dir(library)
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn ships_every_language_in_the_library() {
    let d = support::git_project();
    let project = open(d.path());
    let names = project.languages.names();
    assert_eq!(names, shipped_dirs());
    assert!(names.contains(&"python") && names.contains(&"rust"));
}

#[test]
fn a_path_finds_its_language() {
    let d = support::git_project();
    let p = open(d.path());
    let name = |s: &str| p.languages.for_path(Path::new(s)).map(|l| l.name.clone());
    assert_eq!(name("src/a.py").as_deref(), Some("python"));
    assert_eq!(name("src/lib.rs").as_deref(), Some("rust"));
    assert_eq!(name("a.py").as_deref(), Some("python"));
    assert_eq!(name("README.md"), None);
}

#[test]
fn a_project_key_replaces_the_shipped_one_and_doctor_can_say_so() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/python/language.toml",
        "files = [\"**/*.pyi\"]\n",
    );
    let p = open(d.path());
    let py = p.languages.get("python").unwrap();
    assert_eq!(py.files, vec!["**/*.pyi"]);
    assert_eq!(py.origins["files"], "project");
    assert_eq!(py.origins["grammar"], "shipped");
    assert!(matches!(py.grammar, GrammarSource::Git { .. }));
}

#[test]
fn a_project_query_file_replaces_by_name_and_adds_new_names() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/python/queries/tags.scm",
        "(function_definition) @x\n",
    );
    support::write(
        d.path(),
        ".sorcerer/languages/python/queries/extra.scm",
        "(class_definition) @y\n",
    );
    let p = open(d.path());
    let py = p.languages.get("python").unwrap();
    assert_eq!(py.queries["tags.scm"], "(function_definition) @x\n");
    assert_eq!(py.queries["extra.scm"], "(class_definition) @y\n");
    assert!(py.queries.contains_key("textobjects.scm"));
    assert_eq!(py.origins["queries/tags.scm"], "project");
    assert_eq!(py.origins["queries/textobjects.scm"], "shipped");
}

#[test]
fn a_project_language_with_files_and_grammar_is_a_new_language() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/kotlin/language.toml",
        "files = [\"**/*.kt\"]\ngrammar = { git = \"https://github.com/tree-sitter/tree-sitter-kotlin\", rev = \"abc\" }\n",
    );
    let p = open(d.path());
    assert!(p.languages.names().contains(&"kotlin"));
    let kotlin = p.languages.for_path(Path::new("app/Main.kt")).unwrap();
    assert_eq!(kotlin.unit_separator, ".");
    assert!(matches!(&kotlin.grammar, GrammarSource::Git { rev, .. } if rev == "abc"));
}

#[test]
fn a_local_grammar_path_is_relative_to_the_layer() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/kotlin/language.toml",
        "files = [\"**/*.kt\"]\ngrammar = { path = \"grammar\" }\n",
    );
    let p = open(d.path());
    let want = p.root.join(".sorcerer/languages/kotlin/grammar");
    assert!(
        matches!(&p.languages.get("kotlin").unwrap().grammar, GrammarSource::Local(g) if *g == want)
    );
}

#[test]
fn a_project_language_without_a_grammar_is_an_error_naming_it() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/kotlin/language.toml",
        "files = [\"**/*.kt\"]\n",
    );
    let e = Project::open(d.path()).unwrap_err().to_string();
    assert!(e.contains("kotlin") && e.contains("grammar"), "{e}");
}

#[test]
fn a_named_language_with_no_file_is_an_error() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/config.toml",
        "[languages]\nnames = [\"zig\"]\n",
    );
    let e = Project::open(d.path()).unwrap_err().to_string();
    assert!(e.contains("zig") && e.contains("no language file"), "{e}");
}

#[test]
fn unknown_tables_stay_in_rest() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/python/language.toml",
        "[test]\npreset = \"pytest\"\n",
    );
    let p = open(d.path());
    let py = p.languages.get("python").unwrap();
    assert_eq!(py.rest["test"]["preset"].as_str(), Some("pytest"));
    assert!(!py.rest.contains_key("files"));
}

#[test]
fn assemble_without_a_project_dir_is_the_shipped_set() {
    let d = tempfile::TempDir::new().unwrap();
    assert_eq!(
        Languages::assemble(d.path(), &[]).unwrap().names(),
        shipped_dirs()
    );
}

#[test]
fn a_project_layer_that_names_a_grammar_makes_it_the_projects() {
    use sorcerer_core::domain::language::{GrammarOrigin, Languages};
    let d = tempfile::TempDir::new().unwrap();
    let shipped = Languages::assemble(d.path(), &[]).unwrap();
    assert_eq!(
        shipped.get("python").unwrap().grammar_origin(),
        GrammarOrigin::Shipped
    );

    // only adding files keeps the shipped grammar
    let files_only = d.path().join(".sorcerer/languages/python");
    std::fs::create_dir_all(&files_only).unwrap();
    std::fs::write(files_only.join("language.toml"), "files = [\"*.pyi\"]\n").unwrap();
    let ls = Languages::assemble(d.path(), &[]).unwrap();
    assert_eq!(
        ls.get("python").unwrap().grammar_origin(),
        GrammarOrigin::Shipped
    );

    // naming a grammar, even the shipped one, makes it the project's
    std::fs::write(
        files_only.join("language.toml"),
        "grammar = { path = \"my-grammar\" }\n",
    )
    .unwrap();
    let ls = Languages::assemble(d.path(), &[]).unwrap();
    assert_eq!(
        ls.get("python").unwrap().grammar_origin(),
        GrammarOrigin::Project
    );
}

#[test]
fn a_grammar_may_name_its_folder_in_the_repository() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/kotlin/language.toml",
        "files = [\"**/*.kt\"]\ngrammar = { git = \"https://example.com/g\", rev = \"abcdef1\", dir = \"kotlin\" }\n",
    );
    let p = open(d.path());
    assert!(matches!(
        &p.languages.get("kotlin").unwrap().grammar,
        GrammarSource::Git { dir: Some(dir), .. } if dir == "kotlin"
    ));
}

#[test]
fn a_grammar_folder_that_climbs_out_is_refused() {
    for dir in ["../up", "/abs", ""] {
        let d = support::git_project();
        support::write(
            d.path(),
            ".sorcerer/languages/kotlin/language.toml",
            &format!(
                "files = [\"**/*.kt\"]\ngrammar = {{ git = \"https://example.com/g\", rev = \"abcdef1\", dir = \"{dir}\" }}\n"
            ),
        );
        let e = Project::open(d.path()).unwrap_err().to_string();
        assert!(e.contains("optional dir"), "{dir}: {e}");
    }
}

#[test]
fn a_closed_comment_has_an_end_mark() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/page/language.toml",
        "files = [\"**/*.page\"]\ngrammar = { path = \"g\" }\ncomment = \"<!--\"\ncomment_end = \"-->\"\n",
    );
    let p = open(d.path());
    let marks = p.languages.get("page").unwrap().comment_marks().unwrap();
    assert_eq!(marks.around("note"), "<!-- note -->");
    assert_eq!(
        marks.inside("  <!-- spellbook:end x -->"),
        Some("spellbook:end x")
    );
    assert_eq!(marks.inside("<!-- open"), None);
    assert_eq!(marks.without_end("a — why -->"), "a — why");
    let python = p.languages.get("python").unwrap().comment_marks().unwrap();
    assert_eq!(python.around("note"), "# note");
    assert_eq!(python.inside("    # note"), Some("note"));
    assert!(
        !p.languages
            .get("python")
            .unwrap()
            .rest
            .contains_key("comment_end")
    );
}

#[test]
fn a_language_without_comment_has_no_comment_marks() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/data/language.toml",
        "files = [\"**/*.data\"]\ngrammar = { path = \"g\" }\n",
    );
    assert_eq!(
        open(d.path())
            .languages
            .get("data")
            .unwrap()
            .comment_marks(),
        None
    );
}

#[test]
fn a_language_has_units_when_its_queries_capture_one() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/data/language.toml",
        "files = [\"**/*.data\"]\ngrammar = { path = \"g\" }\n",
    );
    support::write(
        d.path(),
        ".sorcerer/languages/data/queries/textobjects.scm",
        "; (pair) @definition.function is not a capture: the line is a comment\n(comment) @comment.outer\n",
    );
    support::write(
        d.path(),
        ".sorcerer/languages/sections/language.toml",
        "files = [\"**/*.sections\"]\ngrammar = { path = \"g\" }\n",
    );
    support::write(
        d.path(),
        ".sorcerer/languages/sections/queries/units.scm",
        "(section (title) @unit.name) @unit.section\n",
    );
    let p = open(d.path());
    assert!(!p.languages.get("data").unwrap().has_unit_captures());
    assert!(p.languages.get("sections").unwrap().has_unit_captures());
    assert!(p.languages.get("python").unwrap().has_unit_captures());
    assert!(
        p.languages
            .with_units_for_path(Path::new("a.data"))
            .is_none()
    );
    assert!(p.languages.for_path(Path::new("a.data")).is_some());
    assert!(p.languages.with_units_for_path(Path::new("a.py")).is_some());
}
