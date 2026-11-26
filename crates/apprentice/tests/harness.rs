//! The `claude` adapter against a fake `claude` (`apprentice-fake-claude`), which really starts
//! `apprentice mcp` from `--mcp-config` and really runs `apprentice hook` from `--settings`: the
//! flags, the stdin framing, a tool call reaching the editor through the MCP server and the
//! loopback port, the person's edits reaching the agent through the hook. No model is called.
//!
//! The same contract against the real `claude` is the user's by-hand run (see the internals
//! book, "To confirm against the real claude").

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use apprentice::adapters::claude_agent::ClaudeAgent;
use apprentice::adapters::task_runner::TaskRunner;
use apprentice::domain::task::TaskState;
use apprentice::usecases::run_task::TaskOrder;
use serde_json::{Value, json};
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_core::domain::fences::Fences;
use support::{RecordingEditor, TempProject};
use tempfile::TempDir;

const BOOKING: &str = "def handle(self):\n    delete(booking)\n";

struct Fake {
    dir: TempDir,
}

impl Fake {
    fn new(script: Value) -> Fake {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("script.json"), script.to_string()).unwrap();
        Fake { dir }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn envs(&self) -> Vec<(String, String)> {
        ["SCRIPT:script.json", "ARGS:args", "STDIN:stdin", "LOG:log"]
            .iter()
            .map(|pair| {
                let (var, file) = pair.split_once(':').unwrap();
                (
                    format!("FAKE_APPRENTICE_{var}"),
                    self.file(file).to_string_lossy().into_owned(),
                )
            })
            .collect()
    }

    fn args(&self) -> Vec<String> {
        std::fs::read_to_string(self.file("args"))
            .unwrap()
            .lines()
            .map(String::from)
            .collect()
    }

    fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.file("log"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn agent(project: &TempProject, fake: &Fake) -> ClaudeAgent {
    let settings = ProviderConfig {
        runner: Some(PathBuf::from(env!("CARGO_BIN_EXE_apprentice-fake-claude"))),
        ..ProviderConfig::default()
    };
    let fences = Fences::new(project.root.clone(), &[]).unwrap();
    ClaudeAgent::new(
        &settings,
        PathBuf::from(env!("CARGO_BIN_EXE_apprentice")),
        fences,
    )
    .with_env(fake.envs())
}

fn order() -> TaskOrder {
    TaskOrder {
        instruction: "Refuse late cancellations.".into(),
        file: "src/cancel.txt".into(),
        text: BOOKING.into(),
        line: 1,
        unsaved: Vec::new(),
    }
}

#[test]
fn a_task_runs_on_the_fake_claude_through_the_real_mcp_server_and_hooks() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let fake = Fake::new(json!({
        "cost": 0.42,
        "session": "s-harness",
        "steps": [
            { "builtin": "Read", "input": { "file_path": "src/cancel.txt" } },
            { "tool": "write_file", "arguments": { "path": "src/cancel.txt", "text": "def handle(self):\n    refuse()\n" } },
            { "bash": "make test" },
            { "tool": "notify", "arguments": { "text": "done: late cancellations refused" } },
        ],
    }));
    let editor = Arc::new(RecordingEditor::at(&project.root).answering(&["Allow once"]));
    let agent = Arc::new(agent(&project, &fake));
    let runner = TaskRunner::open(project.deps(editor.clone(), agent)).unwrap();
    let record = runner.run(&order()).unwrap();

    assert_eq!(record.state, TaskState::Done, "{record:?}");
    assert_eq!(record.cost_usd, 0.42);
    assert_eq!(record.session_id.as_deref(), Some("s-harness"));
    assert_eq!(
        editor.buffer("src/cancel.txt").as_deref(),
        Some("def handle(self):\n    refuse()\n")
    );
    assert!(
        editor
            .shown()
            .contains(&"notify: apprentice: done: late cancellations refused".to_string())
    );
    let log = fake.log();
    let written = log
        .iter()
        .find(|entry| entry["tool"] == "write_file")
        .unwrap();
    assert_eq!(
        written["result"]["text"],
        "written: src/cancel.txt (in the editor, unsaved)"
    );
    let permission = log
        .iter()
        .find(|entry| entry.get("permission").is_some())
        .unwrap();
    let behavior: Value =
        serde_json::from_str(permission["permission"]["text"].as_str().unwrap()).unwrap();
    assert_eq!(behavior["behavior"], "allow");
    assert_eq!(
        editor.asked.lock().unwrap()[0].question,
        "apprentice wants to run: make test"
    );
    // a quiet editor: every hook after a tool call printed nothing
    let hooks: Vec<&Value> = log
        .iter()
        .filter(|entry| entry["hook"] == "PostToolUse")
        .collect();
    assert_eq!(hooks.len(), 4);
    assert!(hooks.iter().all(|hook| hook["output"] == ""), "{hooks:?}");
}

#[test]
fn the_flags_and_the_first_message_are_the_designs() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let fake = Fake::new(json!({ "steps": [] }));
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let runner = TaskRunner::open(project.deps(editor, Arc::new(agent(&project, &fake)))).unwrap();
    runner.run(&order()).unwrap();
    let args = fake.args();
    let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(args[0], "-p");
    assert_eq!(after("--output-format"), "stream-json");
    assert_eq!(after("--input-format"), "stream-json");
    assert!(args.contains(&"--verbose".to_string()));
    assert!(args.contains(&"--strict-mcp-config".to_string()));
    assert_eq!(after("--model"), "sonnet");
    assert_eq!(after("--max-budget-usd"), "2");
    assert_eq!(after("--max-turns"), "60");
    assert_eq!(after("--setting-sources"), "");
    assert_eq!(
        after("--permission-prompt-tool"),
        "mcp__apprentice__permission"
    );
    let settings: Value = serde_json::from_str(&after("--settings")).unwrap();
    let deny = settings["permissions"]["deny"].as_array().unwrap();
    for tool in [
        "Edit",
        "Write",
        "MultiEdit",
        "NotebookEdit",
        "AskUserQuestion",
    ] {
        assert!(deny.contains(&json!(tool)), "{tool}");
    }
    let stdin: Value =
        serde_json::from_str(&std::fs::read_to_string(fake.file("stdin")).unwrap()).unwrap();
    assert_eq!(stdin["type"], "user");
    assert_eq!(stdin["message"]["role"], "user");
    assert!(
        stdin["message"]["content"]
            .as_str()
            .unwrap()
            .contains("Refuse late cancellations.")
    );
}

#[test]
fn the_persons_edits_reach_the_fake_through_the_hook() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let fake = Fake::new(json!({
        "steps": [
            { "tool": "write_file", "arguments": { "path": "src/cancel.txt", "text": "class TooLate: pass\n" } },
            { "sleep_ms": 600 },
            { "builtin": "Read", "input": { "file_path": "src/cancel.txt" } },
        ],
    }));
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let runner =
        TaskRunner::open(project.deps(editor.clone(), Arc::new(agent(&project, &fake)))).unwrap();
    let started = runner.start(&order()).unwrap();
    let person = editor.clone();
    let typing = std::thread::spawn(move || {
        // after the write has landed, before the next tool call
        for _ in 0..200 {
            if person.buffer("src/cancel.txt").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        person.person_edits("src/cancel.txt", "class TooLateToCancel: pass\n");
    });
    runner.follow(started);
    typing.join().unwrap();
    let log = fake.log();
    // the edit reaches the agent after the next tool call, whichever that is
    let fed = log
        .iter()
        .filter(|entry| entry["hook"] == "PostToolUse")
        .find_map(|entry| entry["output"].as_str().filter(|output| !output.is_empty()))
        .unwrap_or_else(|| panic!("no hook output in {log:?}"));
    let output: Value = serde_json::from_str(fed).unwrap();
    assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let context = output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        context.contains("their edits are authoritative"),
        "{context}"
    );
    assert!(
        context.contains("+class TooLateToCancel: pass"),
        "{context}"
    );
}

#[test]
fn a_runner_that_is_not_there_fails_the_start() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let settings = ProviderConfig {
        runner: Some(project.root.join("no-such-claude")),
        ..ProviderConfig::default()
    };
    let agent = ClaudeAgent::new(
        &settings,
        PathBuf::from(env!("CARGO_BIN_EXE_apprentice")),
        Fences::new(project.root.clone(), &[]).unwrap(),
    );
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let runner = TaskRunner::open(project.deps(editor, Arc::new(agent))).unwrap();
    let refused = runner.run(&order()).unwrap_err();
    assert!(
        refused
            .text()
            .starts_with("apprentice: the task did not start:"),
        "{}",
        refused.text()
    );
}
