//! A task end to end over the use cases: a scripted agent that calls back through the real
//! loopback port, an editor that records. No process is started and no model is called.

mod support;

use std::sync::Arc;

use apprentice::adapters::task_runner::TaskRunner;
use apprentice::domain::agent_event::{AgentEvent, Finished};
use apprentice::domain::task::TaskState;
use apprentice::domain::token_cost::TokenUsage;
use apprentice::usecases::run_task::{TaskOrder, TaskRefused};
use serde_json::{Value, json};
use sorcerer_core::domain::errand::Errand;
use support::{RecordingEditor, ScriptedAgent, Step, TempProject, tool_use};

const BOOKING: &str = "def handle(self):\n    delete(booking)\n";

fn order() -> TaskOrder {
    TaskOrder {
        instruction: "Refuse late cancellations.".into(),
        file: "src/cancel.txt".into(),
        text: BOOKING.into(),
        line: 1,
        unsaved: Vec::new(),
    }
}

struct Run {
    project: TempProject,
    editor: Arc<RecordingEditor>,
    agent: Arc<ScriptedAgent>,
    record: apprentice::domain::task::TaskRecord,
}

fn run(script: Vec<Step>, answers: &[&str]) -> Run {
    run_with(
        TempProject::with(&[("src/cancel.txt", BOOKING)]),
        script,
        answers,
        |_| {},
    )
}

fn run_with(
    project: TempProject,
    script: Vec<Step>,
    answers: &[&str],
    meanwhile: impl Fn(&RecordingEditor),
) -> Run {
    let editor = Arc::new(RecordingEditor::at(&project.root).answering(answers));
    meanwhile(&editor);
    let agent = Arc::new(ScriptedAgent::playing(script));
    let runner = TaskRunner::open(project.deps(editor.clone(), agent.clone())).unwrap();
    let record = runner.run(&order()).unwrap();
    Run {
        project,
        editor,
        agent,
        record,
    }
}

fn replies_text(agent: &ScriptedAgent) -> Vec<String> {
    agent
        .replies()
        .into_iter()
        .map(|reply| reply.text)
        .collect()
}

#[test]
fn a_task_that_writes_a_file_lands_it_in_the_editor_and_finishes_done() {
    let run = run(
        vec![
            tool_use("Read", json!({ "file_path": "src/cancel.txt" })),
            Step::Tool(
                "write_file",
                json!({ "path": "src/cancel.txt", "text": "def handle(self):\n    refuse()\n" }),
            ),
            Step::Hook,
            Step::Tool("notify", json!({ "text": "refused late cancellations" })),
            Step::Finish(0.71),
        ],
        &[],
    );
    assert_eq!(run.record.state, TaskState::Done);
    assert_eq!(run.record.cost_usd, 0.71);
    assert_eq!(run.record.files.len(), 1);
    assert_eq!(run.record.files[0].path, "src/cancel.txt");
    assert!(!run.record.files[0].new);
    assert_eq!(
        run.editor.buffer("src/cancel.txt").as_deref(),
        Some("def handle(self):\n    refuse()\n")
    );
    // the disk is the person's to save
    assert_eq!(run.project.read("src/cancel.txt"), BOOKING);
    assert_eq!(
        replies_text(&run.agent),
        [
            "written: src/cancel.txt (in the editor, unsaved)",
            "",
            "shown"
        ]
    );
    assert!(
        run.editor
            .shown()
            .contains(&"notify: apprentice: refused late cancellations".to_string())
    );
    let progress = run.editor.progress.lock().unwrap().clone();
    assert!(
        progress
            .iter()
            .any(|line| line.contains("reading cancel.txt")),
        "{progress:?}"
    );
    let (record, done) = run.editor.finished.lock().unwrap()[0].clone();
    assert_eq!(record.id, run.record.id);
    assert!(
        done.starts_with("apprentice: done · refuse late cancellations · 1 file · $0.71"),
        "{done}"
    );
    // the before-text is kept for undo, the ledger has the task, the record is the last line
    let journal = run
        .project
        .state(&format!("tasks/{}/journal.md", run.record.id));
    assert!(
        journal.contains("## write_file\n\nsrc/cancel.txt"),
        "{journal}"
    );
    assert_eq!(
        run.project
            .state(&format!("tasks/{}/before/src/cancel.txt", run.record.id)),
        BOOKING
    );
    let ledger =
        std::fs::read_to_string(run.project.root.join(".sorcerer/state/ledger.jsonl")).unwrap();
    let errand: Errand = serde_json::from_str(ledger.lines().last().unwrap()).unwrap();
    assert_eq!(
        (errand.product.as_str(), errand.verb.as_str()),
        ("apprentice", "task")
    );
    assert!(
        run.project
            .state("tasks.jsonl")
            .lines()
            .last()
            .unwrap()
            .contains(r#""state":"done""#)
    );
}

#[test]
fn the_persons_edits_reach_the_agent_and_a_write_over_them_is_refused() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Tool(
            "write_file",
            json!({ "path": "src/cancel.txt", "text": "class TooLate: pass\n" }),
        ),
        Step::Sleep(300),
        Step::Hook,
        Step::Tool(
            "write_file",
            json!({ "path": "src/cancel.txt", "text": "class TooLate: pass\nx = 1\n" }),
        ),
        Step::Finish(0.1),
    ]));
    let runner = TaskRunner::open(project.deps(editor.clone(), agent.clone())).unwrap();
    let started = runner.start(&order()).unwrap();
    let person = editor.clone();
    let typing = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        person.person_edits("src/cancel.txt", "class TooLateToCancel: pass\n");
    });
    let record = runner.follow(started);
    typing.join().unwrap();
    let replies = agent.replies();
    assert!(
        replies[1]
            .text
            .starts_with("The person changed these files"),
        "{:?}",
        replies[1]
    );
    assert!(
        replies[1].text.contains("+class TooLateToCancel: pass"),
        "{:?}",
        replies[1]
    );
    // the look moved with the hook: the second write builds on the person's text, so it lands
    assert_eq!(
        replies[2].text,
        "written: src/cancel.txt (in the editor, unsaved)"
    );
    assert_eq!(record.state, TaskState::Done);
    let journal = project.state(&format!("tasks/{}/journal.md", record.id));
    assert!(journal.contains("## edits fed"), "{journal}");
}

#[test]
fn a_write_the_person_moved_on_is_refused_with_the_text_now() {
    let run = run_with(
        TempProject::with(&[("src/cancel.txt", BOOKING)]),
        vec![
            Step::Tool(
                "write_file",
                json!({ "path": "src/cancel.txt", "text": "one\n" }),
            ),
            Step::Sleep(200),
            Step::Tool(
                "write_file",
                json!({ "path": "src/cancel.txt", "text": "two\n" }),
            ),
            Step::Finish(0.1),
        ],
        &[],
        |_| {},
    );
    // nothing moved: both land
    assert_eq!(replies_text(&run.agent).len(), 2);
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Tool(
            "write_file",
            json!({ "path": "src/cancel.txt", "text": "one\n" }),
        ),
        Step::Sleep(300),
        Step::Tool(
            "write_file",
            json!({ "path": "src/cancel.txt", "text": "two\n" }),
        ),
        Step::Finish(0.1),
    ]));
    let runner = TaskRunner::open(project.deps(editor.clone(), agent.clone())).unwrap();
    let started = runner.start(&order()).unwrap();
    let person = editor.clone();
    let typing = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        person.person_edits("src/cancel.txt", "mine\n");
    });
    runner.follow(started);
    typing.join().unwrap();
    let refused = &agent.replies()[1];
    assert!(refused.is_error);
    assert!(
        refused
            .text
            .starts_with("refused: src/cancel.txt changed since you read it"),
        "{}",
        refused.text
    );
    assert!(
        refused.text.ends_with("Its text now:\nmine\n"),
        "{}",
        refused.text
    );
    assert_eq!(editor.buffer("src/cancel.txt").as_deref(), Some("mine\n"));
}

#[test]
fn a_path_out_of_the_project_or_into_sorcerers_own_is_refused() {
    let run = run(
        vec![
            Step::Tool(
                "write_file",
                json!({ "path": "../outside.txt", "text": "x" }),
            ),
            Step::Tool(
                "write_file",
                json!({ "path": ".sorcerer/config.toml", "text": "x" }),
            ),
            Step::Tool("write_file", json!({ "path": "src/cancel.txt" })),
            Step::Finish(0.0),
        ],
        &[],
    );
    let replies = run.agent.replies();
    assert!(replies.iter().all(|reply| reply.is_error), "{replies:?}");
    assert_eq!(
        replies[0].text,
        "../outside.txt is not a path inside the project"
    );
    assert!(
        replies[1].text.contains("fenced") || replies[1].text.contains("sorcerer's own"),
        "{}",
        replies[1].text
    );
    assert_eq!(replies[2].text, "write_file needs text");
    assert!(run.record.files.is_empty());
}

#[test]
fn a_write_to_an_agents_or_a_tools_config_is_refused() {
    let never = [
        ".claude/settings.json",
        ".mcp.json",
        ".envrc",
        "tools/.envrc",
        ".git/hooks/pre-commit",
        ".sorcerer/config.toml",
        ".Claude/commands/x.md",
    ];
    let script = never
        .iter()
        .map(|path| Step::Tool("write_file", json!({ "path": path, "text": "x" })))
        .chain([
            Step::Tool(
                "write_file",
                json!({ "path": "src/envrc.txt", "text": "x" }),
            ),
            Step::Finish(0.0),
        ])
        .collect();
    let run = run(script, &[]);
    let replies = run.agent.replies();
    for (path, reply) in never.iter().zip(&replies) {
        assert!(reply.is_error, "{path}: {}", reply.text);
        assert!(
            reply.text.contains("no task writes there") || reply.text.contains("fenced"),
            "{path}: {}",
            reply.text
        );
    }
    assert!(
        !replies[never.len()].is_error,
        "{}",
        replies[never.len()].text
    );
    assert_eq!(run.record.files.len(), 1);
}

#[test]
fn an_ask_is_answered_or_times_out_to_the_default() {
    let run = run(
        vec![
            Step::Tool(
                "ask",
                json!({ "question": "A clock port?", "options": ["inject a clock port", "call now()"], "file": "src/cancel.txt", "line": 1, "default": "call now()" }),
            ),
            Step::Tool(
                "ask",
                json!({ "question": "Name it?", "options": ["Clock", "Timer"], "default": "Clock" }),
            ),
            Step::Finish(0.2),
        ],
        &["inject a clock port"],
    );
    assert_eq!(
        replies_text(&run.agent),
        [
            "the person chose: inject a clock port",
            "no answer yet; carry on with the default (Clock)"
        ]
    );
    let asked = run.editor.asked.lock().unwrap().clone();
    assert_eq!(asked[0].at, Some(("src/cancel.txt".into(), 1)));
    let journal = run
        .project
        .state(&format!("tasks/{}/journal.md", run.record.id));
    assert!(
        journal.contains("A clock port?\n→ inject a clock port"),
        "{journal}"
    );
}

fn behavior(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn a_command_is_asked_once_and_remembered_for_the_project() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let first = run_with(
        project,
        vec![
            Step::Bash("uv run pytest -q"),
            Step::Bash("uv run pytest  -q"),
            Step::Bash("uv run pytest tests"),
            Step::Bash("uv run pytest -q && rm -rf x"),
            Step::Finish(0.1),
        ],
        &["Always for this project (uv run pytest -q)", "Deny", "Deny"],
        |_| {},
    );
    let replies = first.agent.replies();
    assert_eq!(behavior(&replies[0].text)["behavior"], "allow");
    assert_eq!(
        behavior(&replies[0].text)["updatedInput"]["command"],
        "uv run pytest -q"
    );
    // remembered: the same line runs unasked; another line of the program is asked about, and a
    // chained command too, and both are denied
    assert_eq!(behavior(&replies[1].text)["behavior"], "allow");
    assert_eq!(behavior(&replies[2].text)["behavior"], "deny");
    assert_eq!(behavior(&replies[3].text)["behavior"], "deny");
    let asked = first.editor.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 3);
    assert_eq!(
        asked[0].question,
        "apprentice wants to run: uv run pytest -q"
    );
    assert_eq!(
        asked[0].options,
        [
            "Allow once",
            "Always for this project (uv run pytest -q)",
            "Deny"
        ]
    );
    assert_eq!(asked[2].options, ["Allow once", "Deny"]);
    assert!(
        first
            .project
            .state("permissions.json")
            .contains("uv run pytest -q")
    );
}

#[test]
fn the_journal_says_a_command_waits_then_the_answer_not_that_it_runs() {
    let command = "ls -a && cat CLAUDE.md";
    // as claude does it in practice: the stream shows the call first, then asks
    let run = run(
        vec![
            tool_use("Bash", json!({ "command": command })),
            Step::Bash("ls -a && cat CLAUDE.md"),
            Step::Finish(0.1),
        ],
        &["Deny"],
    );
    let journal = run
        .project
        .state(&format!("tasks/{}/journal.md", run.record.id));
    assert!(!journal.contains("running ls"), "{journal}");
    let waiting = journal
        .find("## tool\n\nwaiting for you: ls")
        .expect(&journal);
    let asked = journal.find("## permission").expect(&journal);
    let denied = journal.find("## tool\n\ndenied: ls").expect(&journal);
    assert!(waiting < asked && asked < denied, "{journal}");
}

/// The person ruled: allowing one search never opens the door to a search of the whole tree.
#[test]
fn the_status_line_says_a_command_waits_and_was_denied_not_that_it_runs() {
    let command = "ls -a && cat CLAUDE.md";
    // claude asks first; its stream shows the call after the answer
    let run = run(
        vec![
            Step::Bash("ls -a && cat CLAUDE.md"),
            tool_use("Bash", json!({ "command": command })),
            Step::Finish(0.1),
        ],
        &["Deny"],
    );
    let progress = run.editor.progress.lock().unwrap().clone();
    assert!(
        progress
            .iter()
            .any(|line| line.contains("waiting for you: ls")),
        "{progress:?}"
    );
    assert!(
        progress.iter().any(|line| line.contains("denied: ls")),
        "{progress:?}"
    );
    assert!(
        !progress.iter().any(|line| line.contains("running ls")),
        "{progress:?}"
    );
}

#[test]
fn allowing_a_search_always_does_not_allow_a_recursive_one() {
    let run = run(
        vec![
            Step::Bash("grep foo src/"),
            Step::Bash("grep foo src/"),
            Step::Bash("grep -r KEY ."),
            Step::Bash("rg KEY"),
            Step::Bash("git log -p -1"),
            Step::Bash("cat s*/keys.txt"),
            Step::Finish(0.0),
        ],
        &[
            "Always for this project (grep foo src/)",
            "Deny",
            "Deny",
            "Deny",
            "Deny",
        ],
    );
    let replies = run.agent.replies();
    let answers: Vec<String> = replies
        .iter()
        .map(|reply| {
            behavior(&reply.text)["behavior"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(answers, ["allow", "allow", "deny", "deny", "deny", "deny"]);
    let asked = run.editor.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 5);
    // a tree reader or a glob is offered once, never always
    for ask in &asked[1..] {
        assert_eq!(ask.options, ["Allow once", "Deny"], "{}", ask.question);
    }
}

/// A version 1 `permissions.json` remembered first words, such as `grep`, which let any search
/// run. Its bare program names and tree readers are dropped; longer prefixes stay as exact lines.
#[test]
fn a_version_1_grant_of_a_bare_program_no_longer_lets_it_run() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let state = project.root.join(".sorcerer/state/apprentice");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("permissions.json"),
        r#"{"version":1,"data":{"allowed":["grep","git log","uv run pytest"]}}"#,
    )
    .unwrap();
    let run = run_with(
        project,
        vec![
            Step::Bash("grep -r KEY ."),
            Step::Bash("uv run pytest"),
            Step::Finish(0.0),
        ],
        &["Deny"],
        |_| {},
    );
    let replies = run.agent.replies();
    assert_eq!(behavior(&replies[0].text)["behavior"], "deny");
    assert_eq!(behavior(&replies[1].text)["behavior"], "allow");
    let saved = run.project.state("permissions.json");
    assert!(saved.contains("\"version\": 2"), "{saved}");
    assert!(!saved.contains("\"grep\""), "{saved}");
}

#[test]
fn a_command_naming_a_fenced_path_is_refused_even_when_allowed_always() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let state = project.root.join(".sorcerer/state/apprentice");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("permissions.json"),
        r#"{"version":2,"data":{"commands":["ls -la","ls .git","git status .sorcerer"]}}"#,
    )
    .unwrap();
    let run = run_with(
        project,
        vec![
            Step::Bash("ls -la"),
            Step::Bash("ls .git"),
            Step::Bash("git status .sorcerer"),
            Step::Bash("git show HEAD:.env"),
            Step::Bash("cat ./src/../.sorcerer/state/apprentice/tasks.jsonl"),
            Step::Bash("grep KEY src/cancel.txt"),
            Step::Finish(0.0),
        ],
        &["Allow once"],
        |_| {},
    );
    let replies = run.agent.replies();
    let refused = |at: usize| {
        let reply = behavior(&replies[at].text);
        reply["behavior"] == "deny" && reply["message"].as_str().unwrap().contains("is fenced")
    };
    assert_eq!(behavior(&replies[0].text)["behavior"], "allow");
    assert!(
        refused(1) && refused(2) && refused(3) && refused(4),
        "{replies:?}"
    );
    assert_eq!(behavior(&replies[5].text)["behavior"], "allow");
    let asked = run.editor.asked.lock().unwrap().clone();
    let questions: Vec<&str> = asked.iter().map(|ask| ask.question.as_str()).collect();
    assert_eq!(
        questions,
        ["apprentice wants to run: grep KEY src/cancel.txt"]
    );
}

#[test]
fn the_writers_and_askuserquestion_are_denied_outright() {
    let run = run(
        vec![
            Step::Tool(
                "permission",
                json!({ "tool_name": "Edit", "input": { "file_path": "a" } }),
            ),
            Step::Tool(
                "permission",
                json!({ "tool_name": "AskUserQuestion", "input": {} }),
            ),
            Step::Finish(0.0),
        ],
        &[],
    );
    let replies = run.agent.replies();
    assert_eq!(behavior(&replies[0].text)["behavior"], "deny");
    assert!(
        behavior(&replies[0].text)["message"]
            .as_str()
            .unwrap()
            .contains("write_file")
    );
    assert!(
        behavior(&replies[1].text)["message"]
            .as_str()
            .unwrap()
            .contains("ask")
    );
    assert!(run.editor.asked.lock().unwrap().is_empty());
}

#[test]
fn a_final_text_with_no_medium_is_posted() {
    let run = run(
        vec![
            Step::Event(AgentEvent::Text {
                text: "thinking".into(),
                parent: None,
            }),
            Step::Finish(0.1),
        ],
        &[],
    );
    assert!(
        run.editor
            .shown()
            .contains(&"notify: apprentice: done".to_string()),
        "{:?}",
        run.editor.shown()
    );
    let spoke = run_with(
        TempProject::with(&[("src/cancel.txt", BOOKING)]),
        vec![
            Step::Tool("notify", json!({ "text": "hi" })),
            Step::Finish(0.1),
        ],
        &[],
        |_| {},
    );
    assert_eq!(spoke.editor.shown(), ["notify: apprentice: hi"]);
}

#[test]
fn a_stopped_task_that_showed_nothing_is_charged_its_cap() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let agent = Arc::new(ScriptedAgent::playing(vec![Step::Hang]));
    let runner = TaskRunner::open(project.deps(editor.clone(), agent)).unwrap();
    let started = runner.start(&order()).unwrap();
    started.live.cancel.cancel();
    let record = runner.follow(started);
    assert_eq!(record.state, TaskState::Stopped);
    assert_eq!(record.cost_usd, 2.0);
}

#[test]
fn a_stopped_task_that_showed_its_tokens_is_charged_them() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let call = AgentEvent::Used {
        message_id: "msg_1".into(),
        model: "claude-sonnet-5-5".into(),
        usage: TokenUsage {
            input: 1_000,
            output: 10_000,
            ..TokenUsage::default()
        },
    };
    // the stream repeats a message once per block: it counts once
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Event(call.clone()),
        Step::Event(call),
        Step::Hang,
    ]));
    let runner = TaskRunner::open(project.deps(editor.clone(), agent)).unwrap();
    let started = runner.start(&order()).unwrap();
    let cancel = started.live.cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        cancel.cancel();
    });
    let record = runner.follow(started);
    assert_eq!(record.state, TaskState::Stopped);
    // sonnet at $2 in and $10 out per million: 1,000 in and 10,000 out
    assert!(
        (record.cost_usd - 0.102).abs() < 1e-9,
        "{}",
        record.cost_usd
    );
}

#[test]
fn a_result_without_a_cost_is_charged_the_cap() {
    let run = run(
        vec![Step::Event(AgentEvent::Finished(Finished {
            cost_usd: None,
            session_id: Some("s1".into()),
            is_error: false,
            text: "done".into(),
            turns: 2,
        }))],
        &[],
    );
    assert_eq!(run.record.state, TaskState::Done);
    assert_eq!(run.record.cost_usd, 2.0);
}

#[test]
fn a_spent_day_refuses_a_task_and_a_failed_start_is_recorded() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let spent = json!({
        "at": "2026-11-26T15:00:00Z", "product": "apprentice", "verb": "task", "model": "sonnet",
        "provider": "claude", "cost_usd": 5.0, "input_tokens": 0, "output_tokens": 0,
        "seconds": 1.0, "ok": true, "note": null
    });
    project.write(".sorcerer/state/ledger.jsonl", &format!("{spent}\n"));
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let runner =
        TaskRunner::open(project.deps(editor.clone(), Arc::new(ScriptedAgent::default()))).unwrap();
    let Err(refused) = runner.run(&order()) else {
        panic!("ran")
    };
    assert_eq!(
        refused.text(),
        "apprentice: today's budget is spent ($5.00 of $5.00); [apprentice] budget_per_day raises it"
    );
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let failing = ScriptedAgent {
        fails: true,
        ..ScriptedAgent::default()
    };
    let runner = TaskRunner::open(project.deps(editor, Arc::new(failing))).unwrap();
    assert!(matches!(
        runner.run(&order()),
        Err(TaskRefused::NotStarted(_))
    ));
    assert!(project.state("tasks.jsonl").contains(r#""state":"failed""#));
}

#[test]
fn the_first_message_says_the_task_and_where() {
    let run = run(vec![Step::Finish(0.0)], &[]);
    let request = run.agent.requests.lock().unwrap()[0].clone();
    assert!(
        request
            .prompt
            .starts_with("The person wrote this in their code"),
        "{}",
        request.prompt
    );
    assert!(request.prompt.contains("Refuse late cancellations."));
    assert!(request.prompt.contains("src/cancel.txt at line 1"));
    assert_eq!(
        (
            request.model.as_str(),
            request.budget_usd,
            request.max_turns
        ),
        ("sonnet", 2.0, 60)
    );
    assert!(request.system.contains("write_file"));
}

#[test]
fn a_task_in_a_fenced_file_is_refused_and_fenced_unsaved_edits_are_left_out() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING), (".env", "KEY=\n")]);
    let editor = Arc::new(RecordingEditor::at(&project.root));
    let agent = Arc::new(ScriptedAgent::playing(vec![Step::Finish(0.0)]));
    let runner = TaskRunner::open(project.deps(editor, agent.clone())).unwrap();
    let in_env = TaskOrder {
        file: ".env".into(),
        text: "KEY=sk_live_1\n".into(),
        ..order()
    };
    let Err(refused) = runner.run(&in_env) else {
        panic!("ran")
    };
    assert_eq!(
        refused.text(),
        "apprentice: .env is fenced; no task starts in it"
    );
    let with_unsaved = TaskOrder {
        unsaved: vec![apprentice::domain::edits_text::FileEdit {
            path: ".env".into(),
            before: "KEY=\n".into(),
            after: "KEY=sk_live_2\n".into(),
        }],
        ..order()
    };
    runner.run(&with_unsaved).unwrap();
    let prompt = agent.requests.lock().unwrap()[0].prompt.clone();
    assert!(!prompt.contains("sk_live"), "{prompt}");
}
