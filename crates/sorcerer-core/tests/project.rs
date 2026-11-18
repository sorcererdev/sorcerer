mod support;

use sorcerer_core::config::{DecideConfig, ModelConfig};
use sorcerer_core::env::MapEnv;
use sorcerer_core::project::{Project, default_decider, default_provider};

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
fn the_default_provider_prefers_claude_on_path() {
    let m = ModelConfig::default();
    let both = MapEnv::default()
        .on_path("claude")
        .var_set("ANTHROPIC_API_KEY", "k");
    assert_eq!(default_provider(&m, &both).unwrap().name, "claude");
    let key = MapEnv::default().var_set("ANTHROPIC_API_KEY", "k");
    let c = default_provider(&m, &key).unwrap();
    assert_eq!(
        (c.name.as_str(), c.why.as_str()),
        ("anthropic", "ANTHROPIC_API_KEY is set")
    );
    assert!(default_provider(&m, &MapEnv::default()).is_none());
    let set = ModelConfig {
        provider: Some("local".into()),
        ..Default::default()
    };
    assert_eq!(default_provider(&set, &both).unwrap().why, "set in [model]");
}

#[test]
fn jev_is_the_decider_when_its_key_is_set() {
    let d = DecideConfig::default();
    assert_eq!(
        default_decider(&d, &MapEnv::default().var_set("JEV_API_KEY", "k"))
            .unwrap()
            .name,
        "jev"
    );
    assert!(default_decider(&d, &MapEnv::default()).is_none());
    let own = DecideConfig {
        key_env: Some("MY_JEV".into()),
        ..Default::default()
    };
    assert!(default_decider(&own, &MapEnv::default().var_set("MY_JEV", "k")).is_some());
}
