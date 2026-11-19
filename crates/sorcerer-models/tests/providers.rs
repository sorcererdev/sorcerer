mod support;

use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::domain::config::{DecideConfig, ModelConfig};
use sorcerer_core::domain::project::Project;
use sorcerer_models::adapters::providers::{
    default_decider, default_provider, default_provider_for, provider_named,
};

#[test]
fn claude_on_path_is_the_default() {
    let d = support::git_project(None);
    let (p, why) = default_provider_for(
        &Project::open(d.path()).unwrap(),
        &MapEnv::default().on_path("claude"),
    )
    .unwrap();
    assert_eq!((p.name(), why.name.as_str()), ("claude", "claude"));
}

#[test]
fn an_api_key_alone_gives_anthropic() {
    let d = support::git_project(None);
    let (p, _) = default_provider_for(
        &Project::open(d.path()).unwrap(),
        &MapEnv::default().var_set("ANTHROPIC_API_KEY", "k"),
    )
    .unwrap();
    assert_eq!(p.name(), "anthropic");
}

#[test]
fn a_named_openai_compatible_table_builds_one() {
    let d = support::git_project(Some(
        "[model]\nprovider = \"local\"\n[model.providers.local]\nkind = \"openai-compatible\"\nurl = \"http://localhost:11434/v1\"\n",
    ));
    let (p, why) =
        default_provider_for(&Project::open(d.path()).unwrap(), &MapEnv::default()).unwrap();
    assert_eq!((p.name(), why.why.as_str()), ("local", "set in [model]"));
}

#[test]
fn an_unknown_kind_lists_the_three() {
    let d = support::git_project(Some("[model.providers.x]\nkind = \"bedrock\"\n"));
    let Err(e) = provider_named(&Project::open(d.path()).unwrap(), "x", &MapEnv::default()) else {
        panic!()
    };
    let e = e.to_string();
    assert!(
        e.contains("bedrock") && e.contains("anthropic, claude, openai-compatible"),
        "{e}"
    );
}

#[test]
fn nothing_available_says_what_to_do() {
    let d = support::git_project(None);
    let Err(e) = default_provider_for(&Project::open(d.path()).unwrap(), &MapEnv::default()) else {
        panic!()
    };
    assert!(
        e.to_string()
            .contains("put claude on PATH or set ANTHROPIC_API_KEY"),
        "{e}"
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
