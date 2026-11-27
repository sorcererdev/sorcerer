#![allow(clippy::too_many_lines)]
mod support;

use std::time::Duration;

use sorcerer_core::Cancel;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_models::adapters::claude_code::ClaudeCodeResearcher;
use sorcerer_models::domain::research::{Research, Tool};
use sorcerer_models::ports::researcher::Researcher;

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
    // the research's cap reaches `claude -p`, which stops itself at that spend
    assert_eq!(at("--max-budget-usd"), "0.25");
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
    let rows = JsonlLedger::for_project(&p)
        .unwrap()
        .read()
        .unwrap()
        .errands;
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
    let rows = JsonlLedger::for_project(&p)
        .unwrap()
        .read()
        .unwrap()
        .errands;
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].cost_usd, rows[0].ok), (Some(0.25), false));
    assert!(rows[0].note.as_deref().unwrap().contains("timed out"));
}

#[test]
fn deny_rules_cover_the_root_form_and_fenced_symlinks() {
    let d = support::git_project(None);
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(d.path().join("src")).unwrap();
    sorcerer_core::adapters::platform::paths::link_to(outside.path(), &d.path().join("src/link"))
        .unwrap();
    let f = support::FakeClaude::new();
    let (r, _) = researcher(d.path(), &f, &[]);
    r.research(research(Duration::from_secs(5)), &Cancel::default())
        .unwrap();
    let args = f.args();
    let settings: serde_json::Value =
        serde_json::from_str(&args[args.iter().position(|a| a == "--settings").unwrap() + 1])
            .unwrap();
    let deny: Vec<String> =
        serde_json::from_value(settings["permissions"]["deny"].clone()).unwrap();
    for rule in ["Read(./.env*)", "Read(./src/link)", "Read(./src/link/**)"] {
        assert!(deny.contains(&rule.to_string()), "{rule} not in {deny:?}");
    }
}

/// `claude -p` that stops at its `--max-budget-usd` prints no `result`, only `errors`
/// (measured 2026-11-27 with claude 2.x): the error says so, and the ledger charges what it spent.
#[test]
fn a_research_stopped_at_its_cap_says_so_and_is_charged_what_it_spent() {
    let d = support::git_project(None);
    let f = support::FakeClaude::new();
    let reply = r#"{"type":"result","subtype":"error_max_budget_usd","is_error":true,"total_cost_usd":0.27,"duration_ms":17000,"errors":["Reached maximum budget ($0.25)"],"usage":{"input_tokens":5,"output_tokens":900}}"#;
    let (r, p) = researcher(d.path(), &f, &[("FAKE_CLAUDE_REPLY", reply)]);
    let error = r
        .research(research(Duration::from_secs(5)), &Cancel::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Reached maximum budget ($0.25)"), "{error}");
    let rows = JsonlLedger::for_project(&p)
        .unwrap()
        .read()
        .unwrap()
        .errands;
    assert_eq!((rows[0].cost_usd, rows[0].ok), (Some(0.27), false));
    assert!(rows[0].note.as_deref().unwrap().contains("maximum budget"));
}

/// An error reply with neither a result nor errors still says something: its subtype.
#[test]
fn an_error_reply_with_no_text_names_its_subtype() {
    let d = support::git_project(None);
    let f = support::FakeClaude::new();
    let reply =
        r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":""}"#;
    let (r, _) = researcher(d.path(), &f, &[("FAKE_CLAUDE_REPLY", reply)]);
    let error = r
        .research(research(Duration::from_secs(5)), &Cancel::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("error_during_execution"), "{error}");
}
