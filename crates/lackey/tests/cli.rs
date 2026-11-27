//! The command line over doubles: a task from the terminal, the log, the cost, the doctor, and
//! the helpers refusing to run by hand.

mod support;

use std::sync::Arc;

use lackey::cli::{Wiring, main_with};
use serde_json::json;
use sorcerer_core::adapters::env::MapEnv;
use support::{FixedClock, ScriptedAgent, Step, TempProject};

const BOOKING: &str = "def handle(self):\n    delete(booking)\n";

fn wiring(script: Vec<Step>) -> Wiring {
    let agent = Arc::new(ScriptedAgent::playing(script));
    Wiring {
        agent: Arc::new(move |_| Ok(agent.clone())),
        clock: Arc::new(FixedClock::thanksgiving()),
    }
}

fn run(project: &TempProject, args: &[&str], wiring: &Wiring) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = std::iter::once("lackey")
        .chain(args.iter().copied())
        .map(Into::into)
        .collect();
    let code = main_with(
        args,
        &project.root,
        &MapEnv::default(),
        wiring,
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_task_from_the_terminal_writes_to_disk_and_prints_what_it_said() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let wiring = wiring(vec![
        Step::Tool(
            "write_file",
            json!({ "path": "src/cancel.txt", "text": "refuse()\n" }),
        ),
        Step::Tool(
            "ask",
            json!({ "question": "Port?", "options": ["port", "now()"], "default": "now()" }),
        ),
        Step::Bash("make test"),
        Step::Tool("notify", json!({ "text": "refused" })),
        Step::Finish(0.3),
    ]);
    let (code, out, err) = run(
        &project,
        &["src/cancel.txt:1", "Refuse late cancellations."],
        &wiring,
    );
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(project.read("src/cancel.txt"), "refuse()\n");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "wrote src/cancel.txt");
    assert_eq!(lines[1], "asked: Port? [port | now()] → now()");
    // a command is never allowed from the terminal
    assert_eq!(
        lines[2],
        "asked: lackey wants to run: make test [Allow once | Always for this project (make test) | Deny] → Deny"
    );
    assert_eq!(lines[3], "lackey: refused");
    assert!(
        lines[4].starts_with("lackey: done · refuse late cancellations · 1 file · $0.30"),
        "{out}"
    );

    let (code, out, _) = run(&project, &["log"], &wiring);
    assert_eq!(code, 0);
    assert!(out.contains("done"), "{out}");
    assert!(
        out.contains("src/cancel.txt:1  refuse late cancellations"),
        "{out}"
    );
    let (code, out, _) = run(&project, &["cost"], &wiring);
    assert_eq!(code, 0);
    assert!(
        out.starts_with("day         triage  answers  checks  tasks  cost"),
        "{out}"
    );
    assert!(
        out.contains("2026-11-26  0       0        0       1      $0.30"),
        "{out}"
    );
}

#[test]
fn a_task_needs_a_place_and_an_instruction() {
    let project = TempProject::with(&[("src/cancel.txt", BOOKING)]);
    let wiring = wiring(Vec::new());
    let (code, _, err) = run(&project, &["src/cancel.txt:1"], &wiring);
    assert_eq!(code, 2);
    assert!(
        err.contains("give an instruction after src/cancel.txt:1"),
        "{err}"
    );
    let (code, _, err) = run(&project, &["nowhere.txt:1", "do"], &wiring);
    assert_eq!(code, 2);
    assert!(err.contains("nowhere.txt cannot be read"), "{err}");
}

#[test]
fn the_helpers_refuse_to_run_by_hand_and_a_hook_never_fails() {
    let project = TempProject::with(&[]);
    let wiring = wiring(Vec::new());
    let (code, _, err) = run(&project, &["mcp"], &wiring);
    assert_eq!(code, 2);
    assert!(err.contains("LACKEY_PORT"), "{err}");
    let (code, out, _) = run(&project, &["hook", "post-tool-use"], &wiring);
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn the_doctor_names_what_tasks_need() {
    let project = TempProject::with(&[]);
    let (_, out, _) = run(&project, &["doctor"], &wiring(Vec::new()));
    let names: Vec<&str> = out
        .lines()
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "claude", "provider", "jev", "config", "ledger", "records", "trust"
        ]
    );
    assert!(out.contains("JEV_API_KEY is not set"), "{out}");
    assert!(
        out.contains("$2.00 a task, $5.00 a day, $0.20 a day of triage"),
        "{out}"
    );
}

#[test]
fn a_log_with_no_tasks_says_so() {
    let project = TempProject::with(&[]);
    let (code, out, err) = run(&project, &["log"], &wiring(Vec::new()));
    assert_eq!((code, out.as_str()), (0, ""));
    assert_eq!(err, "lackey: no tasks recorded yet\n");
}
