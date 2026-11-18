mod support;

use sorcerer_core::Project;
use sorcerer_core::env::MapEnv;
use sorcerer_models::providers::{default_provider_for, provider_named};

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
