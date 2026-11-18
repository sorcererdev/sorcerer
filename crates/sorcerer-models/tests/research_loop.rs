mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use sorcerer_core::fences::Fences;
use sorcerer_core::grammar::GrammarCache;
use sorcerer_core::ledger::Ledger;
use sorcerer_core::view::FilesView;
use sorcerer_core::{Cancel, Project};
use sorcerer_models::provider::{Content, Reply, ScriptedProvider, StopReason, ToolCall, Usage};
use sorcerer_models::research::tool_loop::LoopResearcher;
use sorcerer_models::research::{Research, Researcher, Tool};

fn tool(name: &str, input: serde_json::Value, cost: f64) -> Reply {
    Reply {
        text: String::new(),
        tool_calls: vec![ToolCall {
            id: format!("c-{name}"),
            name: name.into(),
            input,
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
        },
        cost_usd: Some(cost),
        stop: StopReason::ToolUse,
        seconds: 0.5,
    }
}

fn text(t: &str) -> Reply {
    Reply {
        text: t.into(),
        tool_calls: vec![],
        usage: Usage {
            input_tokens: 20,
            output_tokens: 8,
        },
        cost_usd: Some(0.01),
        stop: StopReason::End,
        seconds: 0.5,
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    provider: Arc<ScriptedProvider>,
    researcher: LoopResearcher,
    project: Arc<Project>,
}

fn setup(replies: Vec<Reply>) -> Setup {
    let d = support::git_project(Some("[model]\nnot_evidence = [\"docs/plans/**\"]\n"));
    support::write(d.path(), "docs/plans/p.md", "return 1 SECRET=hunter4\n");
    support::write(d.path(), "src/a.py", "def f():\n    return 1\n");
    support::write(d.path(), ".env", "SECRET=hunter2\n");
    support::write(d.path(), "config/.env.local", "SECRET=hunter3 return 1\n");
    let project = Arc::new(Project::open(d.path()).unwrap());
    let provider = Arc::new(ScriptedProvider::new(replies));
    let view = Arc::new(FilesView::new(
        project.clone(),
        GrammarCache::at(d.path().join("cache")),
    ));
    let researcher = LoopResearcher::new(
        provider.clone(),
        view,
        Fences::for_project(&project).unwrap(),
        Ledger::for_project(&project).unwrap(),
    );
    Setup {
        _dir: d,
        provider,
        researcher,
        project,
    }
}

fn research(cap: f64) -> Research {
    Research {
        product: "scry".into(),
        verb: "find".into(),
        prompt: "look".into(),
        model: "sonnet".into(),
        tools: vec![Tool::Read, Tool::Grep, Tool::Glob],
        cap_usd: cap,
        timeout: Duration::from_secs(10),
    }
}

/// What the model was sent as tool results, in order.
fn results(s: &Setup) -> Vec<String> {
    let seen = s.provider.seen.lock().unwrap();
    let last = seen.last().unwrap();
    last.messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|c| {
            if let Content::ToolResult { text, .. } = c {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect()
}

fn everything_sent(s: &Setup) -> String {
    format!("{:?}", s.provider.seen.lock().unwrap())
}

#[test]
fn a_read_tool_call_is_answered_and_the_loop_ends_on_text() {
    let s = setup(vec![
        tool("read", json!({"path": "src/a.py"}), 0.01),
        text("f returns 1"),
    ]);
    let out = s
        .researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    assert_eq!(out.answer, "f returns 1");
    assert_eq!(
        results(&s),
        vec!["1\tdef f():\n2\t    return 1".to_string()]
    );
    assert_eq!(out.reads, vec![PathBuf::from("src/a.py")]);
    assert!(out.errand.ok);
}

#[test]
fn a_read_takes_a_line_range() {
    let s = setup(vec![
        tool(
            "read",
            json!({"path": "src/a.py", "start_line": 2, "end_line": 2}),
            0.01,
        ),
        text("ok"),
    ]);
    s.researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    assert_eq!(results(&s), vec!["2\t    return 1".to_string()]);
}

#[test]
fn a_fenced_read_is_refused_and_the_secret_never_sent() {
    let s = setup(vec![
        tool("read", json!({"path": ".env"}), 0.01),
        text("ok"),
    ]);
    let out = s
        .researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    assert_eq!(
        results(&s),
        vec!["refused: .env is fenced (not evidence)".to_string()]
    );
    assert!(!everything_sent(&s).contains("hunter2"));
    assert!(out.reads.is_empty());
}

#[test]
fn dot_dot_to_env_is_refused() {
    let s = setup(vec![
        tool("read", json!({"path": "src/../.env"}), 0.01),
        text("ok"),
    ]);
    s.researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    assert!(results(&s)[0].starts_with("refused:"));
    assert!(!everything_sent(&s).contains("hunter2"));
}

#[test]
fn an_absolute_path_outside_is_refused() {
    let s = setup(vec![
        tool("read", json!({"path": "/etc/hosts"}), 0.01),
        text("ok"),
    ]);
    s.researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    assert!(results(&s)[0].starts_with("refused:"));
}

#[test]
fn grep_drops_fenced_hits_and_says_how_many() {
    let s = setup(vec![
        tool("grep", json!({"pattern": "return 1|SECRET"}), 0.01),
        text("ok"),
    ]);
    s.researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    let r = &results(&s)[0];
    assert!(r.contains("src/a.py:2:     return 1"), "{r}");
    assert!(r.contains("(1 hit in fenced paths not shown)"), "{r}");
    assert!(!everything_sent(&s).contains("hunter"), "{r}");
}

#[test]
fn glob_lists_allowed_files_only() {
    let s = setup(vec![
        tool("glob", json!({"pattern": "**/*"}), 0.01),
        text("ok"),
    ]);
    s.researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    let r = &results(&s)[0];
    assert!(r.contains("src/a.py"), "{r}");
    assert!(!r.contains(".env"), "{r}");
    assert!(!r.contains(".sorcerer"), "{r}");
    assert!(!r.contains("docs/plans"), "{r}");
}

#[test]
fn the_turn_cap_stops_the_loop() {
    let s = setup(
        (0..13)
            .map(|_| tool("glob", json!({"pattern": "src/*"}), 0.0))
            .collect(),
    );
    let out = s
        .researcher
        .research(research(10.0), &Cancel::default())
        .unwrap();
    assert_eq!(s.provider.seen.lock().unwrap().len(), 12);
    assert!(!out.errand.ok);
    assert!(out.errand.note.unwrap().contains("turn cap"));
}

#[test]
fn the_spend_cap_stops_the_loop() {
    let s = setup(
        (0..10)
            .map(|_| tool("glob", json!({"pattern": "src/*"}), 0.02))
            .collect(),
    );
    let out = s
        .researcher
        .research(research(0.05), &Cancel::default())
        .unwrap();
    assert_eq!(s.provider.seen.lock().unwrap().len(), 3);
    assert!(out.errand.note.unwrap().contains("spend cap"));
}

#[test]
fn one_errand_sums_the_turns_and_is_in_the_ledger() {
    let s = setup(vec![
        tool("read", json!({"path": "src/a.py"}), 0.01),
        text("done"),
    ]);
    let out = s
        .researcher
        .research(research(1.0), &Cancel::default())
        .unwrap();
    assert_eq!(
        (out.errand.input_tokens, out.errand.output_tokens),
        (30, 13)
    );
    assert!((out.errand.cost_usd.unwrap() - 0.02).abs() < 1e-12);
    assert_eq!(
        (out.errand.provider.as_str(), out.errand.product.as_str()),
        ("scripted", "scry")
    );
    let rows = Ledger::for_project(&s.project)
        .unwrap()
        .read()
        .unwrap()
        .errands;
    assert_eq!(rows, vec![out.errand]);
}

#[test]
fn a_provider_failure_is_recorded_and_returned() {
    let s = setup(vec![]);
    assert!(
        s.researcher
            .research(research(1.0), &Cancel::default())
            .is_err()
    );
    let rows = Ledger::for_project(&s.project)
        .unwrap()
        .read()
        .unwrap()
        .errands;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].ok);
}
