//! apprentice's language server over an in-memory connection, driven by a small editor that
//! applies the edits it is sent, with Jev, the researcher and the agent as doubles: the walk of
//! the design (a claim checked on save, offered, done as a task that writes, asks and runs a
//! command once, then shown, diffed and undone), a stub offered by rule, a question answered and
//! kept, an untrusted project. No model is called.
#![allow(clippy::too_many_lines)]
mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use apprentice::domain::agent_event::AgentEvent;
use serde_json::json;
use support::{
    Doubles, ScriptedAgent, Served, Step, StubJev, StubResearcher, TempProject, served, tool_use,
};

const FILE: &str = "src/cancel_booking.py";
const BEFORE: &str = "class CancelBooking:\n    def handle(self, booking_id):\n        \"\"\"Cancel a booking.\n\n        \"\"\"\n        self.bookings.delete(booking_id)\n";
const CLAIM: &str = "        Cancelling less than two hours before the slot should be refused.\n";
const AFTER: &str = "class CancelBooking:\n    def handle(self, booking_id):\n        \"\"\"Cancel a booking.\n\n        Cancelling less than two hours before the slot should be refused.\n        \"\"\"\n        if self.clock.too_late(booking_id):\n            raise TooLateToCancel()\n        self.bookings.delete(booking_id)\n";

fn with_claim() -> String {
    BEFORE.replace("\n\n        \"\"\"", &format!("\n\n{CLAIM}        \"\"\""))
}

fn wrong_verdict() -> String {
    json!({
        "verdict": "wrong",
        "why": "handle deletes any booking it finds",
        "evidence": [{ "path": FILE, "unit": null, "line": 7 }],
    })
    .to_string()
}

/// Waits until the server's state file holds `wanted`.
fn state_holds(project: &TempProject, served: &Served, file: &str, wanted: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !project.state(file).contains(wanted) {
        assert!(
            Instant::now() < deadline,
            "{file} never held {wanted}: {}",
            project.state(file)
        );
        served.settle(Duration::from_millis(50));
    }
}

fn walk_agent() -> Arc<ScriptedAgent> {
    Arc::new(ScriptedAgent::playing(vec![
        tool_use("Read", json!({ "file_path": FILE })),
        Step::Tool("write_file", json!({ "path": FILE, "text": AFTER })),
        Step::Hook,
        Step::Tool(
            "ask",
            json!({ "question": "A clock?", "options": ["inject a clock port", "call datetime.now() in handle"], "file": FILE, "line": 2 }),
        ),
        Step::Bash("uv run pytest -q"),
        Step::Tool(
            "notify",
            json!({ "text": "late cancellations are refused" }),
        ),
        Step::Finish(0.71),
    ]))
}

#[test]
fn a_claim_checked_on_save_is_offered_done_as_a_task_shown_diffed_and_undone() {
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let agent = walk_agent();
    let jev = Arc::new(StubJev::saying("claim", 0.91, 0.82));
    let s = served(
        &project.root,
        Doubles {
            agent: agent.clone(),
            decider: Some(jev.clone()),
            researcher: Some(Arc::new(StubResearcher::replying(&wrong_verdict()))),
        },
    );
    s.open(FILE);
    s.settle(Duration::from_millis(100));

    // 0:00 the person writes the sentence; on idle Jev says claim: nothing shows yet
    s.type_text(FILE, &with_claim());
    state_holds(
        &project,
        &s,
        "regions.json",
        "Cancelling less than two hours",
    );
    assert!(jev.asked.lock().unwrap().iter().any(|q| q.kind == "kind"));

    // 0:08 a save checks it: the verdict in the margin, and the offer
    s.save(FILE);
    let marks = s.marks_until(FILE, |marks| {
        marks
            .iter()
            .any(|m| m.message.starts_with("apprentice: does not hold"))
    });
    let verdict = marks
        .iter()
        .find(|m| m.message.starts_with("apprentice: does not hold"))
        .unwrap();
    assert_eq!(verdict.range.start.line, 4);
    assert!(
        verdict
            .message
            .contains("handle deletes any booking it finds"),
        "{}",
        verdict.message
    );
    let titles = s.titles(FILE, 5);
    for title in [
        "apprentice: do this · ~$0.50 · sonnet",
        "apprentice: Show code",
        "apprentice: Dismiss",
        "apprentice: this was a claim",
        "apprentice: this was nothing",
    ] {
        assert!(titles.contains(&title.to_string()), "{title} in {titles:?}");
    }

    // 0:15 do this: the task writes, asks on a line, asks before a command, says it is done
    s.picks
        .borrow_mut()
        .push_back("Always for this project (uv run pytest -q)".into());
    s.run_action(FILE, 5, "apprentice: do this");
    assert_eq!(
        s.shown_message("apprentice asks:"),
        format!("apprentice asks: A clock? (choices on {FILE}:2)")
    );
    assert!(
        s.titles(FILE, 2)
            .contains(&"apprentice: inject a clock port".to_string())
    );
    s.run_action(FILE, 2, "apprentice: inject a clock port");
    assert_eq!(
        s.shown_message("apprentice: late"),
        "apprentice: late cancellations are refused"
    );
    let done = s.shown_message("apprentice: done");
    assert!(
        done.starts_with(
            "apprentice: done · cancelling less than two hours before the slot… · 1 file · $0.71"
        ),
        "{done}"
    );
    assert_eq!(s.text(FILE).as_deref(), Some(AFTER));
    let replies: Vec<String> = agent
        .replies()
        .into_iter()
        .map(|reply| reply.text)
        .collect();
    assert_eq!(
        replies[0],
        format!("written: {FILE} (in the editor, unsaved)")
    );
    assert_eq!(replies[2], "the person chose: inject a clock port");
    assert!(
        replies[3].contains(r#""behavior":"allow""#),
        "{}",
        replies[3]
    );
    assert_eq!(
        s.picked_questions.borrow()[0],
        "apprentice wants to run: uv run pytest -q"
    );
    assert!(
        project
            .state("permissions.json")
            .contains("uv run pytest -q")
    );

    // 1:50 the mark on the first changed line, and its actions
    let marks = s.marks_until(FILE, |marks| {
        marks
            .iter()
            .any(|m| m.message.starts_with("\u{2726} apprentice:"))
    });
    let task_mark = marks
        .iter()
        .find(|m| m.message.starts_with("\u{2726} apprentice:"))
        .unwrap();
    assert_eq!(task_mark.range.start.line, 6);
    let titles = s.titles(FILE, 7);
    for title in [
        "apprentice: Show diff",
        "apprentice: Undo this task",
        "apprentice: Show journal",
    ] {
        assert!(titles.contains(&title.to_string()), "{title} in {titles:?}");
    }
    s.run_action(FILE, 7, "apprentice: Show diff");
    let diff = s.notification("sorcerer/showDiff");
    assert_eq!(diff["uri"], s.uri(FILE));
    assert_eq!(diff["before"], with_claim());

    // undo: the buffer back to the person's text before the task
    s.run_action(FILE, 7, "apprentice: Undo this task");
    assert!(
        s.shown_message("apprentice: undid")
            .starts_with("apprentice: undid cancelling")
    );
    assert_eq!(s.text(FILE).as_deref(), Some(with_claim().as_str()));
    state_holds(&project, &s, "tasks.jsonl", r#""state":"undone""#);
    s.stop();
}

#[test]
fn saving_every_file_a_task_changed_keeps_it() {
    let project = TempProject::with(&[(FILE, BEFORE)]);
    // the instruction is unsaved: the first write is refused with the buffer, the second lands
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Tool("write_file", json!({ "path": FILE, "text": AFTER })),
        Step::Tool("write_file", json!({ "path": FILE, "text": AFTER })),
        Step::Finish(0.3),
    ]));
    let s = served(
        &project.root,
        Doubles {
            agent,
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    s.open(FILE);
    s.settle(Duration::from_millis(100));
    s.type_text(FILE, &with_claim());
    let marks = s.marks_until(FILE, |marks| {
        marks
            .iter()
            .any(|m| m.message.starts_with("apprentice: I can do this"))
    });
    assert_eq!(marks[0].severity, Some(lsp_types::DiagnosticSeverity::HINT));
    s.run_action(FILE, 5, "apprentice: do this");
    s.shown_message("apprentice: done");
    assert_eq!(
        s.shown_count("apprentice: done"),
        0,
        "the done message, once"
    );
    s.marks_until(FILE, |marks| {
        marks.iter().any(|m| m.message.starts_with("\u{2726}"))
    });
    s.save(FILE);
    state_holds(&project, &s, "tasks.jsonl", r#""state":"kept""#);
    s.marks_until(FILE, |marks| {
        !marks.iter().any(|m| m.message.starts_with("\u{2726}"))
    });
    s.stop();
}

#[test]
fn a_final_newline_the_editor_adds_to_a_write_is_nobodys_edit() {
    // the agent's text ends without a newline; the editor, like Neovim, reports one
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let unterminated = AFTER.trim_end_matches('\n');
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Tool("write_file", json!({ "path": FILE, "text": unterminated })),
        Step::Tool("write_file", json!({ "path": FILE, "text": unterminated })),
        Step::Hook,
        Step::Finish(0.3),
    ]));
    let s = served(
        &project.root,
        Doubles {
            agent: agent.clone(),
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    s.adds_final_newline.set(true);
    s.open(FILE);
    s.settle(Duration::from_millis(100));
    s.type_text(FILE, &with_claim());
    s.marks_until(FILE, |marks| {
        marks
            .iter()
            .any(|m| m.message.starts_with("apprentice: I can do this"))
    });
    s.run_action(FILE, 5, "apprentice: do this");
    s.shown_message("apprentice: done");
    assert_eq!(s.text(FILE).as_deref(), Some(AFTER));
    let replies: Vec<String> = agent.replies().into_iter().map(|r| r.text).collect();
    assert_eq!(replies[2], "", "no edits are fed to the agent: {replies:?}");
    s.marks_until(FILE, |marks| {
        marks.iter().any(|m| m.message.starts_with("\u{2726}"))
    });

    s.run_action(FILE, 7, "apprentice: Undo this task");
    assert!(
        s.shown_message("apprentice: undid")
            .starts_with("apprentice: undid"),
    );
    assert_eq!(s.text(FILE).as_deref(), Some(with_claim().as_str()));
    let task_dir = std::fs::read_dir(project.root.join(".sorcerer/state/apprentice/tasks"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal = std::fs::read_to_string(task_dir.join("journal.md")).unwrap();
    assert!(!journal.contains("## edits"), "{journal}");
    s.stop();
}

#[test]
fn a_line_a_sorcerer_server_landed_is_not_triaged_and_the_persons_is() {
    use lsp_types::{
        DocumentChangeOperation, DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier,
        TextDocumentEdit, TextEdit, WorkspaceEdit,
    };
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let jev = Arc::new(StubJev::saying("aside", 0.9, 0.2));
    let s = served(
        &project.root,
        Doubles {
            agent: Arc::new(ScriptedAgent::playing(Vec::new())),
            decider: Some(jev.clone()),
            researcher: None,
        },
    );
    s.open(FILE);
    s.settle(Duration::from_millis(100));

    // a sibling server (conjure, say) notes its edit, then the editor reports it as a change
    let landed = "        self.clock.tick(booking_id)\n";
    let edit = WorkspaceEdit {
        document_changes: Some(DocumentChanges::Operations(vec![
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: sorcerer_lsp::uri_of(&s.path(FILE)),
                    version: None,
                },
                edits: vec![OneOf::Left(TextEdit {
                    range: lsp_types::Range::default(),
                    new_text: landed.into(),
                })],
            }),
        ])),
        ..WorkspaceEdit::default()
    };
    sorcerer_lsp::tool_edits::note_tool_edit(
        &project.root.join(".sorcerer/state"),
        &edit,
        std::time::SystemTime::now(),
    );
    let by_the_tool = BEFORE.replace(
        "        self.bookings.delete",
        &format!("{landed}        self.bookings.delete"),
    );
    s.type_text(FILE, &by_the_tool);
    s.settle(Duration::from_millis(400));
    assert!(
        jev.asked.lock().unwrap().is_empty(),
        "Jev was asked about the tool's line"
    );

    // the person types a line of their own: that one is triaged
    let by_the_person = by_the_tool.replace(
        "        self.bookings.delete",
        "        self.audit.log(booking_id)\n        self.bookings.delete",
    );
    s.type_text(FILE, &by_the_person);
    state_holds(&project, &s, "regions.json", "self.audit.log");
    assert!(!project.state("regions.json").contains("self.clock.tick"));
    s.stop();
}

#[test]
fn a_stub_is_offered_by_rule_with_no_jev() {
    let project = TempProject::with(&[("src/cart.py", "def total(cart):\n    return 1\n")]);
    let s = served(
        &project.root,
        Doubles {
            agent: Arc::new(ScriptedAgent::default()),
            decider: None,
            researcher: None,
        },
    );
    s.open("src/cart.py");
    s.settle(Duration::from_millis(100));
    s.type_text(
        "src/cart.py",
        "def total(cart):\n    return 1\n\n\ndef tax(cart):\n    pass\n",
    );
    let marks = s.marks_until("src/cart.py", |marks| !marks.is_empty());
    assert!(
        marks[0]
            .message
            .starts_with("apprentice: I can do this · ~$0.50 · sonnet"),
        "{marks:?}"
    );
    assert_eq!(marks[0].range.start.line, 4);
    s.stop();
}

#[test]
fn a_stub_typed_before_the_server_saw_the_file_is_offered() {
    // VS Code starts the server without waiting; a buffer edited meanwhile (or restored
    // unsaved) is opened with the edit already in it
    let project = TempProject::with(&[("src/cart.py", "def total(cart):\n    return 1\n")]);
    let s = served(
        &project.root,
        Doubles {
            agent: Arc::new(ScriptedAgent::default()),
            decider: None,
            researcher: None,
        },
    );
    s.open_unsaved(
        "src/cart.py",
        "def total(cart):\n    return 1\n\n\ndef tax(cart):\n    pass\n",
    );
    let marks = s.marks_until("src/cart.py", |marks| !marks.is_empty());
    assert!(
        marks[0].message.starts_with("apprentice: I can do this"),
        "{marks:?}"
    );
    assert_eq!(marks[0].range.start.line, 4);
    s.stop();
}

#[test]
fn a_stub_and_its_comment_on_one_line_offer_one_row_of_each() {
    let project = TempProject::with(&[("src/cart.py", "def total(cart):\n    return 1\n")]);
    let jev = Arc::new(StubJev::saying("instruction", 0.9, 0.9));
    let s = served(
        &project.root,
        Doubles {
            agent: Arc::new(ScriptedAgent::default()),
            decider: Some(jev),
            researcher: None,
        },
    );
    s.open("src/cart.py");
    s.settle(Duration::from_millis(100));
    s.type_text(
        "src/cart.py",
        "def total(cart):\n    return 1\n\n\ndef tax(cart):\n    # The cart's tax: 20% of its total, rounded to the cent.\n    pass\n",
    );
    state_holds(&project, &s, "regions.json", "\"comment\"");
    let marks = s.marks_until("src/cart.py", |marks| !marks.is_empty());
    let offers = marks
        .iter()
        .filter(|m| m.message.starts_with("apprentice: I can do this"))
        .count();
    assert_eq!(offers, 1, "{marks:?}");
    let titles = s.titles("src/cart.py", 6);
    for title in [
        "apprentice: do this · ~$0.50 · sonnet",
        "apprentice: Dismiss",
        "apprentice: this was an instruction",
    ] {
        let rows = titles.iter().filter(|t| *t == title).count();
        assert_eq!(rows, 1, "{title} in {titles:?}");
    }
    s.stop();
}

#[test]
fn a_question_is_answered_on_save_and_its_answer_kept_as_a_comment() {
    let text = "def total(cart):\n    return sum(cart)\n";
    let asked = "def total(cart):\n    # why is the tax not in here?\n    return sum(cart)\n";
    let project = TempProject::with(&[("src/cart.py", text), ("src/tax.py", "RATE = 2\n")]);
    let s = served(
        &project.root,
        Doubles {
            agent: Arc::new(ScriptedAgent::default()),
            decider: Some(Arc::new(StubJev::saying("question", 0.95, 0.9))),
            researcher: Some(Arc::new(StubResearcher::replying(
                "Tax is added at checkout (src/tax.py:1).",
            ))),
        },
    );
    s.open("src/cart.py");
    s.settle(Duration::from_millis(100));
    s.type_text("src/cart.py", asked);
    state_holds(&project, &s, "regions.json", "why is the tax");
    s.save("src/cart.py");
    let marks = s.marks_until("src/cart.py", |marks| !marks.is_empty());
    assert!(
        marks[0]
            .message
            .starts_with("apprentice: Tax is added at checkout (src/tax.py:1).\n$0.02"),
        "{}",
        marks[0].message
    );
    let related = marks[0].related_information.as_ref().unwrap();
    assert_eq!(related[0].message, "src/tax.py:1");
    s.run_action("src/cart.py", 2, "apprentice: Keep this answer");
    s.settle(Duration::from_millis(300));
    assert_eq!(
        s.text("src/cart.py").as_deref(),
        Some(
            "def total(cart):\n    # why is the tax not in here?\n    # Tax is added at checkout (src/tax.py:1).\n    return sum(cart)\n"
        )
    );
    s.stop();
}

#[test]
fn a_dismissed_region_goes_and_a_label_is_a_line_of_the_corpus() {
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let s = served(
        &project.root,
        Doubles {
            agent: Arc::new(ScriptedAgent::default()),
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    s.open(FILE);
    s.settle(Duration::from_millis(100));
    s.type_text(FILE, &with_claim());
    s.marks_until(FILE, |marks| !marks.is_empty());
    s.run_action(FILE, 5, "apprentice: this was a claim");
    assert_eq!(
        s.shown_message("apprentice: labelled"),
        "apprentice: labelled as claim"
    );
    s.run_action(FILE, 5, "apprentice: Dismiss");
    state_holds(&project, &s, "labels.jsonl", r#""how":"dismiss""#);
    assert!(
        !s.titles(FILE, 5)
            .contains(&"apprentice: Dismiss".to_string())
    );
    let labels = project.state("labels.jsonl");
    assert_eq!(labels.lines().count(), 2, "{labels}");
    assert!(
        labels.contains(r#""person":"claim""#) && labels.contains(r#""how":"dismiss""#),
        "{labels}"
    );
    s.stop();
}

#[test]
fn an_untrusted_project_runs_nothing_and_says_so_once() {
    let project = TempProject::with(&[
        (FILE, BEFORE),
        (
            ".sorcerer/config.toml",
            "[model.providers.claude]\nrunner = \"/opt/claude\"\n",
        ),
    ]);
    let jev = Arc::new(StubJev::saying("claim", 0.91, 0.82));
    let s = served(
        &project.root,
        Doubles {
            agent: Arc::new(ScriptedAgent::default()),
            decider: Some(jev.clone()),
            researcher: None,
        },
    );
    s.open(FILE);
    s.type_text(FILE, &with_claim());
    assert_eq!(
        s.shown_message("apprentice: this project"),
        "apprentice: this project is not trusted; run \"apprentice trust\" in it"
    );
    s.settle(Duration::from_millis(300));
    assert!(jev.asked.lock().unwrap().is_empty());
    s.stop();
}

#[test]
fn the_agents_final_text_and_a_ghost_reach_the_editor() {
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Tool(
            "ghost",
            json!({ "file": FILE, "line": 2, "lines": ["    # a hint"] }),
        ),
        Step::Tool(
            "note",
            json!({ "title": "Late cancellations", "text": "Research: see docs." }),
        ),
        Step::Event(AgentEvent::Text {
            text: "thinking".into(),
            parent: None,
        }),
        Step::Finish(0.1),
    ]));
    let s = served(
        &project.root,
        Doubles {
            agent,
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    s.open(FILE);
    s.settle(Duration::from_millis(100));
    s.type_text(FILE, &with_claim());
    s.marks_until(FILE, |marks| !marks.is_empty());
    s.run_action(FILE, 5, "apprentice: do this");
    let ghost = s.notification("sorcerer/ghost");
    assert_eq!(
        (ghost["line"].clone(), ghost["lines"][0].clone()),
        (json!(1), json!("    # a hint"))
    );
    let note = s.notification("sorcerer/note");
    assert!(
        note["uri"]
            .as_str()
            .unwrap()
            .ends_with("-late-cancellations.md"),
        "{note}"
    );
    s.shown_message("apprentice: done");
    s.stop();
}

#[test]
fn a_fenced_file_open_in_the_editor_never_reaches_the_agent() {
    const JOURNAL: &str = ".sorcerer/state/apprentice/tasks/t0/journal.md";
    let project = TempProject::with(&[
        (FILE, BEFORE),
        (".env", "STRIPE_KEY=\n"),
        ("docs/plans/launch.md", "# Launch\n"),
        (JOURNAL, "# an old task\n"),
        (
            ".sorcerer/config.toml",
            "[model]\nnot_evidence = [\"docs/plans\"]\n",
        ),
    ]);
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Tool(
            "ask",
            json!({ "question": "Which?", "options": ["this one", "that one"], "file": FILE, "line": 2 }),
        ),
        Step::Hook,
        Step::Finish(0.1),
    ]));
    let s = served(
        &project.root,
        Doubles {
            agent: agent.clone(),
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    for file in [FILE, ".env", "docs/plans/launch.md", JOURNAL] {
        s.open(file);
    }
    s.settle(Duration::from_millis(100));
    // unsaved before the task: these would be in the first message
    s.type_text(".env", "STRIPE_KEY=sk_live_before\n");
    s.type_text("docs/plans/launch.md", "# Launch\nsecret plan before\n");
    s.type_text(JOURNAL, "# an old task\njournal line before\n");
    s.type_text(FILE, &with_claim());
    s.marks_until(FILE, |marks| !marks.is_empty());
    s.run_action(FILE, 5, "apprentice: do this");

    // during the task, after its first look: these would be in the hook's diff
    s.shown_message("apprentice asks:");
    s.type_text(".env", "STRIPE_KEY=sk_live_during\n");
    s.type_text("docs/plans/launch.md", "# Launch\nsecret plan during\n");
    s.type_text(JOURNAL, "# an old task\njournal line during\n");
    s.type_text(FILE, &format!("{}# the person's edit\n", with_claim()));
    s.settle(Duration::from_millis(100));
    s.run_action(FILE, 2, "apprentice: this one");
    s.shown_message("apprentice: done");

    let prompt = agent.requests.lock().unwrap()[0].prompt.clone();
    let hook = agent.replies()[1].text.clone();
    assert!(hook.contains("the person's edit"), "{hook}");
    for said in [&prompt, &hook] {
        for secret in ["sk_live", "secret plan", "journal line"] {
            assert!(!said.contains(secret), "{secret} in {said}");
        }
    }
    s.stop();
}

#[test]
fn a_write_over_unsaved_edits_the_agent_has_not_seen_is_refused_once_with_the_editors_text() {
    let renamed = with_claim().replace("booking_id", "booking_ref");
    let from_disk = AFTER.to_string();
    let rebased = AFTER.replace("booking_id", "booking_ref");
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Tool("write_file", json!({ "path": FILE, "text": from_disk })),
        Step::Tool("write_file", json!({ "path": FILE, "text": rebased })),
        Step::Finish(0.2),
    ]));
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let s = served(
        &project.root,
        Doubles {
            agent: agent.clone(),
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    s.open(FILE);
    s.settle(Duration::from_millis(100));
    // the person renames, unsaved, and writes the instruction: the disk still says booking_id
    s.type_text(FILE, &renamed);
    s.marks_until(FILE, |marks| !marks.is_empty());
    s.run_action(FILE, 5, "apprentice: do this");
    s.shown_message("apprentice: done");
    let replies = agent.replies();
    assert!(replies[0].is_error, "{}", replies[0].text);
    assert!(
        replies[0]
            .text
            .ends_with(&format!("Its text now:\n{renamed}")),
        "{}",
        replies[0].text
    );
    assert_eq!(
        replies[1].text,
        format!("written: {FILE} (in the editor, unsaved)")
    );
    assert_eq!(s.text(FILE).as_deref(), Some(rebased.as_str()));
    s.stop();
}

#[test]
fn two_do_this_at_once_start_one_task_and_it_can_be_stopped() {
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let agent = Arc::new(ScriptedAgent::playing(vec![Step::Hang]));
    let s = served(
        &project.root,
        Doubles {
            agent: agent.clone(),
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    s.open(FILE);
    s.settle(Duration::from_millis(100));
    s.type_text(FILE, &with_claim());
    s.marks_until(FILE, |marks| !marks.is_empty());
    let actions = s.actions(FILE, 5);
    let command = actions
        .iter()
        .find(|action| {
            action["title"]
                .as_str()
                .unwrap()
                .starts_with("apprentice: do this")
        })
        .unwrap()["command"]
        .clone();
    let params = json!({ "command": command["command"], "arguments": command["arguments"] });
    let ids: Vec<i32> = (0..4)
        .map(|_| s.request("workspace/executeCommand", params.clone()))
        .collect();
    for id in ids {
        s.response(id);
    }
    s.shown_message("apprentice: a task is running");
    s.settle(Duration::from_millis(300));
    assert_eq!(agent.requests.lock().unwrap().len(), 1);
    s.run_action(FILE, 5, "apprentice: Stop this task");
    let ended = s.shown_message("apprentice: stopped");
    assert!(ended.starts_with("apprentice: stopped"), "{ended}");
    s.stop();
}

/// S9: an editor that answers the picker with an error gets a command's permission question as
/// choices on the instruction's line, and a question of several lines shows its first line
/// there, the rest in the mark's hover text.
#[test]
fn an_editor_with_no_picker_is_asked_a_permission_on_the_instructions_line() {
    let script = "cat > notes.txt <<EOF\nlate cancellations\nEOF";
    let project = TempProject::with(&[(FILE, BEFORE)]);
    let agent = Arc::new(ScriptedAgent::playing(vec![
        Step::Bash(script),
        Step::Finish(0.1),
    ]));
    let s = served(
        &project.root,
        Doubles {
            agent: agent.clone(),
            decider: Some(Arc::new(StubJev::saying("instruction", 0.9, 0.9))),
            researcher: None,
        },
    );
    s.no_picker.set(true);
    s.open(FILE);
    s.settle(Duration::from_millis(100));
    s.type_text(FILE, &with_claim());
    s.marks_until(FILE, |marks| !marks.is_empty());
    s.run_action(FILE, 5, "apprentice: do this");
    let said = s.shown_message("apprentice asks: apprentice wants to run: cat > notes.txt <<EOF …");
    assert!(said.ends_with(&format!("(choices on {FILE}:5)")), "{said}");
    let is_ask = |mark: &lsp_types::Diagnostic| {
        mark.code == Some(lsp_types::NumberOrString::String("ask".into()))
    };
    let marks = s.marks_until(FILE, |marks| marks.iter().any(is_ask));
    let ask = marks.iter().find(|mark| is_ask(mark)).unwrap();
    assert_eq!(
        ask.message,
        "apprentice asks: apprentice wants to run: cat > notes.txt <<EOF …\nlate cancellations\nEOF"
    );
    s.run_action(FILE, 5, "apprentice: Allow once");
    s.shown_message("apprentice: done");
    let allowed: serde_json::Value = serde_json::from_str(&agent.replies()[0].text).unwrap();
    assert_eq!(allowed["behavior"], "allow", "{allowed}");
    s.stop();
}
