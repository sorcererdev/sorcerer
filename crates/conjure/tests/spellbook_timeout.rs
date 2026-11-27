//! The live checker gives up on a spellbook that hangs: the check is stopped after its timeout
//! and answers `TimedOut`, so the change goes ahead unchecked. Needs a shell to stand in for
//! spellbook, so it runs where there is one.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use conjure::adapters::spellbook_checker::SpellbookChecker;
use conjure::domain::spellbook_findings::RulesRead;
use conjure::ports::rule_checker::RuleChecker;

fn program(dir: &std::path::Path, body: &str) -> PathBuf {
    let path = dir.join("spellbook");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn a_spellbook_that_hangs_is_stopped_and_the_check_times_out() {
    let d = tempfile::tempdir().unwrap();
    let hanging = program(d.path(), "sleep 30");
    let checker = SpellbookChecker::running(d.path().to_path_buf(), hanging)
        .with_timeout(Duration::from_millis(300));
    let started = Instant::now();
    let read = checker.findings_in(&[(PathBuf::from("src/a.py"), "x = 1\n".into())]);
    assert_eq!(read, RulesRead::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_spellbook_that_answers_in_time_is_read() {
    let d = tempfile::tempdir().unwrap();
    let answering = program(d.path(), "cat >/dev/null; echo '{\"findings\": []}'");
    let checker = SpellbookChecker::running(d.path().to_path_buf(), answering)
        .with_timeout(Duration::from_secs(5));
    let read = checker.findings_in(&[(PathBuf::from("src/a.py"), "x = 1\n".into())]);
    assert_eq!(read, RulesRead::Checked(Vec::new()));
}
