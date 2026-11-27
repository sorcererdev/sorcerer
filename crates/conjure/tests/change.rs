//! The prompt, the situation and the change use case, over scripted writers.

mod support;

use std::path::PathBuf;

use conjure::adapters::scry_files::read_findings;
use conjure::domain::change_prompt::CHANGE_SYSTEM;
use conjure::domain::context_choice::Closeness;
use conjure::domain::rules::{RULES_CAP, rules_text};
use conjure::domain::scry_notes::{FindingsRead, ScryNote, StoredFinding, notes_near};
use conjure::domain::situation::{RelatedFile, Situation, situation_text};
use sorcerer_core::domain::unit::Span;
use support::*;

#[test]
fn the_change_system_is_one_line_and_fits_cmd_exe() {
    assert!(!CHANGE_SYSTEM.contains('\n') && !CHANGE_SYSTEM.contains('\r'));
    assert!(CHANGE_SYSTEM.len() < 4000, "{}", CHANGE_SYSTEM.len());
    assert!(CHANGE_SYSTEM.starts_with("You take one small step"));
    assert!(CHANGE_SYSTEM.ends_with("and no file blocks."));
}

fn note(statement: &str, stale: bool) -> ScryNote {
    ScryNote {
        issue_type: "bug".into(),
        rule: Some("money-is-cents".into()),
        statement: statement.into(),
        file: "src/cancel.py".into(),
        line: 41,
        stale,
    }
}

fn fixed_situation() -> Situation {
    Situation {
        language: "python".into(),
        unit_words: "a top-level class or function name (Name) or a method (Class.method)".into(),
        file: "src/cancel.py".into(),
        unit: "Cancel.handle".into(),
        broken_region: false,
        line: 40,
        span: Span { start: 38, end: 52 },
        module: "class Cancel:\n    def handle(self):\n        pass\n".into(),
        test_file: Some("src/cancel__test.py".into()),
        test_text: Some("def test_cancel():\n    assert True\n".into()),
        related: vec![RelatedFile {
            path: "src/models.py".into(),
            text: "class Booking:\n    pass\n".into(),
            closeness: Closeness::Imported,
            names: vec!["Booking".into()],
        }],
        left_for_room: vec![],
        session: "diff --git a/x b/x\n+changed\n".into(),
        finding_asked: Some(note("handle returns before refunding", false)),
        notes: vec![note("a note", false), note("an old note", true)],
        rules: "A booking app.\n\n- Money is cents.".into(),
        left_out: vec![],
    }
}

#[test]
fn the_prompt_is_as_recorded() {
    let recorded = format!("{CHANGE_SYSTEM}\n{}", situation_text(&fixed_situation()));
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/change-prompt.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &recorded).unwrap();
    }
    let golden = std::fs::read_to_string(&path).unwrap();
    assert_eq!(recorded, golden, "UPDATE_GOLDEN=1 rewrites the golden file");
}

/// conjure-context-choice: related files are whole or not shown at all. One left out for room
/// is named, so the writer knows it exists and does not rewrite it unseen.
#[test]
fn a_related_file_left_out_for_room_is_named_not_shown() {
    let s = Situation {
        left_for_room: vec!["src/slot.rs".into(), "src/table.py".into()],
        ..fixed_situation()
    };
    let text = situation_text(&s);
    assert!(
        text.contains("Also related, but left out for room (not shown, so do not rewrite them): src/slot.rs, src/table.py."),
        "{text}"
    );
    assert!(text.contains("Related file src/models.py:\n"));
    assert!(!situation_text(&fixed_situation()).contains("left out for room"));
}

#[test]
fn a_situation_with_no_test_file_and_nothing_else_says_so() {
    let s = Situation {
        test_file: None,
        test_text: None,
        related: vec![],
        session: "  \n".into(),
        finding_asked: None,
        notes: vec![],
        rules: String::new(),
        ..fixed_situation()
    };
    let text = situation_text(&s);
    assert!(
        text.contains("No test file is known for this language."),
        "{text}"
    );
    assert!(!text.contains("Related file"));
    assert!(!text.contains("uncommitted"));
    assert!(!text.contains("findings"));
    assert!(!text.contains("rules"));
    let s = Situation {
        test_file: Some("t.py".into()),
        test_text: None,
        ..s
    };
    assert!(
        situation_text(&s)
            .contains("There is no test file for this module yet (its tests would go in t.py).")
    );
}

fn finding(
    id: &str,
    file: &str,
    unit: Option<&str>,
    lines: (u32, u32),
    status: &str,
) -> StoredFinding {
    serde_json::from_value(serde_json::json!({
        "id": id, "issue_type": "bug", "rule": null, "statement": format!("statement {id}"),
        "place": {"file": file, "unit": unit, "lines": {"start": lines.0, "end": lines.1}, "fingerprint": "aaaaaaa"},
        "evidence": [], "status": status
    }))
    .unwrap()
}

fn handle_unit() -> sorcerer_core::domain::unit::Unit {
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::{Unit, UnitKind};
    Unit {
        name: "Cancel.handle".into(),
        kind: UnitKind::Method,
        span: Span { start: 38, end: 52 },
        body: None,
        fingerprint: Fingerprint::parse("aaaaaaa").unwrap(),
    }
}

#[test]
fn notes_keep_open_and_unverified_near_the_unit_at_most_five() {
    let mut all = vec![
        finding(
            "a",
            "src/cancel.py",
            Some("Cancel.handle"),
            (41, 41),
            "open",
        ),
        finding(
            "b",
            "src/cancel.py",
            Some("Cancel.handle"),
            (42, 42),
            "unverified",
        ),
        finding(
            "c",
            "src/cancel.py",
            Some("Cancel.handle"),
            (43, 43),
            "dismissed-here",
        ),
        finding("d", "src/other.py", Some("Other.f"), (5, 5), "open"),
        finding("t", "src/cancel__test.py", Some("test_x"), (3, 3), "open"),
    ];
    for i in 0..6 {
        all.push(finding(
            &format!("x{i}"),
            "src/cancel.py",
            None,
            (40 + i, 40 + i),
            "open",
        ));
    }
    let notes = notes_near(
        &all,
        &handle_unit(),
        std::path::Path::new("src/cancel.py"),
        Some(std::path::Path::new("src/cancel__test.py")),
        |_, _| Some("aaaaaaa".into()),
    );
    assert_eq!(notes.len(), 5);
    let statements: Vec<&str> = notes.iter().map(|n| n.statement.as_str()).collect();
    assert!(statements[..2].contains(&"statement a") && statements[..2].contains(&"statement b"));
    assert!(
        !statements.contains(&"statement c"),
        "dismissed is not a note"
    );
    assert!(
        !statements.contains(&"statement d"),
        "another file's unit is not near"
    );
}

#[test]
fn a_finding_citing_the_units_lines_is_a_note() {
    let mut f = finding("e", "src/elsewhere.py", Some("E.f"), (9, 9), "open");
    f.evidence = vec![conjure::domain::scry_notes::StoredEvidence {
        file: "src/cancel.py".into(),
        lines: Span { start: 50, end: 60 },
    }];
    let notes = notes_near(
        &[f],
        &handle_unit(),
        std::path::Path::new("src/cancel.py"),
        None,
        |_, _| None,
    );
    assert_eq!(notes.len(), 1);
}

#[test]
fn a_moved_unit_marks_its_note_stale() {
    let all = vec![finding(
        "a",
        "src/cancel.py",
        Some("Cancel.handle"),
        (41, 41),
        "open",
    )];
    let fresh = notes_near(
        &all,
        &handle_unit(),
        "src/cancel.py".as_ref(),
        None,
        |_, _| Some("aaaaaaa".into()),
    );
    assert!(!fresh[0].stale);
    let moved = notes_near(
        &all,
        &handle_unit(),
        "src/cancel.py".as_ref(),
        None,
        |_, _| Some("bbbbbbb".into()),
    );
    assert!(moved[0].stale);
}

#[test]
fn rules_are_cut_at_3000_characters() {
    let long = "x".repeat(4000);
    let toml = format!("context = \"{long}\"\n");
    let text = rules_text(&toml, |_| None).unwrap();
    assert!(
        text.ends_with("… (rules cut at 3000 characters)"),
        "{}",
        &text[text.len() - 60..]
    );
    assert!(text.chars().count() < RULES_CAP + 60);
}

#[test]
fn rules_read_context_includes_and_rule_text_and_ignore_other_keys() {
    let toml = "context = \"A booking app.\"\ninclude = [\"docs/style.md\"]\nfuture = 1\n[[rule]]\nid = \"cents\"\ntext = \"Money is cents.\"\n";
    let text = rules_text(toml, |p| {
        (p == std::path::Path::new("docs/style.md")).then(|| "Use snake_case.\n".to_string())
    })
    .unwrap();
    assert_eq!(
        text,
        "A booking app.\n\nUse snake_case.\n\n- Money is cents."
    );
    assert!(rules_text("context = [", |_| None).is_err());
}

#[test]
fn a_findings_file_of_version_1_is_other_version() {
    let d = git_project();
    write(
        d.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 1, "data": {"findings": []}}"#,
    );
    let (project, _) = project_with_vendored_grammars(d.path());
    match read_findings(&project).unwrap() {
        FindingsRead::OtherVersion(why) => assert!(why.contains("version 1"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_findings_file_is_no_file() {
    let d = git_project();
    let (project, _) = project_with_vendored_grammars(d.path());
    assert_eq!(read_findings(&project).unwrap(), FindingsRead::NoFile);
}

#[test]
fn a_version_2_file_is_read_for_the_fields_conjure_needs() {
    let d = git_project();
    write(
        d.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 2, "data": {"findings": [{"id": "3fa9c0", "issue_type": "bug", "rule": null,
        "statement": "s", "place": {"file": "a.py", "unit": "f", "lines": {"start": 1, "end": 2},
        "unit_start": 1, "fingerprint": "aaaaaaa"}, "evidence": [], "scores": {}, "status": "open",
        "extra": 1}], "reviewed": {}, "capped": {}}}"#,
    );
    let (project, _) = project_with_vendored_grammars(d.path());
    let FindingsRead::Found(found) = read_findings(&project).unwrap() else {
        panic!()
    };
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "3fa9c0");
    let _ = PathBuf::new();
}

#[test]
fn the_session_keeps_the_languages_files_and_cuts_at_300_lines() {
    use conjure::domain::situation::session_text;
    let globs = {
        let mut b = globset::GlobSetBuilder::new();
        b.add(globset::Glob::new("**/*.py").unwrap());
        b.build().unwrap()
    };
    let diff = |n: usize| -> String { (0..n).map(|i| format!("+line {i}\n")).collect() };
    let diffs = vec![
        ("a.py".into(), diff(250)),
        ("notes.md".into(), diff(5)),
        ("uv.lock".into(), diff(5)),
        ("poetry-lock.py".into(), diff(5)),
        (".sorcerer/x.py".into(), diff(5)),
        (
            "img.py".into(),
            "Binary files a/img.py and b/img.py differ\n".to_string(),
        ),
        ("b.py".into(), diff(100)),
    ];
    let new_files: Vec<PathBuf> = (0..25)
        .map(|i| format!("new{i}.py").into())
        .chain(["x.md".into()])
        .collect();
    let text = session_text(&diffs, &new_files, &globs);
    assert!(text.contains("+line 0"));
    assert!(!text.contains("Binary files") && !text.contains("notes.md"));
    assert!(text.contains("… (50 more lines)"), "{text}");
    assert_eq!(text.lines().filter(|l| l.starts_with("+line")).count(), 300);
    assert!(text.contains("New files, not yet in git: new0.py, new1.py"));
    assert!(text.contains("new19.py … (5 more)"), "{text}");
    assert!(!text.contains("x.md"));
}

// Task 9: the writers and the records.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use conjure::adapters::jsonl_change_store::JsonlChangeStore;
use conjure::adapters::provider_writer::ProviderWriter;
use conjure::adapters::research_writer::ResearchWriter;
use conjure::domain::record::{
    Attempt, ChangeRecord, Choice, RecordedFacts, RecordedFile, Trigger, change_id,
};
use conjure::ports::change_store::ChangeStore;
use conjure::ports::writer::Writer;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_models::adapters::scripted_provider::ScriptedProvider;
use sorcerer_models::domain::model::{Reply, StopReason, Usage};
use sorcerer_models::domain::research::{Research, Researched, Tool};
use sorcerer_models::ports::researcher::Researcher;

fn reply(text: &str, cost: Option<f64>) -> Reply {
    Reply {
        text: text.into(),
        tool_calls: vec![],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
        },
        cost_usd: cost,
        stop: StopReason::End,
        seconds: 2.0,
    }
}

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::at("2026-11-22T12:00:00Z".parse().unwrap()))
}

#[test]
fn the_provider_writer_sends_the_change_system_and_records_one_errand() {
    let d = tempfile::tempdir().unwrap();
    let ledger = Arc::new(JsonlLedger::at(d.path().join("ledger.jsonl")));
    let provider = Arc::new(ScriptedProvider::new(vec![reply(
        "<goal>g</goal>",
        Some(0.001),
    )]));
    let writer = ProviderWriter {
        provider: provider.clone(),
        model: "haiku".into(),
        ledger: ledger.clone(),
        clock: clock(),
    };
    let written = writer.write("the situation", &Cancel::default()).unwrap();
    assert_eq!(written.text, "<goal>g</goal>");
    let seen = provider.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].system, CHANGE_SYSTEM);
    assert_eq!(seen[0].messages.len(), 1);
    assert!(seen[0].tools.is_empty());
    assert_eq!(seen[0].max_tokens, 8192);
    assert_eq!(seen[0].timeout, Duration::from_secs(120));
    assert_eq!(seen[0].model.0, "haiku");
    let rows = ledger.read().unwrap().errands;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].product.as_str(), rows[0].verb.as_str()),
        ("conjure", "conjure")
    );
    assert_eq!(rows[0].cost_usd, Some(0.001));
    assert!(rows[0].ok);
    assert_eq!(
        rows[0].at,
        "2026-11-22T12:00:00Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap()
    );
}

#[test]
fn a_failed_call_is_recorded_not_ok() {
    let d = tempfile::tempdir().unwrap();
    let ledger = Arc::new(JsonlLedger::at(d.path().join("ledger.jsonl")));
    let writer = ProviderWriter {
        provider: Arc::new(ScriptedProvider::new(vec![])),
        model: "haiku".into(),
        ledger: ledger.clone(),
        clock: clock(),
    };
    assert!(writer.write("s", &Cancel::default()).is_err());
    let rows = ledger.read().unwrap().errands;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].ok);
    assert!(rows[0].note.as_deref().unwrap().contains("no more replies"));
}

struct SpyResearcher {
    asked: Mutex<Vec<Research>>,
}

impl Researcher for SpyResearcher {
    fn research(&self, research: Research, _: &Cancel) -> sorcerer_models::Result<Researched> {
        self.asked.lock().unwrap().push(research);
        Ok(Researched {
            answer: "answer".into(),
            errand: errand("try-harder", "sonnet", 0.02),
            reads: vec![],
        })
    }
}

#[test]
fn the_research_writer_asks_with_three_tools_and_a_quarter() {
    let spy = Arc::new(SpyResearcher {
        asked: Mutex::new(vec![]),
    });
    let writer = ResearchWriter {
        researcher: spy.clone(),
        model: "sonnet".into(),
    };
    let written = writer.write("the situation", &Cancel::default()).unwrap();
    assert_eq!(written.text, "answer");
    let asked = spy.asked.lock().unwrap();
    assert_eq!(
        (asked[0].product.as_str(), asked[0].verb.as_str()),
        ("conjure", "try-harder")
    );
    assert_eq!(asked[0].tools, [Tool::Read, Tool::Grep, Tool::Glob]);
    assert_eq!(asked[0].cap_usd, 0.25);
    assert_eq!(asked[0].timeout, Duration::from_secs(300));
    assert_eq!(asked[0].model, "sonnet");
    assert!(
        asked[0].prompt.starts_with(CHANGE_SYSTEM) && asked[0].prompt.ends_with("the situation")
    );
}

fn a_record(id: &str) -> ChangeRecord {
    ChangeRecord {
        version: 1,
        id: id.into(),
        at: "2026-11-22T12:00:00Z".parse().unwrap(),
        file: "src/a.py".into(),
        line: 12,
        unit: "A.f".into(),
        trigger: Trigger::Finding("3fa9c0".into()),
        attempt: Attempt::TryHarder,
        try_harder_of: Some("111111".into()),
        facts: RecordedFacts {
            parse_errors: vec![],
            todo_lines: vec![3],
            body_empty: true,
        },
        model: "sonnet".into(),
        goal: Some("fill it".into()),
        files: vec![RecordedFile {
            path: "src/a.py".into(),
            new: false,
            lines_changed: 7,
            parse_errors: vec![],
        }],
        dropped: vec!["../x".into()],
        seconds: 4.2,
        cost_usd: Some(0.02),
        choice: Choice::NoChange,
        retry_drops: vec![],
    }
}

#[test]
fn a_record_round_trips_and_a_line_of_another_version_is_skipped() {
    let d = git_project();
    let (project, _) = project_with_vendored_grammars(d.path());
    let store = JsonlChangeStore::for_project(&project).unwrap();
    assert_eq!(store.read().unwrap().records.len(), 0);
    store.append(&a_record("aaaaaa")).unwrap();
    store.append(&a_record("bbbbbb")).unwrap();
    let path = d.path().join(".sorcerer/state/conjure/changes.jsonl");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("{\"version\": 2, \"id\": \"x\"}\nnot json\n");
    std::fs::write(&path, text).unwrap();
    let rows = store.read().unwrap();
    assert_eq!(rows.records, [a_record("aaaaaa"), a_record("bbbbbb")]);
    assert_eq!(rows.skipped, 2);
    let first = std::fs::read_to_string(&path).unwrap();
    let line: serde_json::Value = serde_json::from_str(first.lines().next().unwrap()).unwrap();
    assert_eq!(line["trigger"], serde_json::json!({"finding": "3fa9c0"}));
    assert_eq!(line["attempt"], "try-harder");
    assert_eq!(line["choice"], "no-change");
}

#[test]
fn a_cursor_trigger_is_the_word_cursor_and_ids_are_six_hex_digits() {
    assert_eq!(
        serde_json::to_string(&Trigger::Cursor).unwrap(),
        "\"cursor\""
    );
    let id = change_id(
        "2026-11-22T12:00:00Z".parse().unwrap(),
        std::path::Path::new("src/a.py"),
        12,
    );
    assert_eq!(id.len(), 6);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

// Task 10: the use case.

use conjure::domain::budget::day_allows;
use conjure::domain::edits::{DropReason, NameCase};
use conjure::domain::record::Attempt as AttemptKind;
use conjure::usecases::conjure_change::{ChangeRefused, ChangeRequest, conjure_change};

fn ask(file: &str, line: u32) -> ChangeRequest {
    ChangeRequest {
        file: file.into(),
        line,
        selection: None,
        finding: None,
        attempt: AttemptKind::First,
        situation: None,
    }
}

#[test]
fn a_change_on_a_stub_lands_in_the_file_and_its_new_test_file() {
    let d = cart_project();
    let rig = Rig::new(d.path(), CART_REPLY);
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert_eq!(change.goal.as_deref(), Some("fill total"));
    assert_eq!(change.unit, "Cart.total");
    assert!(change.facts.body_empty);
    assert_eq!(change.files.len(), 2);
    let (cart, test) = (&change.files[0], &change.files[1]);
    assert_eq!(cart.path, std::path::Path::new("src/shop/cart.py"));
    assert!(!cart.new && cart.before.is_some());
    assert_eq!(cart.first_changed_line, 3);
    assert_eq!(cart.lines_changed, 1);
    assert!(cart.parsed && cart.parse_errors.is_empty());
    assert!(test.new && test.before.is_none());
    assert_eq!(test.edits.len(), 1);
    assert_eq!(change.id.len(), 6);
    // nothing is written to disk
    assert!(
        std::fs::read_to_string(d.path().join("src/shop/cart.py"))
            .unwrap()
            .contains("pass")
    );
    assert!(!d.path().join("src/shop/cart__test.py").exists());
    // the writer was shown the situation: the unit, its file and where its tests would go
    let shown = &rig.writer.situations()[0];
    assert!(
        shown.contains("The cursor is in `Cart.total` in src/shop/cart.py, line 3"),
        "{shown}"
    );
    assert!(shown.contains("There is no test file for this module yet (its tests would go in src/shop/cart__test.py)."), "{shown}");
}

#[test]
fn outside_a_unit_is_refused_with_its_sentence() {
    let d = cart_project();
    write(d.path(), "src/shop/notes.py", "# only a comment\n\nx = 1\n");
    let rig = Rig::new(d.path(), CART_REPLY);
    let refused = conjure_change(&ask("src/shop/notes.py", 1), &rig.deps()).unwrap_err();
    assert_eq!(refused, ChangeRefused::OutsideAUnit);
    assert_eq!(refused.text(), "put the cursor in a function to conjure");
    assert!(rig.writer.situations().is_empty());
    // a file in no language is outside a unit as well
    let refused = conjure_change(&ask("README.md", 1), &rig.deps()).unwrap_err();
    assert_eq!(refused, ChangeRefused::OutsideAUnit);
}

const BROKEN_RUST: &str = "struct Cart;\n\nimpl Cart {\n    fn before(&self) {}\n\n    pub fn total(&self) -> u32 {\n        look the table up\n        if the party is too big say so\n    }\n}\n\nfn after() {}\n";

#[test]
fn a_function_the_parser_lost_is_conjured_as_a_broken_region_to_repair() {
    let d = git_project();
    write(d.path(), "src/cart.rs", BROKEN_RUST);
    commit(d.path(), "broken");
    let rig = Rig::new(d.path(), "<goal>repair total</goal>");
    conjure_change(&ask("src/cart.rs", 8), &rig.deps()).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(
        shown.contains("The cursor is in code that does not parse, in src/cart.rs, line 8"),
        "{shown}"
    );
    assert!(shown.contains("Repair it"), "{shown}");
}

#[test]
fn a_line_with_no_unit_and_no_broken_region_is_still_refused() {
    let d = git_project();
    write(d.path(), "src/cart.rs", "use std::fmt;\n\nfn total() {}\n");
    commit(d.path(), "clean");
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    let refused = conjure_change(&ask("src/cart.rs", 1), &rig.deps()).unwrap_err();
    assert_eq!(refused, ChangeRefused::OutsideAUnit);
}

#[test]
fn a_selection_takes_its_lines() {
    let d = cart_project();
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    let mut request = ask("src/shop/cart.py", 5);
    request.selection = Some(Span { start: 5, end: 6 });
    conjure_change(&request, &rig.deps()).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(shown.contains("(the unit is lines 5-6)"), "{shown}");
    assert!(shown.contains("`Cart.empty`"), "{shown}");
}

#[test]
fn a_finding_asked_leads_the_situation() {
    let d = cart_project();
    write(
        d.path(),
        ".sorcerer/state/scry/findings.json",
        r#"{"version": 2, "data": {"findings": [{"id": "3fa9c0", "issue_type": "bug", "rule": null,
        "statement": "total ignores the items", "place": {"file": "src/shop/cart.py", "unit": "Cart.total",
        "lines": {"start": 3, "end": 3}, "fingerprint": null}, "evidence": [], "status": "open"}]}}"#,
    );
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    let mut request = ask("src/shop/cart.py", 3);
    request.finding = Some("3fa9c0".into());
    conjure_change(&request, &rig.deps()).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(
        shown.starts_with("The person asked you to conjure for this finding: [bug] total ignores the items (src/shop/cart.py:3)"),
        "{shown}"
    );
    assert!(shown.contains("scry's open findings here"), "{shown}");
    let mut unknown = ask("src/shop/cart.py", 3);
    unknown.finding = Some("nope".into());
    let e = conjure_change(&unknown, &rig.deps()).unwrap_err();
    assert!(e.text().contains("nope"), "{}", e.text());
}

#[test]
fn a_fenced_path_is_dropped() {
    let d = cart_project();
    let outside = tempfile::tempdir().unwrap();
    sorcerer_core::adapters::platform::paths::link_to(outside.path(), &d.path().join("link"))
        .unwrap();
    write(
        d.path(),
        ".sorcerer/config.toml",
        "[conjure]\nexclude = [\"gen/**\"]\n",
    );
    let mut reply = String::from("<goal>g</goal>\n");
    for path in [
        ".env",
        ".env.local",
        ".sorcerer/x",
        ".git/hooks/pre-commit",
        "gen/x.py",
        "link/x.py",
        "../x.py",
        "src/shop/ok.py",
    ] {
        reply.push_str(&format!("<file path=\"{path}\">\nx = 1\n</file>\n"));
    }
    let rig = Rig::new(d.path(), &reply);
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    let kept: Vec<_> = change.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(kept, [std::path::PathBuf::from("src/shop/ok.py")]);
    let reasons: std::collections::BTreeMap<_, _> = change.dropped.iter().cloned().collect();
    assert_eq!(reasons[".env"], DropReason::Fenced);
    assert_eq!(reasons[".env.local"], DropReason::Fenced);
    assert_eq!(reasons[".sorcerer/x"], DropReason::Fenced);
    assert_eq!(reasons[".git/hooks/pre-commit"], DropReason::Fenced);
    assert_eq!(reasons["gen/x.py"], DropReason::Excluded);
    assert_eq!(reasons["link/x.py"], DropReason::Fenced);
    assert_eq!(reasons["../x.py"], DropReason::OutsideProject);
    assert_eq!(change.dropped.len(), 7);
    assert!(!outside.path().join("x.py").exists());
}

#[test]
fn a_reply_with_nothing_is_no_change() {
    let d = cart_project();
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert!(change.files.is_empty());
    assert_eq!(change.goal.as_deref(), Some("nothing"));
}

#[test]
fn files_equal_to_their_base_are_no_change() {
    let d = cart_project();
    let same = std::fs::read_to_string(d.path().join("src/shop/cart.py")).unwrap();
    let rig = Rig::new(
        d.path(),
        &format!("<goal>tidy</goal>\n<file path=\"src/shop/cart.py\">\n{same}</file>\n"),
    );
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert!(change.files.is_empty());
    assert_eq!(change.goal.as_deref(), Some("tidy"));
}

#[test]
fn a_crlf_file_in_the_project_keeps_its_line_endings() {
    let d = git_project();
    write(d.path(), "src/a.py", "def f():\r\n    pass\r\n");
    commit(d.path(), "a");
    let rig = Rig::new(
        d.path(),
        "<goal>g</goal>\n<file path=\"src/a.py\">\ndef f():\n    return 1\n</file>\n",
    );
    let change = conjure_change(&ask("src/a.py", 1), &rig.deps()).unwrap();
    assert_eq!(change.files[0].after, "def f():\r\n    return 1\r\n");
    assert_eq!(change.files[0].edits[0].text, "    return 1\r\n");
}

#[test]
fn a_file_that_does_not_parse_says_its_lines() {
    let d = cart_project();
    let rig = Rig::new(
        d.path(),
        "<goal>g</goal>\n<file path=\"src/shop/cart.py\">\nclass Cart:\n    def total(self:\n        return 0\n</file>\n",
    );
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert!(!change.files[0].parse_errors.is_empty());
}

#[test]
fn a_spent_day_is_refused_before_any_call() {
    let d = cart_project();
    let rig = Rig::new(d.path(), CART_REPLY);
    let mut spent = errand("conjure", "haiku", 3.00);
    spent.at = "2026-11-22T09:00:00Z".parse().unwrap();
    rig.ledger.append(&spent).unwrap();
    let refused = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap_err();
    assert_eq!(refused, ChangeRefused::DaySpent { budget: 3.0 });
    assert_eq!(
        refused.text(),
        "conjure: today's budget ($3.00) is spent. Conjuring resumes tomorrow; \"conjure cost\" shows the days, and [conjure] budget_per_day sets the budget."
    );
    assert!(rig.writer.situations().is_empty());
}

#[test]
fn yesterdays_spending_does_not_count() {
    let d = cart_project();
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    let mut spent = errand("conjure", "haiku", 9.00);
    spent.at = "2026-11-21T09:00:00Z".parse().unwrap();
    rig.ledger.append(&spent).unwrap();
    assert!(conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).is_ok());
}

#[test]
fn try_harder_is_refused_when_its_cap_would_pass_the_day() {
    assert!(day_allows(2.80, 3.00, AttemptKind::First));
    assert!(!day_allows(2.80, 3.00, AttemptKind::TryHarder));
    assert!(day_allows(2.75, 3.00, AttemptKind::TryHarder));
    assert!(!day_allows(3.00, 3.00, AttemptKind::First));
    let d = cart_project();
    let rig = Rig::new(d.path(), CART_REPLY);
    let mut spent = errand("conjure", "haiku", 2.80);
    spent.at = "2026-11-22T09:00:00Z".parse().unwrap();
    rig.ledger.append(&spent).unwrap();
    let mut harder = ask("src/shop/cart.py", 3);
    harder.attempt = AttemptKind::TryHarder;
    assert_eq!(
        conjure_change(&harder, &rig.deps()).unwrap_err(),
        ChangeRefused::DaySpent { budget: 3.0 }
    );
    assert!(rig.writer.situations().is_empty());
}

#[test]
fn try_harder_reuses_the_situation_it_is_given() {
    let d = cart_project();
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    let mut harder = ask("src/shop/cart.py", 3);
    harder.attempt = AttemptKind::TryHarder;
    harder.situation = Some("the first situation, kept".into());
    let change = conjure_change(&harder, &rig.deps()).unwrap();
    assert_eq!(rig.writer.situations(), ["the first situation, kept"]);
    assert_eq!(change.situation, "the first situation, kept");
}

#[test]
fn the_session_and_related_files_reach_the_situation() {
    let d = git_project();
    write(d.path(), "src/shop/item.py", "class Item:\n    price = 1\n");
    write(
        d.path(),
        "src/shop/cart.py",
        "from shop.item import Item\n\n\nclass Cart:\n    def total(self):\n        pass\n",
    );
    commit(d.path(), "base");
    write(
        d.path(),
        "src/shop/cart.py",
        "from shop.item import Item\n\n\nclass Cart:\n    def total(self):\n        pass  # wip\n",
    );
    write(d.path(), "src/shop/new_thing.py", "x = 1\n");
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    conjure_change(&ask("src/shop/cart.py", 5), &rig.deps()).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(shown.contains("Related file src/shop/item.py:"), "{shown}");
    assert!(
        shown.contains("What the person has been doing (uncommitted changes):"),
        "{shown}"
    );
    assert!(shown.contains("+        pass  # wip"), "{shown}");
    assert!(
        shown.contains("New files, not yet in git: src/shop/new_thing.py"),
        "{shown}"
    );
}

/// conjure-context-choice (the user's ruling on walk 2): a related file is never cut at 120
/// lines. It is shown whole; one that does not fit the step's budget is left out whole and
/// named, the import the unit uses kept first.
#[test]
fn related_files_come_whole_and_one_without_room_is_named() {
    let d = git_project();
    let long_but_fitting = format!("class Item:\n{}", "    price = 1\n".repeat(300));
    let too_big = format!("class Huge:\n{}", "    x = 1\n".repeat(3_000));
    write(d.path(), "src/shop/item.py", &long_but_fitting);
    write(d.path(), "src/shop/huge.py", &too_big);
    write(
        d.path(),
        "src/shop/cart.py",
        "from shop.huge import Huge\nfrom shop.item import Item\n\n\nclass Cart:\n    def total(self):\n        return Item.price\n",
    );
    commit(d.path(), "base");
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    conjure_change(&ask("src/shop/cart.py", 6), &rig.deps()).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(
        shown.contains(&long_but_fitting),
        "a 301-line file is shown whole"
    );
    assert!(
        !shown.contains("class Huge"),
        "too big for the budget: not shown"
    );
    assert!(
        shown.contains("left out for room (not shown, so do not rewrite them): src/shop/huge.py."),
        "{shown}"
    );
}

// Review fixes.

fn conjure_cart(
    rig: &Rig,
) -> Result<conjure::usecases::conjure_change::ConjuredChange, ChangeRefused> {
    conjure_change(&ask("src/shop/cart.py", 5), &rig.deps())
}

const NOT_EVIDENCE: &str =
    "[model]\nnot_evidence = [\"src/secrets\", \"src/shop/cart__test.py\"]\n";

fn cart_importing(module: &str) -> String {
    format!("from {module} import X\n\n\nclass Cart:\n    def total(self):\n        pass\n")
}

#[test]
fn a_not_evidence_import_is_left_out_of_the_situation() {
    let d = git_project();
    write(d.path(), ".sorcerer/config.toml", NOT_EVIDENCE);
    write(d.path(), "src/secrets/keys.py", "X = 'SECRET_TOKEN'\n");
    write(
        d.path(),
        "src/shop/cart.py",
        &cart_importing("secrets.keys"),
    );
    commit(d.path(), "base");
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    conjure_cart(&rig).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(!shown.contains("SECRET_TOKEN"), "{shown}");
    assert!(
        shown.contains("Left out, because models may not read them: "),
        "{shown}"
    );
    assert!(shown.contains("src/secrets/keys.py"), "{shown}");
}

#[test]
fn a_symlinked_import_is_left_out_of_the_situation() {
    let d = git_project();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "lib.py", "X = 'OUTSIDE_MARK'\n");
    sorcerer_core::adapters::platform::paths::link_to(outside.path(), &d.path().join("src_vendor"))
        .unwrap();
    std::fs::create_dir_all(d.path().join("src")).unwrap();
    sorcerer_core::adapters::platform::paths::link_to(outside.path(), &d.path().join("src/vendor"))
        .unwrap();
    write(d.path(), "src/shop/cart.py", &cart_importing("vendor.lib"));
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    conjure_cart(&rig).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(!shown.contains("OUTSIDE_MARK"), "{shown}");
    assert!(shown.contains("Left out"), "{shown}");
}

#[test]
fn a_not_evidence_session_diff_is_left_out_of_the_situation() {
    let d = git_project();
    write(d.path(), ".sorcerer/config.toml", NOT_EVIDENCE);
    write(d.path(), "src/secrets/conf.py", "A = 1\n");
    write(d.path(), "src/shop/cart.py", &cart_importing("os"));
    commit(d.path(), "base");
    write(d.path(), "src/secrets/conf.py", "A = 'DIFF_MARK'\n");
    write(d.path(), "src/secrets/new.py", "B = 1\n");
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    conjure_cart(&rig).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(
        !shown.contains("DIFF_MARK") && !shown.contains("New files, not yet in git"),
        "{shown}"
    );
    assert!(
        shown.contains("src/secrets/new.py"),
        "named as left out: {shown}"
    );
    assert!(
        shown.contains("src/secrets/conf.py"),
        "named as left out: {shown}"
    );
}

#[test]
fn a_fenced_test_file_is_left_out_of_the_situation() {
    let d = git_project();
    write(d.path(), ".sorcerer/config.toml", NOT_EVIDENCE);
    write(d.path(), "src/shop/cart.py", &cart_importing("os"));
    write(
        d.path(),
        "src/shop/cart__test.py",
        "def test(): assert 'TEST_MARK'\n",
    );
    commit(d.path(), "base");
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    conjure_cart(&rig).unwrap();
    let shown = &rig.writer.situations()[0];
    assert!(!shown.contains("TEST_MARK"), "{shown}");
    assert!(shown.contains("src/shop/cart__test.py"), "{shown}");
}

#[test]
fn a_fenced_target_is_refused_before_any_call() {
    let d = git_project();
    write(d.path(), ".sorcerer/config.toml", NOT_EVIDENCE);
    write(
        d.path(),
        "src/secrets/keys.py",
        "def f():\n    return 'SECRET_TOKEN'\n",
    );
    commit(d.path(), "base");
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    let refused = conjure_change(&ask("src/secrets/keys.py", 2), &rig.deps()).unwrap_err();
    assert!(refused.text().contains("fenced"), "{}", refused.text());
    assert!(rig.writer.situations().is_empty());
}

#[test]
fn an_import_template_that_leaves_the_project_is_not_followed() {
    let d = git_project();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "abs.py", "X = 'ABS_MARK'\n");
    let absolute = outside.path().join("abs.py");
    write(
        d.path(),
        ".sorcerer/languages/python/language.toml",
        &format!(
            "[imports]\nresolve = [{:?}, \"../{{= path =}}.py\"]\n",
            absolute.to_string_lossy()
        ),
    );
    write(d.path(), "src/shop/cart.py", &cart_importing("anything"));
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    conjure_cart(&rig).unwrap();
    assert!(!rig.writer.situations()[0].contains("ABS_MARK"));
}

#[test]
fn exclude_ignores_case_and_follows_links() {
    let d = cart_project();
    std::fs::create_dir_all(d.path().join("gen")).unwrap();
    sorcerer_core::adapters::platform::paths::link_to(&d.path().join("gen"), &d.path().join("g"))
        .unwrap();
    write(
        d.path(),
        ".sorcerer/config.toml",
        "[conjure]\nexclude = [\"gen/**\"]\n",
    );
    let reply = "<goal>g</goal>\n<file path=\"Gen/x.py\">\nx = 1\n</file>\n<file path=\"g/y.py\">\ny = 1\n</file>\n<file path=\"GEN/z.py\">\nz = 1\n</file>\n";
    let mut rig = Rig::new(d.path(), reply);
    rig.name_case = NameCase::Ignored;
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert!(change.files.is_empty(), "{:?}", change.files);
    let reasons: Vec<_> = change.dropped.iter().map(|(_, why)| *why).collect();
    assert_eq!(reasons, [DropReason::Excluded; 3]);
}

#[test]
fn two_spellings_of_one_file_are_one_change_and_the_last_wins() {
    let d = cart_project();
    let reply = "<goal>g</goal>\n<file path=\"src/shop/a.py\">\none = 1\n</file>\n<file path=\"./src/shop//a.py\">\ntwo = 2\n</file>\n<file path=\"SRC/shop/A.py\">\nthree = 3\n</file>\n<file path=\"src/x/../shop/a.py\">\nfour = 4\n</file>\n";
    let mut rig = Rig::new(d.path(), reply);
    rig.name_case = NameCase::Ignored;
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert_eq!(change.files.len(), 1, "{:?}", change.files);
    assert_eq!(change.files[0].after, "four = 4\n");
}

#[test]
fn where_case_matters_two_spellings_that_differ_in_case_are_two_files() {
    let d = cart_project();
    let reply = "<goal>g</goal>\n<file path=\"src/shop/Foo.py\">\none = 1\n</file>\n<file path=\"./src/shop/foo.py\">\ntwo = 2\n</file>\n<file path=\"src/shop//foo.py\">\nthree = 3\n</file>\n";
    let mut rig = Rig::new(d.path(), reply);
    rig.name_case = NameCase::Matters;
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    let after: Vec<_> = change
        .files
        .iter()
        .map(|f| (f.path.to_string_lossy().replace('\\', "/"), f.after.clone()))
        .collect();
    assert_eq!(
        after,
        [
            ("src/shop/Foo.py".to_string(), "one = 1\n".to_string()),
            ("src/shop/foo.py".to_string(), "three = 3\n".to_string())
        ]
    );
}

#[test]
fn where_case_matters_exclude_matches_case_exactly() {
    let d = cart_project();
    write(
        d.path(),
        ".sorcerer/config.toml",
        "[conjure]\nexclude = [\"gen/**\"]\n",
    );
    let reply = "<goal>g</goal>\n<file path=\"Gen/x.py\">\nx = 1\n</file>\n<file path=\"gen/y.py\">\ny = 1\n</file>\n";
    let mut rig = Rig::new(d.path(), reply);
    rig.name_case = NameCase::Matters;
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert_eq!(change.files.len(), 1);
    assert_eq!(change.files[0].path, std::path::Path::new("Gen/x.py"));
    assert_eq!(
        change.dropped,
        [("gen/y.py".to_string(), DropReason::Excluded)]
    );
}

#[test]
fn a_binary_file_and_a_directory_named_by_the_reply_are_not_text() {
    let d = cart_project();
    std::fs::create_dir_all(d.path().join("assets")).unwrap();
    std::fs::write(d.path().join("assets/logo.png"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let reply = "<goal>g</goal>\n<file path=\"assets/logo.png\">\ntext\n</file>\n<file path=\"src/shop\">\ntext\n</file>\n<file path=\"docs/notes.md\">\nhello\n</file>\n";
    let rig = Rig::new(d.path(), reply);
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    let reasons: std::collections::BTreeMap<_, _> = change.dropped.iter().cloned().collect();
    assert_eq!(reasons["assets/logo.png"], DropReason::NotText);
    assert_eq!(reasons["src/shop"], DropReason::NotText);
    // a file of no known language is kept, and reported unparsed
    assert_eq!(change.files.len(), 1);
    assert!(
        change.files[0].new && !change.files[0].parsed && change.files[0].parse_errors.is_empty()
    );
    assert_eq!(
        std::fs::read(d.path().join("assets/logo.png")).unwrap(),
        [0xff, 0xfe, 0x00, 0x80]
    );
}

#[test]
fn the_day_is_the_users_local_day() {
    let d = cart_project();
    let (project, _) = project_with_vendored_grammars(d.path());
    let _ = project;
    let mut rig = Rig::new(d.path(), "<goal>nothing</goal>");
    // 02:00 UTC on the 23rd is 21:00 on the 22nd in UTC-5
    rig.clock = FixedClock::at("2026-11-23T02:00:00Z".parse().unwrap())
        .in_zone(chrono::FixedOffset::west_opt(5 * 3600).unwrap());
    let mut spent = errand("conjure", "haiku", 3.00);
    spent.at = "2026-11-22T10:00:00Z".parse().unwrap();
    rig.ledger.append(&spent).unwrap();
    assert_eq!(
        conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap_err(),
        ChangeRefused::DaySpent { budget: 3.0 }
    );
}

struct TimingOut;

impl sorcerer_models::ports::provider::Provider for TimingOut {
    fn name(&self) -> &str {
        "slow"
    }
    fn resolve(
        &self,
        alias: &str,
    ) -> sorcerer_models::Result<sorcerer_models::domain::model::ModelId> {
        Ok(sorcerer_models::domain::model::ModelId(alias.into()))
    }
    fn complete(
        &self,
        _: sorcerer_models::domain::model::Completion,
        _: &Cancel,
    ) -> sorcerer_models::Result<Reply> {
        Err(sorcerer_models::Error::Timeout {
            provider: "slow".into(),
            seconds: 120,
        })
    }
    fn supports_tools(&self) -> bool {
        false
    }
}

#[test]
fn a_timed_out_call_counts_toward_the_day() {
    let d = cart_project();
    write(
        d.path(),
        ".sorcerer/config.toml",
        "[conjure]\nbudget_per_day = 0.05\n",
    );
    let ledger = Arc::new(JsonlLedger::at(d.path().join("ledger.jsonl")));
    let writer = ProviderWriter {
        provider: Arc::new(TimingOut),
        model: "haiku".into(),
        ledger: ledger.clone(),
        clock: clock(),
    };
    for _ in 0..3 {
        assert!(writer.write("s", &Cancel::default()).is_err());
    }
    let rows = ledger.read().unwrap().errands;
    assert!(rows.iter().all(|e| !e.ok && e.cost_usd.unwrap() > 0.0));
    assert!(
        rows[0].note.as_deref().unwrap().contains("estimate"),
        "{:?}",
        rows[0].note
    );
    // the day (budget $0.05) is spent by three timeouts: the use case refuses before a fourth
    let rig = Rig::new(d.path(), "<goal>nothing</goal>");
    for e in &rows {
        rig.ledger.append(e).unwrap();
    }
    assert!(matches!(
        conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()),
        Err(ChangeRefused::DaySpent { .. })
    ));
}

#[test]
fn a_file_without_a_final_newline_keeps_it_when_the_last_line_is_untouched() {
    let d = git_project();
    write(
        d.path(),
        "src/a.py",
        "def f():\n    pass\n\ndef g():\n    return 1",
    );
    commit(d.path(), "a");
    let rig = Rig::new(
        d.path(),
        "<goal>g</goal>\n<file path=\"src/a.py\">\ndef f():\n    return 0\n\ndef g():\n    return 1\n</file>\n",
    );
    let change = conjure_change(&ask("src/a.py", 2), &rig.deps()).unwrap();
    assert_eq!(
        change.files[0].after,
        "def f():\n    return 0\n\ndef g():\n    return 1"
    );
    assert_eq!(change.files[0].edits.len(), 1);
    assert_eq!(change.files[0].edits[0].to_end_of_file, None);
}

struct SilentWriter;

impl conjure::ports::writer::Writer for SilentWriter {
    fn write(
        &self,
        _: &str,
        _: &sorcerer_core::Cancel,
    ) -> sorcerer_models::Result<conjure::ports::writer::Written> {
        Err(sorcerer_models::Error::Timeout {
            provider: "claude".into(),
            seconds: 120,
        })
    }

    fn model(&self) -> &str {
        "haiku"
    }
}

#[test]
fn a_writer_that_does_not_answer_is_told_apart_with_the_unit() {
    let d = cart_project();
    let rig = Rig::new(d.path(), CART_REPLY);
    let deps = conjure::usecases::conjure_change::ChangeDeps {
        writer: &SilentWriter,
        ..rig.deps()
    };
    let refused = conjure_change(&ask("src/shop/cart.py", 3), &deps).unwrap_err();
    let ChangeRefused::NotAnswered { why, unit, facts } = &refused else {
        panic!("{refused:?}")
    };
    assert_eq!(why, "claude: timed out after 120 s");
    assert_eq!(unit, "Cart.total");
    assert!(facts.body_empty);
    assert_eq!(
        refused.text(),
        "conjure: the model did not answer: claude: timed out after 120 s"
    );
}

#[test]
fn a_change_carries_its_units_lines() {
    let d = cart_project();
    let rig = Rig::new(d.path(), CART_REPLY);
    let change = conjure_change(&ask("src/shop/cart.py", 3), &rig.deps()).unwrap();
    assert_eq!(change.unit_lines, Span { start: 2, end: 3 });
}

#[test]
fn a_retry_is_recorded_under_its_own_verb() {
    let d = tempfile::tempdir().unwrap();
    let ledger = Arc::new(JsonlLedger::at(d.path().join("ledger.jsonl")));
    let writer = ProviderWriter {
        provider: Arc::new(ScriptedProvider::new(vec![
            reply("<goal>a</goal>", Some(0.001)),
            reply("<goal>b</goal>", Some(0.002)),
        ])),
        model: "haiku".into(),
        ledger: ledger.clone(),
        clock: clock(),
    };
    writer.write("s", &Cancel::default()).unwrap();
    writer.write_retry("s", &Cancel::default()).unwrap();
    let verbs: Vec<String> = ledger
        .read()
        .unwrap()
        .errands
        .iter()
        .map(|row| row.verb.clone())
        .collect();
    assert_eq!(verbs, ["conjure", "conjure-retry"]);
}

#[test]
fn the_research_writer_records_a_retry_as_a_retry() {
    let spy = Arc::new(SpyResearcher {
        asked: Mutex::new(vec![]),
    });
    let writer = ResearchWriter {
        researcher: spy.clone(),
        model: "sonnet".into(),
    };
    writer.write_retry("s", &Cancel::default()).unwrap();
    assert_eq!(spy.asked.lock().unwrap()[0].verb, "conjure-retry");
}
