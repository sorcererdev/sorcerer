//! What a rules retry dropped of the first answer: compared by the language's queries over the
//! whole change, judged by a decider double, said in the step and never holding the change back.

mod support;

use conjure::domain::drop_notes::DropNotes;
use conjure::domain::record::Attempt;
use conjure::domain::retry_drops::{
    DropVerdict, NameKind, WhyRemoved, quiet_drops_text, retry_step_text, said_drops_text,
};
use conjure::usecases::conjure_change::{ChangeDeps, ChangeRequest, conjure_change};
use support::*;

/// The first answer: `total` saves through `sqlite3`, which the rule forbids.
const FIRST: &str = "<goal>total, saved</goal>\n<file path=\"src/shop/cart.py\">\nimport sqlite3\n\n\nclass Cart:\n    def total(self):\n        save_total(sqlite3, 0)\n        return 0\n\n    def empty(self):\n        return True\n</file>\n";

/// A retry that keeps the rule by deleting the save: behaviour lost.
const DELETES_THE_SAVE: &str = "<goal>fill total</goal>\n<file path=\"src/shop/cart.py\">\nclass Cart:\n    def total(self):\n        return 0\n\n    def empty(self):\n        return True\n</file>\n";

/// A retry that keeps the rule by moving the save into a new file of the change.
const MOVES_THE_SAVE: &str = "<goal>fill total through a store</goal>\n<file path=\"src/shop/cart.py\">\nfrom shop.store import keep\n\n\nclass Cart:\n    def total(self):\n        keep(0)\n        return 0\n\n    def empty(self):\n        return True\n</file>\n<file path=\"src/shop/store.py\">\ndef keep(total):\n    save_total(None, total)\n</file>\n";

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

const LOST: [(&str, f64); 3] = [
    ("rule_asked_for_removal", 0.1),
    ("moved_elsewhere", 0.05),
    ("behaviour_lost", 0.85),
];

#[test]
fn a_save_the_retry_deleted_is_a_drop_jev_calls_unintended_and_the_change_still_lands() {
    let d = cart_project();
    let rig = Rig::new(d.path(), &then(FIRST, DELETES_THE_SAVE));
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let jev = SameAnswer::choosing(&LOST);
    let deps = ChangeDeps {
        checker: &rule,
        decider: Some(&jev),
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(change.files.len(), 1, "never held back for a drop");
    assert_eq!(change.retry_drops.len(), 1, "{:?}", change.retry_drops);
    let drop = &change.retry_drops[0];
    assert_eq!(
        (drop.dropped.kind, drop.dropped.name.as_str()),
        (NameKind::Call, "save_total")
    );
    assert_eq!(
        (drop.dropped.file.as_str(), drop.dropped.line),
        ("src/shop/cart.py", 6)
    );
    assert_eq!(drop.verdict, DropVerdict::LooksUnintended);
    assert_eq!(
        drop.rules,
        vec!["commands-use-ports: No sqlite3 here".to_string()]
    );
    assert_eq!(
        retry_step_text(&change.rules, &change.retry_drops),
        " \u{b7} asked again for commands-use-ports \u{b7} dropped: save_total in cart.py (looks unintended)"
    );
    let questions = jev.questions();
    assert_eq!(questions.len(), 1, "one question per drop");
    let context: Vec<&str> = questions[0]
        .context
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        context,
        ["goal", "rules", "first_answer", "retry", "dropped-0"]
    );
    let text_of = |name: &str| {
        questions[0]
            .context
            .iter()
            .find(|(n, _)| n == name)
            .unwrap()
            .1
            .clone()
    };
    assert!(text_of("rules").contains("commands-use-ports (src/shop/cart.py:1)"));
    assert!(text_of("first_answer").contains("+        save_total(sqlite3, 0)"));
    assert_eq!(
        text_of("dropped-0"),
        "the call `save_total` at src/shop/cart.py:6: `save_total(sqlite3, 0)`"
    );
}

#[test]
fn a_save_moved_into_another_file_of_the_change_is_no_drop_and_jev_is_not_asked() {
    let d = cart_project();
    let rig = Rig::new(d.path(), &then(FIRST, MOVES_THE_SAVE));
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let jev = SameAnswer::choosing(&LOST);
    let deps = ChangeDeps {
        checker: &rule,
        decider: Some(&jev),
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(change.files.len(), 2);
    assert!(change.retry_drops.is_empty(), "{:?}", change.retry_drops);
    assert!(jev.questions().is_empty());
    assert_eq!(
        retry_step_text(&change.rules, &change.retry_drops),
        " \u{b7} asked again for commands-use-ports"
    );
}

#[test]
fn a_drop_jev_calls_legitimate_is_said_only_quietly() {
    let d = cart_project();
    let rig = Rig::new(d.path(), &then(FIRST, DELETES_THE_SAVE));
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let jev = SameAnswer::choosing(&[
        ("rule_asked_for_removal", 0.8),
        ("moved_elsewhere", 0.1),
        ("behaviour_lost", 0.1),
    ]);
    let deps = ChangeDeps {
        checker: &rule,
        decider: Some(&jev),
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(
        change.retry_drops[0].verdict,
        DropVerdict::Legitimate(WhyRemoved::RuleAskedForIt)
    );
    assert_eq!(said_drops_text(&change.retry_drops), "");
    assert_eq!(
        quiet_drops_text(&change.retry_drops),
        "dropped on purpose: save_total in cart.py (the rule asked for it)"
    );
    let notes = DropNotes::default().with_change(
        "abc123",
        "2026-11-22T12:00:00Z".parse().unwrap(),
        &change.retry_drops,
    );
    assert!(
        notes.notes.is_empty(),
        "a legitimate drop is not left for scry"
    );
}

#[test]
fn an_unsure_answer_is_said_and_no_jev_is_said_as_not_checked() {
    let d = cart_project();
    let unsure = SameAnswer::choosing(&[
        ("rule_asked_for_removal", 0.3),
        ("moved_elsewhere", 0.2),
        ("behaviour_lost", 0.5),
    ]);
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let rig = Rig::new(d.path(), &then(FIRST, DELETES_THE_SAVE));
    let deps = ChangeDeps {
        checker: &rule,
        decider: Some(&unsure),
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(change.retry_drops[0].verdict, DropVerdict::Unsure);
    assert_eq!(
        said_drops_text(&change.retry_drops),
        "dropped: save_total in cart.py (maybe unintended)"
    );

    let rig = Rig::new(d.path(), &then(FIRST, DELETES_THE_SAVE));
    let deps = ChangeDeps {
        checker: &rule,
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert_eq!(change.retry_drops[0].verdict, DropVerdict::NotChecked);
    assert_eq!(
        said_drops_text(&change.retry_drops),
        "dropped: save_total in cart.py (not checked)"
    );
    let notes = DropNotes::default().with_change(
        "abc123",
        "2026-11-22T12:00:00Z".parse().unwrap(),
        &change.retry_drops,
    );
    assert_eq!(notes.notes.len(), 1);
    assert_eq!(notes.without_change("abc123").notes.len(), 0);
}

#[test]
fn a_first_answer_that_keeps_the_rules_is_not_compared() {
    let d = cart_project();
    let rig = Rig::new(d.path(), CART_REPLY);
    let rule = ForbiddenWord::error("sqlite3", "commands-use-ports");
    let jev = SameAnswer::choosing(&LOST);
    let deps = ChangeDeps {
        checker: &rule,
        decider: Some(&jev),
        ..rig.deps()
    };
    let change = conjure_change(&ask_at_total(), &deps).unwrap();
    assert!(change.retry_drops.is_empty());
    assert!(jev.questions().is_empty());
    assert_eq!(retry_step_text(&change.rules, &change.retry_drops), "");
}

#[test]
fn a_call_is_named_by_its_callee_with_argument_lists_cut_to_their_brackets() {
    use conjure::ports::project_context::ProjectContext;
    let d = cart_project();
    let rig = Rig::new(d.path(), CART_REPLY);
    let text = "fn seat(&self, line: String) {\n    std::fs::write(\"walk-ins.txt\", line).ok();\n    self.clock.append_line(format!(\"{}\", 1));\n}\n";
    let names = rig
        .context
        .names_in(std::path::Path::new("src/seat.rs"), text)
        .expect("a grammar covers the file");
    let calls: Vec<(&str, u32)> = names
        .iter()
        .filter(|named| named.kind == NameKind::Call)
        .map(|named| (named.name.as_str(), named.line))
        .collect();
    assert!(calls.contains(&("std::fs::write", 2)), "{calls:?}");
    assert!(calls.contains(&("std::fs::write().ok", 2)), "{calls:?}");
    assert!(calls.contains(&("self.clock.append_line", 3)), "{calls:?}");
    assert!(calls.contains(&("format", 3)), "{calls:?}");
    assert!(
        names
            .iter()
            .any(|named| named.kind == NameKind::Definition && named.name == "seat")
    );
}
