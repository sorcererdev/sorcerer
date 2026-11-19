mod support;

use sorcerer_core::domain::compare::{UnitChange, compare_units};
use sorcerer_core::domain::unit::units_of;

fn units(lang: &str, text: &str) -> Vec<sorcerer_core::domain::unit::Unit> {
    let l = support::languages_with_vendored_grammars()
        .load(lang, &support::test_cache())
        .unwrap();
    units_of(&l, text).unwrap()
}

fn summary(cs: &[UnitChange]) -> Vec<String> {
    cs.iter()
        .map(|c| match c {
            UnitChange::Added(u) => format!("added {}", u.name),
            UnitChange::Changed { after, .. } => format!("changed {}", after.name),
            UnitChange::Removed(u) => format!("removed {}", u.name),
            UnitChange::Same(u) => format!("same {}", u.name),
        })
        .collect()
}

const PY: &str =
    "class Order:\n    def cancel(self):\n        return None\n\ndef top():\n    pass\n";

#[test]
fn units_carry_their_fingerprint() {
    let us = units("python", "def f():\n    return 1\n");
    assert_eq!(us[0].fingerprint.to_string(), "aee3e55");
}

#[test]
fn an_edit_inside_a_method_changes_the_method_and_its_class() {
    let after = PY.replace("return None", "return 1");
    let cs = compare_units(&units("python", PY), &units("python", &after));
    assert_eq!(
        summary(&cs),
        vec!["changed Order", "changed Order.cancel", "same top"]
    );
}

#[test]
fn a_new_function_is_added_and_a_deleted_one_removed() {
    let after = PY.replace("def top():\n    pass\n", "def other():\n    pass\n");
    let cs = compare_units(&units("python", PY), &units("python", &after));
    assert_eq!(
        summary(&cs),
        vec![
            "same Order",
            "same Order.cancel",
            "added other",
            "removed top"
        ]
    );
}

#[test]
fn two_units_of_one_name_pair_in_order() {
    let rs = "impl Order {\n    fn a() {}\n}\n\nimpl Display for Order {\n    fn fmt() {}\n}\n";
    let after = rs.replace("fn fmt() {}", "fn fmt() { x }");
    let cs = compare_units(&units("rust", rs), &units("rust", &after));
    assert_eq!(
        summary(&cs),
        vec![
            "same Order",
            "same Order::a",
            "changed Order",
            "changed Order::fmt"
        ]
    );
}

#[test]
fn a_formatter_run_is_all_same() {
    let before = "def f(a, b):\n    return g(a, b)\n";
    let after = "def f(\n    a,\n    b,\n):\n    return g(\n        a,\n        b,\n    )\n";
    let cs = compare_units(&units("python", before), &units("python", after));
    assert_eq!(summary(&cs), vec!["same f"]);
}

#[test]
fn crlf_line_endings_change_nothing() {
    let cs = compare_units(
        &units("python", PY),
        &units("python", &PY.replace('\n', "\r\n")),
    );
    assert!(
        summary(&cs).iter().all(|s| s.starts_with("same")),
        "{:?}",
        summary(&cs)
    );
}
