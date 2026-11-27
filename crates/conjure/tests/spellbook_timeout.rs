//! The live checker gives up on a spellbook that hangs: the check is stopped after its timeout
//! and answers `TimedOut`, so the change goes ahead unchecked. This test program stands in for
//! spellbook (the checker runs it as `check --texts - --json`), so it needs no shell and runs
//! on every platform; it has its own `main` for that.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use conjure::adapters::spellbook_checker::SpellbookChecker;
use conjure::domain::spellbook_findings::RulesRead;
use conjure::ports::rule_checker::RuleChecker;

fn main() {
    // run as the stand-in spellbook: `HANG` hangs, anything else answers no findings
    if std::env::args().nth(1).as_deref() == Some("check") {
        let mut input = String::new();
        let _ = std::io::stdin().read_to_string(&mut input);
        if std::env::var_os("STAND_IN_HANGS").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
        println!("{{\"findings\": []}}");
        return;
    }
    a_spellbook_that_hangs_is_stopped_and_the_check_times_out();
    a_spellbook_that_answers_in_time_is_read();
    println!("spellbook_timeout: ok");
}

fn checker(timeout: Duration) -> SpellbookChecker {
    let me = std::env::current_exe().unwrap();
    SpellbookChecker::running(std::env::temp_dir(), me).with_timeout(timeout)
}

fn texts() -> Vec<(PathBuf, String)> {
    vec![(PathBuf::from("src/a.py"), "x = 1\n".into())]
}

fn a_spellbook_that_hangs_is_stopped_and_the_check_times_out() {
    // the child inherits this variable, so it hangs
    unsafe { std::env::set_var("STAND_IN_HANGS", "1") };
    let started = Instant::now();
    let read = checker(Duration::from_millis(500)).findings_in(&texts());
    unsafe { std::env::remove_var("STAND_IN_HANGS") };
    assert_eq!(read, RulesRead::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(10));
}

fn a_spellbook_that_answers_in_time_is_read() {
    let read = checker(Duration::from_secs(20)).findings_in(&texts());
    assert_eq!(read, RulesRead::Checked(Vec::new()));
}
