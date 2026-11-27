//! A change against the project's checked rules: an answer that breaks one is not used, the
//! writer is asked once more with the rule named, and a second break holds the change back.
//! The rules are a double that forbids a word; spellbook's own check is tested in its crate.

mod support;

use conjure::domain::record::Attempt;
use conjure::domain::spellbook_findings::RulesOutcome;
use conjure::usecases::conjure_change::{ChangeDeps, ChangeRequest, conjure_change};
use support::*;

/// A cart whose `total` reaches for the database the rule forbids.
const SQLITE_REPLY: &str = "<goal>total from the database</goal>\n<file path=\"src/shop/cart.py\">\nimport sqlite3\n\n\nclass Cart:\n    def total(self):\n        return sqlite3.connect(\"cart.db\").total()\n\n    def empty(self):\n        return True\n</file>\n";

fn ask_at_total() -> ChangeRequest {
    ChangeRequest {
        file: "src/shop/cart.py".into(),
        line: 3,
        selection: None,
        finding: None,
        attempt: Attempt::First,
        situation: None,
    }
}

fn then(first: &str, second: &str) -> String {
    format!("{first}---next---\n{second}")
}

#[test]
fn an_answer_that_breaks_a_rule_is_asked_again_and_the_second_lands() {
    let d = cart_project();
    let rig = Rig::new(d.path(), &then(SQLITE_REPLY, CART_REPLY));
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let deps = ChangeDeps {
        checker: &rule,
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(change.goal.as_deref(), Some("fill total"));
    assert_eq!(change.files.len(), 2);
    let RulesOutcome::AskedAgain(broken) = &change.rules else {
        panic!("{:?}", change.rules)
    };
    assert_eq!(broken.len(), 2, "{broken:?}");
    assert_eq!((broken[0].line, broken[1].line), (1, 6));
    let seen = rig.writer.situations();
    assert_eq!(seen.len(), 2);
    assert!(
        seen[1].starts_with(&seen[0]),
        "the second reads the first's situation"
    );
    let added = &seen[1][seen[0].len()..];
    assert!(
        added.contains(
            "- src/shop/cart.py:1 `import sqlite3` breaks commands-use-ports: No sqlite3 here\n"
        ),
        "{added}"
    );
    assert!(added.contains("Take the step again without breaking them."));
    assert_eq!(
        change.situation, seen[1],
        "Show why and Try harder read the second"
    );
    assert!((change.errand.cost_usd.unwrap() - 0.002).abs() < 1e-9);
}

#[test]
fn a_second_break_holds_the_change_back_and_says_why() {
    let d = cart_project();
    let rig = Rig::new(d.path(), SQLITE_REPLY);
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let deps = ChangeDeps {
        checker: &rule,
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert!(change.files.is_empty(), "nothing lands");
    assert!(matches!(&change.rules, RulesOutcome::HeldBack(broken) if broken.len() == 2));
    assert_eq!(
        change.goal.as_deref(),
        Some(
            "held back: it breaks commands-use-ports at cart.py:1 (No sqlite3 here); commands-use-ports at cart.py:6 (No sqlite3 here), asked twice"
        )
    );
    assert_eq!(rig.writer.situations().len(), 2, "asked twice, no more");
}

#[test]
fn a_break_the_code_had_before_is_not_the_changes() {
    let d = cart_project();
    write(
        d.path(),
        "src/shop/cart.py",
        "import sqlite3\n\n\nclass Cart:\n    def total(self):\n        pass\n\n    def empty(self):\n        return True\n",
    );
    let kept = "<goal>total is zero</goal>\n<file path=\"src/shop/cart.py\">\nimport sqlite3\n\n\nclass Cart:\n    def total(self):\n        return 0\n\n    def empty(self):\n        return True\n</file>\n";
    let rig = Rig::new(d.path(), kept);
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let deps = ChangeDeps {
        checker: &rule,
        ..rig.deps()
    };
    let change = conjure_change(
        &ChangeRequest {
            line: 6,
            ..ask_at_total()
        },
        &deps,
    )
    .unwrap();
    assert_eq!(change.rules, RulesOutcome::Kept);
    assert_eq!(change.files.len(), 1);
    assert_eq!(rig.writer.situations().len(), 1);
    assert_eq!(rule.times_asked(), 2, "the new texts, then the old ones");
}

#[test]
fn a_warning_or_no_checker_keeps_the_first_answer() {
    let d = cart_project();
    let rig = Rig::new(d.path(), SQLITE_REPLY);
    let warning = ForbiddenWord {
        severity: "warn".into(),
        ..ForbiddenWord::error("sqlite3", "commands-use-ports")
    };
    let deps = ChangeDeps {
        checker: &warning,
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(change.rules, RulesOutcome::Kept);
    assert_eq!(
        warning.times_asked(),
        1,
        "no error, so the old texts are not asked about"
    );
    let change = conjure_change(&ask_at_total(), &rig.deps()).unwrap();
    assert_eq!(change.rules, RulesOutcome::Kept);
    assert_eq!(change.files.len(), 1);
}

#[test]
fn an_answer_of_nothing_is_not_checked() {
    let d = cart_project();
    let rig = Rig::new(d.path(), "<goal>nothing</goal>\n");
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let deps = ChangeDeps {
        checker: &rule,
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(change.rules, RulesOutcome::Kept);
    assert_eq!(rule.times_asked(), 1, "asked about no texts");
}
