mod support;

use std::time::{Duration, Instant};

use sorcerer_core::Cancel;
use sorcerer_models::Error;
use sorcerer_models::adapters::claude::Claude;
use sorcerer_models::domain::model::{Completion, Message, ModelId};
use sorcerer_models::ports::provider::Provider;

fn req(timeout: Duration) -> Completion {
    Completion {
        model: ModelId("sonnet".into()),
        system: "be brief".into(),
        messages: vec![Message::user("q1"), Message::user("q2")],
        tools: vec![],
        max_tokens: 100,
        timeout,
    }
}

fn claude(f: &support::FakeClaude, extra: &[(&str, &str)]) -> Claude {
    Claude::with_env(support::fake_claude(), f.envs(extra))
}

#[test]
fn passes_the_flags_and_the_prompt_on_stdin() {
    let f = support::FakeClaude::new();
    claude(&f, &[])
        .complete(req(Duration::from_secs(5)), &Cancel::default())
        .unwrap();
    let want: Vec<String> = [
        "-p",
        "--output-format",
        "json",
        "--model",
        "sonnet",
        "--permission-prompts",
        "none",
        "--system-prompt",
        "be brief",
        "--tools",
        "",
        "--no-session-persistence",
        "--setting-sources",
        "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(f.args(), want);
    assert_eq!(f.stdin(), "q1\n\nq2");
}

#[test]
fn claudecode_is_unset_and_claude_mds_are_off_for_the_child() {
    let f = support::FakeClaude::new();
    claude(&f, &[("CLAUDECODE", "1")])
        .complete(req(Duration::from_secs(5)), &Cancel::default())
        .unwrap();
    assert!(f.env().contains("CLAUDECODE=unset"), "{}", f.env());
    assert!(f.env().contains("MDS=1"), "{}", f.env());
}

#[test]
fn reads_result_cost_usage_and_seconds() {
    let f = support::FakeClaude::new();
    let r = claude(&f, &[])
        .complete(req(Duration::from_secs(5)), &Cancel::default())
        .unwrap();
    assert_eq!(
        (r.text.as_str(), r.cost_usd, r.seconds),
        ("ok", Some(0.01), 1.5)
    );
    assert_eq!((r.usage.input_tokens, r.usage.output_tokens), (3, 4));
}

#[test]
fn is_error_true_is_an_error_with_the_result_text() {
    let f = support::FakeClaude::new();
    let e = claude(
        &f,
        &[(
            "FAKE_CLAUDE_REPLY",
            r#"{"result":"credit balance too low","is_error":true}"#,
        )],
    )
    .complete(req(Duration::from_secs(5)), &Cancel::default())
    .unwrap_err();
    assert!(e.to_string().contains("credit balance too low"), "{e}");
}

#[test]
fn a_reply_that_is_not_json_is_an_error() {
    let f = support::FakeClaude::new();
    let e = claude(&f, &[("FAKE_CLAUDE_REPLY", "Error: not logged in")])
        .complete(req(Duration::from_secs(5)), &Cancel::default())
        .unwrap_err();
    assert!(e.to_string().contains("not logged in"), "{e}");
}

#[test]
fn a_timeout_kills_the_child_and_says_timeout() {
    let f = support::FakeClaude::new();
    let t = Instant::now();
    let e = claude(&f, &[("FAKE_CLAUDE_SLEEP", "5")])
        .complete(req(Duration::from_millis(300)), &Cancel::default())
        .unwrap_err();
    assert!(matches!(e, Error::Timeout { .. }), "{e}");
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[test]
fn cancel_kills_the_child() {
    let f = support::FakeClaude::new();
    let c = Cancel::default();
    let c2 = c.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        c2.cancel();
    });
    let t = Instant::now();
    let e = claude(&f, &[("FAKE_CLAUDE_SLEEP", "5")])
        .complete(req(Duration::from_secs(30)), &c)
        .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[test]
fn a_missing_runner_is_a_subprocess_error_naming_it() {
    let e = Claude::with_env("/no/such/claude".into(), vec![])
        .complete(req(Duration::from_secs(1)), &Cancel::default())
        .unwrap_err();
    assert!(matches!(e, Error::Subprocess { .. }), "{e}");
    assert!(e.to_string().contains("/no/such/claude"), "{e}");
}

#[test]
fn a_timeout_kills_the_childs_children_too() {
    let f = support::FakeClaude::new();
    let e = claude(&f, &[("FAKE_CLAUDE_SLEEP", "7.317")])
        .complete(req(Duration::from_millis(300)), &Cancel::default())
        .unwrap_err();
    assert!(matches!(e, Error::Timeout { .. }), "{e}");
    std::thread::sleep(Duration::from_millis(300));
    let left = std::process::Command::new("pgrep")
        .args(["-f", "sleep 7.317"])
        .output()
        .unwrap();
    assert!(
        left.stdout.is_empty(),
        "still running: {}",
        String::from_utf8_lossy(&left.stdout)
    );
}
