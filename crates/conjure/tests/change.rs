//! The prompt, the situation and the change use case, over scripted writers.

mod support;

use std::path::PathBuf;

use conjure::adapters::scry_files::{FindingsRead, read_findings};
use conjure::domain::change_prompt::CHANGE_SYSTEM;
use conjure::domain::rules::{RULES_CAP, rules_text};
use conjure::domain::scry_notes::{ScryNote, StoredFinding, notes_near};
use conjure::domain::situation::{Situation, situation_text};
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
        line: 40,
        span: Span { start: 38, end: 52 },
        module: "class Cancel:\n    def handle(self):\n        pass\n".into(),
        test_file: Some("src/cancel__test.py".into()),
        test_text: Some("def test_cancel():\n    assert True\n".into()),
        related: vec![("src/models.py".into(), "class Booking:\n    pass\n".into())],
        session: "diff --git a/x b/x\n+changed\n".into(),
        finding_asked: Some(note("handle returns before refunding", false)),
        notes: vec![note("a note", false), note("an old note", true)],
        rules: "A booking app.\n\n- Money is cents.".into(),
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
