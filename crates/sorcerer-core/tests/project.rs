#![allow(clippy::too_many_lines)]
mod support;

use sorcerer_core::domain::project::Project;

#[test]
fn opens_the_git_root_with_defaults_when_there_is_no_sorcerer_dir() {
    let d = support::git_project();
    std::fs::create_dir_all(d.path().join("src/a")).unwrap();
    let p = Project::open(&d.path().join("src/a")).unwrap();
    assert_eq!(p.root, d.path().canonicalize().unwrap());
    assert!(p.config.model.provider.is_none());
    assert_eq!(p.state, p.root.join(".sorcerer/state"));
}

#[test]
fn a_sorcerer_dir_is_the_root_and_its_config_is_read() {
    let d = support::git_project();
    support::write(
        d.path(),
        "sub/.sorcerer/config.toml",
        "[model]\nprovider = \"anthropic\"\n",
    );
    std::fs::create_dir_all(d.path().join("sub/src")).unwrap();
    let p = Project::open(&d.path().join("sub/src")).unwrap();
    assert_eq!(p.root, d.path().join("sub").canonicalize().unwrap());
    assert_eq!(p.config.model.provider.as_deref(), Some("anthropic"));
}

#[test]
fn no_sorcerer_dir_and_no_git_is_one_sentence() {
    let d = tempfile::TempDir::new().unwrap();
    let e = Project::open(d.path()).unwrap_err();
    assert!(
        e.to_string()
            .contains("no .sorcerer/ directory and no git repository"),
        "{e}"
    );
}

#[test]
fn the_state_dir_is_created_once_with_a_gitignore() {
    let d = support::git_project();
    let p = Project::open(d.path()).unwrap();
    let s = p.state_dir().unwrap();
    assert_eq!(
        std::fs::read_to_string(s.join(".gitignore")).unwrap(),
        "*\n"
    );
    std::fs::write(s.join(".gitignore"), "*\nkept\n").unwrap();
    p.state_dir().unwrap();
    assert_eq!(
        std::fs::read_to_string(s.join(".gitignore")).unwrap(),
        "*\nkept\n"
    );
}

#[test]
fn a_store_in_the_project_writes_the_state_gitignore() {
    #[derive(serde::Serialize, serde::Deserialize, Default)]
    struct S {
        n: u32,
    }
    impl sorcerer_core::adapters::json_store::Versioned for S {
        const VERSION: u32 = 1;
    }
    let d = support::git_project();
    let p = Project::open(d.path()).unwrap();
    let s: sorcerer_core::adapters::json_store::Store<S> =
        sorcerer_core::adapters::json_store::Store::in_project(&p, "scry/findings.json").unwrap();
    s.update(|x| x.n = 1).unwrap();
    assert_eq!(
        std::fs::read_to_string(p.state.join(".gitignore")).unwrap(),
        "*\n"
    );
    assert!(p.state.join("scry/findings.json").exists());
}
