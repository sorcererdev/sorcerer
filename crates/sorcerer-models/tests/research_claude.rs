mod support;

use std::time::Duration;

use sorcerer_core::config::ProviderConfig;
use sorcerer_core::ledger::Ledger;
use sorcerer_core::{Cancel, Project};
use sorcerer_models::research::claude_code::ClaudeCodeResearcher;
use sorcerer_models::research::{Research, Researcher, Tool};

fn research(timeout: Duration) -> Research {
    Research {
        product: "scry".into(),
        verb: "find".into(),
        prompt: "find bugs".into(),
        model: "sonnet".into(),
        tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
        cap_usd: 0.25,
        timeout,
    }
}

fn researcher(
    d: &std::path::Path,
    f: &support::FakeClaude,
    extra: &[(&str, &str)],
) -> (ClaudeCodeResearcher, Project) {
    let p = Project::open(d).unwrap();
    let cfg = ProviderConfig {
        runner: Some(support::fake_claude()),
        ..Default::default()
    };
    (
        ClaudeCodeResearcher::new(&p, &cfg)
            .unwrap()
            .with_env(f.envs(extra)),
        p,
    )
}

#[test]
fn claude_code_researcher_passes_fences_as_deny_rules() {
    let d = support::git_project(Some("[model]\nnot_evidence = [\"docs/plans/**\"]\n"));
    let f = support::FakeClaude::new();
    let (r, p) = researcher(d.path(), &f, &[]);
    let out = r
        .research(research(Duration::from_secs(5)), &Cancel::default())
        .unwrap();
    assert_eq!(out.answer, "ok");
    let args = f.args();
    let at = |flag: &str| {
        args[args
            .iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("no {flag} in {args:?}"))
            + 1]
        .clone()
    };
    assert_eq!(at("--tools"), "Read,Grep,Glob");
    assert_eq!(at("--allowedTools"), "Read,Grep,Glob");
    assert_eq!(at("--model"), "sonnet");
    assert!(args.contains(&"--strict-mcp-config".to_string()));
    let settings: serde_json::Value = serde_json::from_str(&at("--settings")).unwrap();
    let deny: Vec<String> =
        serde_json::from_value(settings["permissions"]["deny"].clone()).unwrap();
    for rule in [
        "Read(./**/.env*)",
        "Read(./.sorcerer/**)",
        "Read(./.git/**)",
        "Read(./docs/plans/**)",
        "WebSearch",
        "WebFetch",
    ] {
        assert!(deny.contains(&rule.to_string()), "{rule} not in {deny:?}");
    }
    assert!(at("--append-system-prompt").contains("docs/plans/**"));
    assert!(
        f.env().contains(&format!("PWD={}", p.root.display())),
        "{}",
        f.env()
    );
    assert_eq!(f.stdin(), "find bugs");
}

#[test]
fn its_errand_is_in_the_ledger() {
    let d = support::git_project(None);
    let f = support::FakeClaude::new();
    let (r, p) = researcher(d.path(), &f, &[]);
    let out = r
        .research(research(Duration::from_secs(5)), &Cancel::default())
        .unwrap();
    let rows = Ledger::for_project(&p).unwrap().read().unwrap().errands;
    assert_eq!(rows, vec![out.errand.clone()]);
    let e = &rows[0];
    assert_eq!(
        (
            e.product.as_str(),
            e.verb.as_str(),
            e.provider.as_str(),
            e.model.as_str()
        ),
        ("scry", "find", "claude", "sonnet")
    );
    assert_eq!((e.cost_usd, e.ok), (Some(0.01), true));
}

#[test]
fn a_timeout_is_charged_the_cap_and_not_ok() {
    let d = support::git_project(None);
    let f = support::FakeClaude::new();
    let (r, p) = researcher(d.path(), &f, &[("FAKE_CLAUDE_SLEEP", "5")]);
    assert!(
        r.research(research(Duration::from_millis(300)), &Cancel::default())
            .is_err()
    );
    let rows = Ledger::for_project(&p).unwrap().read().unwrap().errands;
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].cost_usd, rows[0].ok), (Some(0.25), false));
    assert!(rows[0].note.as_deref().unwrap().contains("timed out"));
}
