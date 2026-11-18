mod support;

use std::path::Path;

use sorcerer_core::Project;
use sorcerer_core::language::{GrammarSource, Languages};

fn open(d: &Path) -> Project {
    Project::open(d).unwrap()
}

#[test]
fn ships_python_and_rust() {
    let d = support::git_project();
    assert_eq!(open(d.path()).languages.names(), vec!["python", "rust"]);
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
        ".sorcerer/languages/go/language.toml",
        "files = [\"**/*.go\"]\ngrammar = { git = \"https://github.com/tree-sitter/tree-sitter-go\", rev = \"abc\" }\n",
    );
    let p = open(d.path());
    assert_eq!(p.languages.names(), vec!["go", "python", "rust"]);
    let go = p.languages.for_path(Path::new("cmd/main.go")).unwrap();
    assert_eq!(go.unit_separator, ".");
    assert!(matches!(&go.grammar, GrammarSource::Git { rev, .. } if rev == "abc"));
}

#[test]
fn a_local_grammar_path_is_relative_to_the_layer() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/go/language.toml",
        "files = [\"**/*.go\"]\ngrammar = { path = \"grammar\" }\n",
    );
    let p = open(d.path());
    let want = p.root.join(".sorcerer/languages/go/grammar");
    assert!(
        matches!(&p.languages.get("go").unwrap().grammar, GrammarSource::Local(g) if *g == want)
    );
}

#[test]
fn a_project_language_without_a_grammar_is_an_error_naming_it() {
    let d = support::git_project();
    support::write(
        d.path(),
        ".sorcerer/languages/go/language.toml",
        "files = [\"**/*.go\"]\n",
    );
    let e = Project::open(d.path()).unwrap_err().to_string();
    assert!(e.contains("go") && e.contains("grammar"), "{e}");
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
        vec!["python", "rust"]
    );
}
